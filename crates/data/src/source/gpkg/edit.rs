//! Whole-table editing of an existing GeoPackage, with optimistic row versions and atomic saves.

use super::{CHUNK, GpkgSource, err, header_blob, is_empty_blob, quote, split_blob, sql_value, value};
use crate::raster::Crs;
use crate::source::SourceInfo;
use crate::source::edits::{Conflict, Edits, Saved};
use crate::{DataError, Feature, Field, Geometry, Result, Value, wkb};
use rusqlite::config::DbConfig;
use rusqlite::functions::FunctionFlags;
use rusqlite::types::{Value as Sql, ValueRef};
use rusqlite::{Connection, OpenFlags, TransactionBehavior, params, params_from_iter};
use rustgis_geom::Extent;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::time::Duration;

/// A whole-table editing session is bounded, unlike a streamed map source.
pub const MAX_EDITABLE: usize = 100_000;

/// Rows loaded for editing, including geometry-less rows but excluding unreadable geometries.
pub struct Editable {
    pub features: Vec<Feature>,
    pub versions: Vec<(u64, i64)>,
    pub matched: u64,
    pub skipped: u64,
    pub writable: bool,
}

/// Stored row versions and snapshots, read in the write transaction before it commits.
#[derive(Debug)]
pub struct SavedRows {
    pub changes: Saved,
    /// Every requested update (even unchanged) followed by inserts, with committed keys.
    /// Attributes follow the supplied fields; fields absent from the table retain submitted values.
    pub features: Vec<Feature>,
}

/// An existing feature table; its read connection is never reused for writes.
pub struct EditTable {
    src: GpkgSource,
    path: PathBuf,
    columns: Vec<String>,
    geometry_type: String,
    srs_id: i64,
    z: i64,
    m: i64,
}

impl EditTable {
    pub fn open(path: &Path, table: &str) -> Result<Self> {
        let src = GpkgSource::open(path, Some(table))?;
        let (columns, geometry_type, srs_id, z, m) = {
            let guard = src.conn.lock().map_err(|_| DataError::Format("GeoPackage connection poisoned".into()))?;
            let conn = guard.as_ref().ok_or_else(|| DataError::Format("GeoPackage connection closed".into()))?;
            let cols = table_columns(conn, &src.table)?;
            let mut keys = cols.iter().filter(|c| c.pk > 0);
            let key = keys.next();
            if keys.next().is_some()
                || !key.is_some_and(|c| c.ty.trim().eq_ignore_ascii_case("INTEGER") && src.pk.as_deref() == Some(c.name.as_str()))
            {
                return Err(DataError::Format(format!("{table}: no INTEGER PRIMARY KEY, so its rows can't be told apart to save")));
            }
            let (ty, srs, z, m) = conn
                .query_row("SELECT geometry_type_name, srs_id, z, m FROM gpkg_geometry_columns WHERE table_name = ?1", [&src.table], |r| {
                    Ok((r.get::<_, String>(0)?, r.get::<_, i64>(1)?, r.get::<_, i64>(2)?, r.get::<_, i64>(3)?))
                })
                .map_err(err)?;
            (cols.into_iter().map(|c| c.name).collect(), ty.to_ascii_uppercase(), srs, z, m)
        };
        Ok(Self { src, path: path.to_path_buf(), columns, geometry_type, srs_id, z, m })
    }

    pub fn info(&self) -> &SourceInfo {
        &self.src.info
    }

    pub fn crs(&self) -> Option<Crs> {
        (!self.src.reproject.is_lonlat()).then_some(self.src.reproject.crs)
    }

    pub fn table(&self) -> &str {
        &self.src.table
    }

    pub fn key(&self) -> &str {
        self.src.pk.as_deref().unwrap_or("")
    }

    pub fn geometry_column(&self) -> &str {
        &self.src.geom
    }

    pub fn read_editable(&self, limit: usize) -> Result<Editable> {
        let guard = self.src.conn.lock().map_err(|_| DataError::Format("GeoPackage connection poisoned".into()))?;
        let conn = guard.as_ref().ok_or_else(|| DataError::Format("GeoPackage connection closed".into()))?;
        // Count and baseline come from the same snapshot, even if another process is writing.
        let tx = conn.unchecked_transaction().map_err(err)?;
        let matched = tx.query_row(&format!("SELECT COUNT(*) FROM {}", quote(self.table())), [], |r| r.get::<_, i64>(0)).map_err(err)?;
        let matched = u64::try_from(matched).map_err(|_| DataError::Format("GeoPackage: negative row count".into()))?;
        let (key, geom) = self.positions(&self.columns)?;
        let attrs = self
            .info()
            .fields
            .iter()
            .map(|f| self.columns.iter().position(|c| c.eq_ignore_ascii_case(&f.name)).ok_or_else(|| self.shape_error()))
            .collect::<Result<Vec<_>>>()?;
        let mut out = Editable {
            features: Vec::new(),
            versions: Vec::new(),
            matched,
            skipped: 0,
            writable: std::fs::metadata(&self.path).is_ok_and(|m| !m.permissions().readonly()),
        };
        {
            let mut st = tx
                .prepare(&format!(
                    "SELECT {} FROM {} ORDER BY {} LIMIT ?1",
                    select_list(&self.columns),
                    quote(self.table()),
                    quote(self.key())
                ))
                .map_err(err)?;
            let mut rows = st.query([limit.clamp(1, MAX_EDITABLE) as i64]).map_err(err)?;
            while let Some(row) = rows.next().map_err(err)? {
                let id = row.get::<_, i64>(key).map_err(err)?.cast_unsigned();
                let Some(geometry) = self.geometry(row.get_ref(geom).map_err(err)?) else {
                    out.skipped += 1;
                    continue;
                };
                let mut hash = FNV_OFFSET;
                for i in 0..self.columns.len() {
                    hash_value(&mut hash, row.get_ref(i).map_err(err)?);
                }
                let attrs = self
                    .info()
                    .fields
                    .iter()
                    .zip(&attrs)
                    .map(|(f, &i)| row.get_ref(i).map(|v| value(v, f.ty)))
                    .collect::<rusqlite::Result<_>>()
                    .map_err(err)?;
                out.versions.push((id, hash.cast_signed()));
                out.features.push(Feature { id, geometry, attrs });
            }
        }
        tx.commit().map_err(err)?;
        Ok(out)
    }

    /// Returns conflicts without changing anything; every other failure also rolls back the save.
    /// `reserved` must include every loaded baseline key, including rows not changed by this save.
    /// New positive keys exceed those keys, the table's current maximum and its AUTOINCREMENT
    /// history. Stored snapshots and their versions are decoded together before commit; an
    /// unreadable or trigger-removed row aborts the whole transaction.
    pub fn save(
        &self,
        edits: &Edits,
        fields: &[Field],
        reserved: impl Iterator<Item = u64>,
    ) -> Result<std::result::Result<SavedRows, Vec<Conflict>>> {
        let mut conn =
            Connection::open_with_flags(&self.path, OpenFlags::SQLITE_OPEN_READ_WRITE | OpenFlags::SQLITE_OPEN_NO_MUTEX).map_err(err)?;
        conn.set_db_config(DbConfig::SQLITE_DBCONFIG_DEFENSIVE, true).map_err(err)?;
        conn.set_db_config(DbConfig::SQLITE_DBCONFIG_TRUSTED_SCHEMA, false).map_err(err)?;
        conn.busy_timeout(Duration::from_secs(5)).map_err(err)?;
        register_functions(&conn)?;
        let tx = conn.transaction_with_behavior(TransactionBehavior::Immediate).map_err(err)?;
        let columns: Vec<String> = table_columns(&tx, self.table())?.into_iter().map(|c| c.name).collect();
        let (key, geom) = self.positions(&columns)?;
        let mut saved = Saved::default();
        let mut targets = Vec::new();
        let mut snapshot_columns = Vec::new();
        for (i, f) in fields.iter().enumerate() {
            if let Some(c) = columns.iter().position(|c| c.eq_ignore_ascii_case(&f.name)) {
                snapshot_columns.push((i, c));
                if c != key && c != geom {
                    targets.push((i, c));
                } else {
                    saved.skipped_fields.push(f.name.clone());
                }
            } else {
                saved.skipped_fields.push(f.name.clone());
            }
        }
        let keys: Vec<i64> =
            edits.updates.iter().map(|(f, _)| f.id.cast_signed()).chain(edits.deletes.iter().map(|(k, _)| k.cast_signed())).collect();
        let current = self.fetch_rows(&tx, &columns, key, &keys)?;
        let mut conflicts = Vec::new();
        for (k, version) in edits.updates.iter().map(|(f, v)| (f.id, *v)).chain(edits.deletes.iter().copied()) {
            match current.get(&k.cast_signed()) {
                None => conflicts.push(Conflict::Deleted(k)),
                Some(row) if fingerprint(row.iter().map(ValueRef::from)) != version => conflicts.push(Conflict::Changed(k)),
                Some(_) => {}
            }
        }
        if !conflicts.is_empty() {
            return Ok(Err(conflicts));
        }

        // A file with no R-tree triggers still needs its existing index kept in step.
        let manual_rtree = if let Some(rt) = &self.src.rtree {
            let triggered: bool = tx.query_row(
                "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type = 'trigger' AND lower(tbl_name) = lower(?1) AND instr(lower(sql), lower(?2)) > 0)",
                params![self.table(), rt], |r| r.get(0),
            ).map_err(err)?;
            (!triggered).then_some(rt.as_str())
        } else {
            None
        };
        let table = quote(self.table());
        let pk = quote(self.key());
        let mut written_extent = Extent::EMPTY;
        for (f, _) in &edits.updates {
            let row = current.get(&f.id.cast_signed()).ok_or_else(|| self.shape_error())?;
            let mut sets = Vec::new();
            let mut args = Vec::new();
            for &(i, c) in &targets {
                let field = fields.get(i).ok_or_else(|| self.shape_error())?;
                let old = row.get(c).ok_or_else(|| self.shape_error())?;
                let attr = f.attrs.get(i).unwrap_or(&Value::Null);
                if value(ValueRef::from(old), field.ty) != *attr {
                    sets.push(format!("{} = ?", quote(columns.get(c).ok_or_else(|| self.shape_error())?)));
                    args.push(sql_value(attr, field.ty));
                }
            }
            let old = ValueRef::from(row.get(geom).ok_or_else(|| self.shape_error())?);
            let geometry_changed = self.geometry(old).ok_or_else(|| self.shape_error())? != f.geometry;
            let mut extent = Extent::EMPTY;
            if geometry_changed {
                let (blob, e) = self.geometry_blob(f.geometry.as_ref(), Some((f.id, old)))?;
                extent = e;
                sets.push(format!("{} = ?", quote(self.geometry_column())));
                args.push(blob);
            }
            if sets.is_empty() {
                saved.unchanged += 1;
                continue;
            }
            args.push(Sql::Integer(f.id.cast_signed()));
            tx.execute(&format!("UPDATE {table} SET {} WHERE {pk} = ?", sets.join(", ")), params_from_iter(args)).map_err(err)?;
            saved.updated += 1;
            if geometry_changed {
                written_extent = written_extent.union(&extent);
                maintain_rtree(&tx, manual_rtree, f.id.cast_signed(), &extent)?;
            }
        }
        // Reserve the loaded baseline as well as live and historical keys. A deleted source row
        // can still be present in the layer, so SQLite's ordinary rowid allocation is not enough.
        let mut high_water = 0;
        if !edits.inserts.is_empty() {
            high_water = reserved.filter_map(|k| i64::try_from(k).ok()).max().unwrap_or(0).max(0);
            let current_max: Option<i64> = tx.query_row(&format!("SELECT max({pk}) FROM {table}"), [], |r| r.get(0)).map_err(err)?;
            high_water = high_water.max(current_max.unwrap_or(0));
            let has_sequence: bool = tx
                .query_row("SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type = 'table' AND name = 'sqlite_sequence')", [], |r| r.get(0))
                .map_err(err)?;
            if has_sequence {
                let sequence: Option<i64> = tx
                    .query_row("SELECT max(seq) FROM sqlite_sequence WHERE lower(name) = lower(?1)", [self.table()], |r| r.get(0))
                    .map_err(err)?;
                high_water = high_water.max(sequence.unwrap_or(0));
            }
        }
        for f in &edits.inserts {
            let new_key = high_water.checked_add(1).ok_or_else(|| {
                DataError::Format(format!("{}: no positive INTEGER PRIMARY KEY remains above the reserved keys", self.table()))
            })?;
            high_water = new_key;
            let mut names = vec![pk.clone()];
            let mut args = vec![Sql::Integer(new_key)];
            let mut extent = Extent::EMPTY;
            if let Some(g) = &f.geometry {
                let (blob, e) = self.geometry_blob(Some(g), None)?;
                extent = e;
                names.push(quote(self.geometry_column()));
                args.push(blob);
            }
            for &(i, c) in &targets {
                let attr = f.attrs.get(i).unwrap_or(&Value::Null);
                if *attr != Value::Null {
                    let field = fields.get(i).ok_or_else(|| self.shape_error())?;
                    names.push(quote(columns.get(c).ok_or_else(|| self.shape_error())?));
                    args.push(sql_value(attr, field.ty));
                }
            }
            let sql = format!("INSERT INTO {table} ({}) VALUES ({})", names.join(", "), vec!["?"; names.len()].join(", "));
            tx.execute(&sql, params_from_iter(args)).map_err(err)?;
            let key = new_key;
            saved.inserted.push((f.id, key.cast_unsigned()));
            maintain_rtree(&tx, manual_rtree, key, &extent)?;
            written_extent = written_extent.union(&extent);
        }
        for (k, _) in &edits.deletes {
            saved.deleted += tx.execute(&format!("DELETE FROM {table} WHERE {pk} = ?1"), [k.cast_signed()]).map_err(err)?;
            maintain_rtree(&tx, manual_rtree, k.cast_signed(), &Extent::EMPTY)?;
        }
        tx.execute(
            "UPDATE gpkg_contents SET last_change = strftime('%Y-%m-%dT%H:%M:%fZ','now') WHERE lower(table_name) = lower(?1)",
            [self.table()],
        )
        .map_err(err)?;
        if !written_extent.is_empty() {
            tx.execute(
                "UPDATE gpkg_contents SET min_x = min(min_x, ?2), min_y = min(min_y, ?3), max_x = max(max_x, ?4), max_y = max(max_y, ?5)
                 WHERE lower(table_name) = lower(?1) AND min_x IS NOT NULL AND min_y IS NOT NULL AND max_x IS NOT NULL AND max_y IS NOT NULL",
                params![self.table(), written_extent.min_x, written_extent.min_y, written_extent.max_x, written_extent.max_y],
            ).map_err(err)?;
        }
        let keys: Vec<i64> =
            edits.updates.iter().map(|(f, _)| f.id.cast_signed()).chain(saved.inserted.iter().map(|(_, k)| k.cast_signed())).collect();
        let rows = self.fetch_rows(&tx, &columns, key, &keys)?;
        let mut features = Vec::with_capacity(keys.len());
        let originals = edits
            .updates
            .iter()
            .map(|(f, _)| (f, f.id.cast_signed()))
            .chain(edits.inserts.iter().zip(&saved.inserted).map(|(f, (_, k))| (f, k.cast_signed())));
        for (original, k) in originals {
            let row = rows.get(&k).ok_or_else(|| DataError::Format(format!("row {k}: it was removed while saving")))?;
            let geometry = self
                .geometry(ValueRef::from(row.get(geom).ok_or_else(|| self.shape_error())?))
                .ok_or_else(|| DataError::Format(format!("row {k}: its stored geometry can't be read, so nothing was saved")))?;
            let mut feature = original.clone();
            feature.id = k.cast_unsigned();
            feature.geometry = geometry;
            feature.attrs.resize(fields.len(), Value::Null);
            for &(i, c) in &snapshot_columns {
                feature.attrs[i] = value(ValueRef::from(row.get(c).ok_or_else(|| self.shape_error())?), fields[i].ty);
            }
            saved.versions.push((k.cast_unsigned(), fingerprint(row.iter().map(ValueRef::from))));
            features.push(feature);
        }
        tx.commit().map_err(err)?;
        Ok(Ok(SavedRows { changes: saved, features }))
    }

    fn shape_error(&self) -> DataError {
        DataError::Format(format!("{} changed shape since it was loaded; Discard Edits loads it again", self.table()))
    }

    fn positions(&self, columns: &[String]) -> Result<(usize, usize)> {
        let key = columns.iter().position(|c| c.eq_ignore_ascii_case(self.key()));
        let geom = columns.iter().position(|c| c.eq_ignore_ascii_case(self.geometry_column()));
        key.zip(geom).ok_or_else(|| self.shape_error())
    }

    // Outer None is unreadable; inner None is a valid NULL or empty geometry.
    fn geometry(&self, v: ValueRef<'_>) -> Option<Option<Geometry>> {
        match v {
            ValueRef::Null => Some(None),
            ValueRef::Blob(b) if is_empty_blob(b) => Some(None),
            ValueRef::Blob(b) => {
                let (_, body) = split_blob(b)?;
                self.src.reproject.geometry(wkb::decode(body)?).map(Some)
            }
            _ => None,
        }
    }

    fn geometry_blob(&self, g: Option<&Geometry>, row: Option<(u64, ValueRef<'_>)>) -> Result<(Sql, Extent)> {
        // Check before returning NULL: removing geometry must not silently drop existing Z/M.
        let has_zm = row.is_some_and(|(_, v)| match v {
            ValueRef::Blob(b) => split_blob(b).is_some_and(|(_, body)| wkb::has_zm(body)),
            _ => false,
        });
        if self.z == 1 || self.m == 1 || has_zm {
            let message = match row {
                Some((k, _)) => format!(
                    "row {}: its geometry has Z or M values, which RustGIS doesn't keep, so its geometry can't be saved (its attributes can)",
                    k.cast_signed()
                ),
                None => format!(
                    "{} needs Z or M values, which RustGIS doesn't keep, so new features with a geometry can't be saved",
                    self.table()
                ),
            };
            return Err(DataError::Format(message));
        }
        let Some(g) = g else {
            return Ok((Sql::Null, Extent::EMPTY));
        };
        let native = self.src.reproject.to_native(g).ok_or_else(|| {
            let id = row.map(|(k, _)| k.cast_signed().to_string()).unwrap_or_else(|| "new".into());
            DataError::Format(format!("row {id}: its geometry can't be converted to {}", self.src.reproject.label()))
        })?;
        let body = match (self.geometry_type.as_str(), &native) {
            ("POINT", Geometry::Point(_) | Geometry::MultiPoint(_))
            | ("LINESTRING", Geometry::Line(_))
            | ("POLYGON", Geometry::Polygon(_)) => wkb::encode_single(&native).ok_or_else(|| {
                DataError::Format(format!("{}'s {} column holds one part; a feature has several", self.table(), self.geometry_type))
            })?,
            ("MULTIPOINT", Geometry::Point(c)) => wkb::encode(&Geometry::MultiPoint(vec![*c])),
            ("MULTIPOINT", Geometry::MultiPoint(_)) | ("MULTILINESTRING", Geometry::Line(_)) | ("MULTIPOLYGON", Geometry::Polygon(_)) => {
                wkb::encode(&native)
            }
            ("GEOMETRY", _) => wkb::encode_single(&native).unwrap_or_else(|| wkb::encode(&native)),
            ("POINT" | "LINESTRING" | "POLYGON" | "MULTIPOINT" | "MULTILINESTRING" | "MULTIPOLYGON", _) => {
                return Err(DataError::Format(format!(
                    "{}'s {} column can't hold this {:?} geometry",
                    self.table(),
                    self.geometry_type,
                    native.kind()
                )));
            }
            // The model has no collection, curve or surface variant to preserve these types.
            _ => return Err(DataError::Format(format!("{}'s {} geometry can't be written by RustGIS", self.table(), self.geometry_type))),
        };
        let srs = i32::try_from(self.srs_id)
            .map_err(|_| DataError::Format(format!("{}: srs_id {} doesn't fit a GeoPackage geometry header", self.table(), self.srs_id)))?;
        let extent = native.extent();
        Ok((Sql::Blob(header_blob(&body, &extent, srs)), extent))
    }

    fn fetch_rows(&self, conn: &Connection, columns: &[String], key: usize, keys: &[i64]) -> Result<HashMap<i64, Vec<Sql>>> {
        let mut out = HashMap::new();
        for chunk in keys.chunks(CHUNK) {
            let mut st = conn
                .prepare(&format!(
                    "SELECT {} FROM {} WHERE {} IN ({})",
                    select_list(columns),
                    quote(self.table()),
                    quote(self.key()),
                    vec!["?"; chunk.len()].join(",")
                ))
                .map_err(err)?;
            let mut rows = st.query(params_from_iter(chunk.iter())).map_err(err)?;
            while let Some(row) = rows.next().map_err(err)? {
                let k = row.get::<_, i64>(key).map_err(err)?;
                let vals = (0..columns.len()).map(|i| row.get(i)).collect::<rusqlite::Result<Vec<Sql>>>().map_err(err)?;
                out.insert(k, vals);
            }
        }
        Ok(out)
    }
}

struct Column {
    name: String,
    ty: String,
    pk: i64,
}

fn table_columns(conn: &Connection, table: &str) -> Result<Vec<Column>> {
    let mut st = conn.prepare(&format!("PRAGMA table_info({})", quote(table))).map_err(err)?;
    st.query_map([], |r| Ok(Column { name: r.get(1)?, ty: r.get::<_, Option<String>>(2)?.unwrap_or_default(), pk: r.get(5)? }))
        .map_err(err)?
        .collect::<rusqlite::Result<_>>()
        .map_err(err)
}

fn select_list(columns: &[String]) -> String {
    columns.iter().map(|c| quote(c)).collect::<Vec<_>>().join(", ")
}

const FNV_OFFSET: u64 = 0xcbf2_9ce4_8422_2325;
const FNV_PRIME: u64 = 0x100_0000_01b3;

fn fingerprint<'a>(vals: impl Iterator<Item = ValueRef<'a>>) -> i64 {
    let mut hash = FNV_OFFSET;
    for v in vals {
        hash_value(&mut hash, v);
    }
    hash.cast_signed()
}

fn hash_value(hash: &mut u64, v: ValueRef<'_>) {
    let mut bytes = |b: &[u8]| {
        for x in b {
            *hash = (*hash ^ u64::from(*x)).wrapping_mul(FNV_PRIME);
        }
    };
    match v {
        ValueRef::Null => bytes(&[0]),
        ValueRef::Integer(v) => {
            bytes(&[1]);
            bytes(&v.to_le_bytes());
        }
        ValueRef::Real(v) => {
            bytes(&[2]);
            bytes(&v.to_bits().to_le_bytes());
        }
        ValueRef::Text(v) => {
            bytes(&[3]);
            bytes(&(v.len() as u64).to_le_bytes());
            bytes(v);
        }
        ValueRef::Blob(v) => {
            bytes(&[4]);
            bytes(&(v.len() as u64).to_le_bytes());
            bytes(v);
        }
    }
}

fn native_extent(v: ValueRef<'_>) -> Option<Extent> {
    let ValueRef::Blob(b) = v else {
        return None;
    };
    if is_empty_blob(b) {
        return None;
    }
    let (envelope, body) = split_blob(b)?;
    envelope.or_else(|| wkb::decode(body).map(|g| g.extent())).filter(|e| !e.is_empty())
}

fn register_functions(conn: &Connection) -> Result<()> {
    let flags = FunctionFlags::SQLITE_UTF8 | FunctionFlags::SQLITE_DETERMINISTIC | FunctionFlags::SQLITE_INNOCUOUS;
    conn.create_scalar_function("ST_IsEmpty", 1, flags, |ctx| {
        Ok(match ctx.get_raw(0) {
            ValueRef::Blob(b) => Some(i64::from(is_empty_blob(b))),
            _ => None,
        })
    })
    .map_err(err)?;
    for (name, index) in [("ST_MinX", 0), ("ST_MaxX", 1), ("ST_MinY", 2), ("ST_MaxY", 3)] {
        conn.create_scalar_function(name, 1, flags, move |ctx| {
            Ok(native_extent(ctx.get_raw(0)).map(|e| match index {
                0 => e.min_x,
                1 => e.max_x,
                2 => e.min_y,
                _ => e.max_y,
            }))
        })
        .map_err(err)?;
    }
    Ok(())
}

fn maintain_rtree(conn: &Connection, rtree: Option<&str>, key: i64, extent: &Extent) -> Result<()> {
    let Some(rt) = rtree else {
        return Ok(());
    };
    if extent.is_empty() {
        conn.execute(&format!("DELETE FROM {} WHERE id = ?1", quote(rt)), [key]).map_err(err)?;
    } else {
        conn.execute(
            &format!("INSERT OR REPLACE INTO {} VALUES (?1, ?2, ?3, ?4, ?5)", quote(rt)),
            params![key, extent.min_x, extent.max_x, extent.min_y, extent.max_y],
        )
        .map_err(err)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use rustgis_geom::Coord;

    fn line(x: f64) -> Geometry {
        Geometry::Line(vec![vec![Coord::new(x, 2.0), Coord::new(x + 1.0, 3.0)]])
    }

    fn fixture() -> (tempfile::TempDir, PathBuf) {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("roads.gpkg");
        let conn = Connection::open(&path).unwrap();
        conn.execute_batch(
            "CREATE TABLE gpkg_spatial_ref_sys (srs_id INTEGER PRIMARY KEY, organization TEXT, organization_coordsys_id INTEGER);
             INSERT INTO gpkg_spatial_ref_sys VALUES (4326, 'EPSG', 4326);
             CREATE TABLE gpkg_contents (table_name TEXT PRIMARY KEY, last_change TEXT, min_x DOUBLE, min_y DOUBLE, max_x DOUBLE, max_y DOUBLE);
             INSERT INTO gpkg_contents VALUES ('roads', 'before', 0, 0, 2, 3);
             CREATE TABLE gpkg_geometry_columns (table_name TEXT, column_name TEXT, geometry_type_name TEXT, srs_id INTEGER, z INTEGER, m INTEGER);
             INSERT INTO gpkg_geometry_columns VALUES ('roads', 'geom', 'LINESTRING', 4326, 0, 0);
             CREATE TABLE roads (fid INTEGER PRIMARY KEY, geom LINESTRING, name TEXT DEFAULT 'default', lanes INTEGER DEFAULT 2, photo BLOB);
             CREATE TABLE other (v TEXT);
             INSERT INTO other VALUES ('keep');",
        ).unwrap();
        let g = line(1.0);
        conn.execute(
            "INSERT INTO roads VALUES (1, ?1, 'old', 2, x'00ff')",
            [header_blob(&wkb::encode_single(&g).unwrap(), &g.extent(), 4326)],
        )
        .unwrap();
        (dir, path)
    }

    fn blob_at(path: &Path, id: i64) -> Vec<u8> {
        Connection::open(path).unwrap().query_row("SELECT geom FROM roads WHERE fid = ?1", [id], |r| r.get(0)).unwrap()
    }

    fn update(table: &EditTable, geometry: Option<Geometry>) -> Edits {
        let read = table.read_editable(MAX_EDITABLE).unwrap();
        let mut f = read.features[0].clone();
        f.geometry = geometry;
        Edits { updates: vec![(f, read.versions[0].1)], ..Default::default() }
    }

    #[test]
    fn gpkg_single_lines_and_transaction_rollback() {
        let (_dir, path) = fixture();
        let table = EditTable::open(&path, "ROADS").unwrap();
        let saved = table.save(&update(&table, Some(line(4.0))), &table.info().fields, std::iter::empty()).unwrap().unwrap();
        assert_eq!(saved.changes.updated, 1);
        let bytes = blob_at(&path, 1);
        let (_, body) = split_blob(&bytes).unwrap();
        assert_eq!(body.get(1..5), Some(2u32.to_le_bytes().as_slice()));
        assert_eq!(wkb::decode(body), Some(line(4.0)));
        assert_eq!(saved.changes.versions, table.read_editable(MAX_EDITABLE).unwrap().versions);
        let part = vec![Coord::new(5.0, 2.0), Coord::new(6.0, 3.0)];
        let multipart = Geometry::Line(vec![part.clone(), part]);
        assert!(
            table
                .save(&update(&table, Some(multipart)), &table.info().fields, std::iter::empty())
                .unwrap_err()
                .to_string()
                .contains("holds one part")
        );
        assert_eq!(blob_at(&path, 1), bytes);

        let conn = Connection::open(&path).unwrap();
        conn.execute_batch(
            "CREATE TRIGGER no_bad BEFORE INSERT ON roads WHEN NEW.name = 'bad' BEGIN SELECT RAISE(ABORT, 'bad name'); END;",
        )
        .unwrap();
        let read = table.read_editable(MAX_EDITABLE).unwrap();
        let mut changed = read.features[0].clone();
        changed.attrs[0] = Value::Text("partial".into());
        let new = Feature { id: 100, geometry: Some(line(7.0)), attrs: vec![Value::Text("bad".into()), Value::Null] };
        let edits = Edits { updates: vec![(changed, read.versions[0].1)], inserts: vec![new], ..Default::default() };
        let before: String = conn.query_row("SELECT last_change FROM gpkg_contents", [], |r| r.get(0)).unwrap();
        assert!(table.save(&edits, &table.info().fields, std::iter::empty()).unwrap_err().to_string().contains("bad name"));
        assert_eq!(conn.query_row("SELECT name FROM roads WHERE fid = 1", [], |r| r.get::<_, String>(0)).unwrap(), "old");
        assert_eq!(conn.query_row("SELECT count(*) FROM roads", [], |r| r.get::<_, i64>(0)).unwrap(), 1);
        assert_eq!(conn.query_row("SELECT last_change FROM gpkg_contents", [], |r| r.get::<_, String>(0)).unwrap(), before);
        assert_eq!(blob_at(&path, 1), bytes);
    }

    #[test]
    fn gpkg_conflicts_fingerprint_unexposed_columns() {
        let (_dir, path) = fixture();
        let table = EditTable::open(&path, "roads").unwrap();
        let edits = update(&table, Some(line(4.0)));
        let conn = Connection::open(&path).unwrap();
        conn.execute("UPDATE roads SET photo = x'01ff' WHERE fid = 1", []).unwrap();
        assert_eq!(table.save(&edits, &table.info().fields, std::iter::empty()).unwrap().unwrap_err(), vec![Conflict::Changed(1)]);
        assert_eq!(conn.query_row("SELECT last_change FROM gpkg_contents", [], |r| r.get::<_, String>(0)).unwrap(), "before");
        conn.execute("DELETE FROM roads WHERE fid = 1", []).unwrap();
        assert_eq!(table.save(&edits, &table.info().fields, std::iter::empty()).unwrap().unwrap_err(), vec![Conflict::Deleted(1)]);
        assert_ne!(
            fingerprint([ValueRef::Text(b"a"), ValueRef::Text(b"bc")].into_iter()),
            fingerprint([ValueRef::Text(b"ab"), ValueRef::Text(b"c")].into_iter())
        );
        assert_ne!(fingerprint([ValueRef::Text(b"1")].into_iter()), fingerprint([ValueRef::Blob(b"1")].into_iter()));
        assert_ne!(fingerprint([ValueRef::Integer(1)].into_iter()), fingerprint([ValueRef::Real(1.0)].into_iter()));
    }

    #[test]
    fn gpkg_editable_keeps_null_and_empty_but_skips_bad_rows() {
        let (_dir, path) = fixture();
        let conn = Connection::open(&path).unwrap();
        conn.execute(
            "INSERT INTO roads (fid, geom) VALUES (2, NULL), (3, ?1), (4, x'47500003'), (5, 'not geometry')",
            [header_blob(&[], &Extent::EMPTY, 4326)],
        )
        .unwrap();
        let table = EditTable::open(&path, "roads").unwrap();
        let read = table.read_editable(MAX_EDITABLE).unwrap();
        assert_eq!((read.matched, read.skipped, read.features.len()), (5, 2, 3));
        assert_eq!(read.versions.iter().map(|(k, _)| *k).collect::<Vec<_>>(), vec![1, 2, 3]);
        assert!(read.features[1..].iter().all(|f| f.geometry.is_none()));
        assert_eq!(table.read_editable(0).unwrap().features.len(), 1);
        let edits = Edits { deletes: read.versions, ..Default::default() };
        assert_eq!(table.save(&edits, &table.info().fields, std::iter::empty()).unwrap().unwrap().changes.deleted, 3);
        assert_eq!(conn.query_row("SELECT count(*) FROM roads", [], |r| r.get::<_, i64>(0)).unwrap(), 2);
        assert_eq!(conn.query_row("SELECT geom FROM roads WHERE fid = 4", [], |r| r.get::<_, Vec<u8>>(0)).unwrap(), b"GP\0\x03");
    }

    #[test]
    fn gpkg_zm_refuses_geometry_removal_but_allows_attributes() {
        let (_dir, path) = fixture();
        let conn = Connection::open(&path).unwrap();
        // Optional Z with an actual ISO XYZ line: the read geometry deliberately drops Z.
        conn.execute("UPDATE gpkg_geometry_columns SET z = 2", []).unwrap();
        let mut body = vec![1];
        body.extend_from_slice(&1002u32.to_le_bytes());
        body.extend_from_slice(&2u32.to_le_bytes());
        for v in [1.0_f64, 2.0, 9.0, 2.0, 3.0, 10.0] {
            body.extend_from_slice(&v.to_le_bytes());
        }
        let original = header_blob(&body, &line(1.0).extent(), 4326);
        conn.execute("UPDATE roads SET geom = ?1 WHERE fid = 1", [&original]).unwrap();
        let table = EditTable::open(&path, "roads").unwrap();
        for g in [None, Some(line(6.0))] {
            assert!(table.save(&update(&table, g), &table.info().fields, std::iter::empty()).unwrap_err().to_string().contains("Z or M"));
            assert_eq!(blob_at(&path, 1), original);
        }
        let mut edits = update(&table, Some(line(1.0)));
        edits.updates[0].0.attrs[0] = Value::Text("attributes only".into());
        assert_eq!(table.save(&edits, &table.info().fields, std::iter::empty()).unwrap().unwrap().changes.updated, 1);
        assert_eq!(blob_at(&path, 1), original);
        conn.execute("UPDATE gpkg_geometry_columns SET z = 1", []).unwrap();
        let table = EditTable::open(&path, "roads").unwrap();
        let edits = Edits { inserts: vec![Feature { id: 100, geometry: Some(line(4.0)), attrs: vec![] }], ..Default::default() };
        assert!(table.save(&edits, &table.info().fields, std::iter::empty()).unwrap_err().to_string().contains("needs Z or M"));
        let edits = Edits { inserts: vec![Feature { id: 100, geometry: None, attrs: vec![] }], ..Default::default() };
        assert_eq!(table.save(&edits, &table.info().fields, std::iter::empty()).unwrap().unwrap().changes.inserted, vec![(100, 2)]);
    }

    #[test]
    fn gpkg_untriggered_rtree_metadata_and_generated_keys() {
        let (_dir, path) = fixture();
        let conn = Connection::open(&path).unwrap();
        conn.execute_batch(
            "CREATE VIRTUAL TABLE rtree_roads_geom USING rtree(id, minx, maxx, miny, maxy);
            INSERT INTO rtree_roads_geom VALUES (1, 1, 2, 2, 3);",
        )
        .unwrap();
        let table = EditTable::open(&path, "roads").unwrap();
        table.save(&update(&table, Some(line(10.0))), &table.info().fields, std::iter::empty()).unwrap().unwrap();
        let envelope: (f64, f64, f64, f64) = conn
            .query_row("SELECT minx, maxx, miny, maxy FROM rtree_roads_geom WHERE id = 1", [], |r| {
                Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?))
            })
            .unwrap();
        assert_eq!(envelope, (10.0, 11.0, 2.0, 3.0));
        assert_eq!(conn.query_row("SELECT max_x FROM gpkg_contents", [], |r| r.get::<_, f64>(0)).unwrap(), 11.0);
        assert_ne!(conn.query_row("SELECT last_change FROM gpkg_contents", [], |r| r.get::<_, String>(0)).unwrap(), "before");
        table.save(&update(&table, None), &table.info().fields, std::iter::empty()).unwrap().unwrap();
        assert_eq!(conn.query_row("SELECT count(*) FROM rtree_roads_geom", [], |r| r.get::<_, i64>(0)).unwrap(), 0);
        let read = table.read_editable(MAX_EDITABLE).unwrap();
        let edits = Edits {
            deletes: read.versions,
            inserts: vec![Feature { id: 100, geometry: Some(line(20.0)), attrs: vec![] }],
            ..Default::default()
        };
        let saved = table.save(&edits, &table.info().fields, std::iter::empty()).unwrap().unwrap();
        assert_eq!(saved.changes.inserted, vec![(100, 2)]);
        assert_eq!(saved.changes.deleted, 1);
        assert_eq!(conn.query_row("SELECT id FROM rtree_roads_geom", [], |r| r.get::<_, i64>(0)).unwrap(), 2);
        assert_eq!(conn.query_row("SELECT name FROM roads", [], |r| r.get::<_, String>(0)).unwrap(), "default");
        assert_eq!(conn.query_row("SELECT v FROM other", [], |r| r.get::<_, String>(0)).unwrap(), "keep");
    }

    #[test]
    fn gpkg_innocuous_functions_work_from_untrusted_triggers() {
        let (_dir, path) = fixture();
        let conn = Connection::open(&path).unwrap();
        conn.execute_batch(
            "CREATE VIRTUAL TABLE rtree_roads_geom USING rtree(id, minx, maxx, miny, maxy);
             INSERT INTO rtree_roads_geom VALUES (1, 1, 2, 2, 3);
             CREATE TRIGGER spatial_update AFTER UPDATE OF geom ON roads
             BEGIN DELETE FROM rtree_roads_geom WHERE id = OLD.fid;
               INSERT INTO rtree_roads_geom SELECT NEW.fid, ST_MinX(NEW.geom), ST_MaxX(NEW.geom), ST_MinY(NEW.geom), ST_MaxY(NEW.geom)
               WHERE NEW.geom IS NOT NULL AND NOT ST_IsEmpty(NEW.geom);
             END;",
        )
        .unwrap();
        let table = EditTable::open(&path, "roads").unwrap();
        table.save(&update(&table, Some(line(8.0))), &table.info().fields, std::iter::empty()).unwrap().unwrap();
        assert_eq!(conn.query_row("SELECT maxx FROM rtree_roads_geom", [], |r| r.get::<_, f64>(0)).unwrap(), 9.0);
        table.save(&update(&table, None), &table.info().fields, std::iter::empty()).unwrap().unwrap();
        assert_eq!(conn.query_row("SELECT count(*) FROM rtree_roads_geom", [], |r| r.get::<_, i64>(0)).unwrap(), 0);
        register_functions(&conn).unwrap();
        for sql in ["SELECT ST_IsEmpty(NULL)", "SELECT ST_MinX('text')", "SELECT ST_MaxY(x'00')"] {
            assert_eq!(conn.query_row(sql, [], |r| r.get::<_, Option<f64>>(0)).unwrap(), None);
        }
        let raw = wkb::encode_single(&line(5.0)).unwrap();
        assert_eq!(conn.query_row("SELECT ST_MaxX(?1)", [raw], |r| r.get::<_, f64>(0)).unwrap(), 6.0);
    }

    #[test]
    fn gpkg_native_reprojection_writes_native_envelopes() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("mercator.gpkg");
        let crs = Crs::from_epsg(3857).unwrap();
        let mut ds = crate::Dataset::new("roads", crate::GeometryType::Polyline, vec![]);
        ds.push(Some(line(1_000_000.0)), vec![]);
        super::super::write_in(&ds, &path, crs).unwrap();
        let table = EditTable::open(&path, "roads").unwrap();
        assert_eq!(table.crs(), Some(crs));
        let mut edits = update(&table, None);
        let original = table.read_editable(MAX_EDITABLE).unwrap().features[0].geometry.clone().unwrap();
        let moved = original.map_coords(&|c| Coord::new(c.x + 1.0, c.y + 1.0));
        let expected = table.src.reproject.to_native(&moved).unwrap();
        edits.updates[0].0.geometry = Some(moved.clone());
        table.save(&edits, &[], std::iter::empty()).unwrap().unwrap();
        let stored = blob_at(&path, 1);
        let (envelope, body) = split_blob(&stored).unwrap();
        assert_eq!(envelope, Some(expected.extent()));
        assert_eq!(wkb::decode(body), Some(expected.clone()));
        let conn = Connection::open(&path).unwrap();
        let minx: f64 = conn.query_row("SELECT minx FROM rtree_roads_geom WHERE id = 1", [], |r| r.get(0)).unwrap();
        assert!((minx - expected.extent().min_x).abs() < 1.0);
        let reloaded = table.read_editable(MAX_EDITABLE).unwrap().features[0].geometry.clone().unwrap();
        let mut expected_coords = Vec::new();
        moved.for_each_coord(|c| expected_coords.push(c));
        let mut actual_coords = Vec::new();
        reloaded.for_each_coord(|c| actual_coords.push(c));
        assert!(expected_coords.iter().zip(actual_coords).all(|(a, b)| (a.x - b.x).abs() < 1e-6 && (a.y - b.y).abs() < 1e-6));
    }

    #[test]
    fn gpkg_fields_match_by_name_and_unstored_edits_are_unchanged() {
        let (_dir, path) = fixture();
        let table = EditTable::open(&path, "roads").unwrap();
        let read = table.read_editable(MAX_EDITABLE).unwrap();
        let fields = vec![
            Field::new("missing", crate::FieldType::Text),
            Field::new("NAME", crate::FieldType::Text),
            Field::new("fid", crate::FieldType::Integer),
        ];
        let mut f = read.features[0].clone();
        f.attrs = vec![Value::Text("not saved".into()), Value::Text("old".into()), Value::Int(999)];
        let mut edits = Edits { updates: vec![(f, read.versions[0].1)], ..Default::default() };
        let saved = table.save(&edits, &fields, std::iter::empty()).unwrap().unwrap();
        assert_eq!((saved.changes.unchanged, saved.changes.updated), (1, 0));
        assert_eq!(saved.changes.skipped_fields, vec!["missing", "fid"]);
        let mut expected = edits.updates[0].0.clone();
        expected.attrs[2] = Value::Int(1);
        assert_eq!(saved.features, vec![expected]);
        edits.updates[0].0.attrs[1] = Value::Text("new".into());
        assert_eq!(table.save(&edits, &fields, std::iter::empty()).unwrap().unwrap().changes.updated, 1);
        let conn = Connection::open(&path).unwrap();
        let stored: (i64, String, i64, Vec<u8>) =
            conn.query_row("SELECT fid, name, lanes, photo FROM roads", [], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?))).unwrap();
        assert_eq!(stored, (1, "new".into(), 2, vec![0, 255]));
        conn.execute("ALTER TABLE roads RENAME COLUMN geom TO shape", []).unwrap();
        assert!(table.save(&Edits::default(), &fields, std::iter::empty()).unwrap_err().to_string().contains("changed shape"));
    }

    #[test]
    fn gpkg_saved_snapshots_include_defaults_triggers_and_supplied_field_order() {
        let (_dir, path) = fixture();
        let table = EditTable::open(&path, "roads").unwrap();
        let conn = Connection::open(&path).unwrap();
        // This column appears after open: snapshots must use the transaction's column list.
        conn.execute_batch(
            "ALTER TABLE roads ADD COLUMN added TEXT DEFAULT 'added-default';
             CREATE TRIGGER stored_insert AFTER INSERT ON roads BEGIN
               UPDATE roads SET name = upper(NEW.name), lanes = NEW.lanes + 5,
                 geom = (SELECT geom FROM roads WHERE fid = 1) WHERE fid = NEW.fid;
             END;
             CREATE TRIGGER stored_update AFTER UPDATE OF name ON roads
             WHEN NEW.name != upper(NEW.name) BEGIN
               UPDATE roads SET name = upper(NEW.name), lanes = NEW.lanes + 1, geom = NULL WHERE fid = NEW.fid;
             END;",
        )
        .unwrap();
        let fields = vec![
            Field::new("lanes", crate::FieldType::Integer),
            Field::new("local", crate::FieldType::Text),
            Field::new("NAME", crate::FieldType::Text),
            Field::new("added", crate::FieldType::Text),
            Field::new("fid", crate::FieldType::Integer),
        ];
        let version = EditTable::open(&path, "roads").unwrap().read_editable(MAX_EDITABLE).unwrap().versions[0].1;
        let unchanged = Feature {
            id: 1,
            geometry: Some(line(1.0)),
            attrs: vec![
                Value::Int(2),
                Value::Text("kept".into()),
                Value::Text("old".into()),
                Value::Text("added-default".into()),
                Value::Int(999),
            ],
        };
        let inserted = Feature {
            id: 100,
            geometry: None,
            attrs: vec![Value::Null, Value::Text("insert-only".into()), Value::Null, Value::Null, Value::Int(999)],
        };
        let edits = Edits { updates: vec![(unchanged, version)], inserts: vec![inserted], ..Default::default() };
        let saved = table.save(&edits, &fields, [1].into_iter()).unwrap().unwrap();
        assert_eq!((saved.changes.unchanged, saved.changes.updated), (1, 0));
        assert_eq!(saved.changes.inserted, vec![(100, 2)]);
        assert_eq!(saved.features.len(), 2);
        assert_eq!(saved.features[0].attrs[1], Value::Text("kept".into()));
        assert_eq!(saved.features[0].attrs[4], Value::Int(1));
        assert_eq!(
            saved.features[1],
            Feature {
                id: 2,
                geometry: Some(line(1.0)),
                attrs: vec![
                    Value::Int(7),
                    Value::Text("insert-only".into()),
                    Value::Text("DEFAULT".into()),
                    Value::Text("added-default".into()),
                    Value::Int(2),
                ],
            }
        );
        let reopened = EditTable::open(&path, "roads").unwrap();
        assert_eq!(saved.changes.versions, reopened.read_editable(MAX_EDITABLE).unwrap().versions);

        let mut changed = saved.features[1].clone();
        changed.attrs[2] = Value::Text("edited".into());
        changed.geometry = Some(line(8.0));
        let edits = Edits { updates: vec![(changed, saved.changes.versions[1].1)], ..Default::default() };
        let saved = table.save(&edits, &fields, [1, 2].into_iter()).unwrap().unwrap();
        assert_eq!(saved.changes.updated, 1);
        assert_eq!(saved.features[0].geometry, None);
        assert_eq!(saved.features[0].attrs[0], Value::Int(8));
        assert_eq!(saved.features[0].attrs[1], Value::Text("insert-only".into()));
        assert_eq!(saved.features[0].attrs[2], Value::Text("EDITED".into()));
        let read = reopened.read_editable(MAX_EDITABLE).unwrap();
        assert_eq!(saved.changes.versions, vec![read.versions[1]]);
        // Reusing the returned snapshot and version must not reset trigger-produced values.
        let edits = Edits { updates: vec![(saved.features[0].clone(), saved.changes.versions[0].1)], ..Default::default() };
        let saved_again = table.save(&edits, &fields, [1, 2].into_iter()).unwrap().unwrap();
        assert_eq!((saved_again.changes.unchanged, saved_again.changes.updated), (1, 0));
        assert_eq!(saved_again.features, saved.features);
    }

    #[test]
    fn gpkg_unreadable_or_removed_stored_rows_roll_back() {
        for operation in ["INSERT", "UPDATE"] {
            for (action, message) in [
                ("UPDATE roads SET geom = x'00' WHERE fid = NEW.fid", "stored geometry can't be read"),
                ("DELETE FROM roads WHERE fid = NEW.fid", "removed while saving"),
            ] {
                let (_dir, path) = fixture();
                let conn = Connection::open(&path).unwrap();
                conn.execute_batch(&format!(
                    "CREATE TRIGGER break_stored AFTER {operation} ON roads WHEN NEW.name = 'broken'
                     BEGIN {action}; END;"
                ))
                .unwrap();
                let table = EditTable::open(&path, "roads").unwrap();
                let read = table.read_editable(MAX_EDITABLE).unwrap();
                let mut changed = read.features[0].clone();
                changed.attrs[0] = Value::Text(if operation == "UPDATE" { "broken" } else { "partial" }.into());
                let mut edits = Edits { updates: vec![(changed, read.versions[0].1)], ..Default::default() };
                if operation == "INSERT" {
                    edits.inserts.push(Feature {
                        id: 100,
                        geometry: Some(line(4.0)),
                        attrs: vec![Value::Text("broken".into()), Value::Null],
                    });
                }
                let error = table.save(&edits, &table.info().fields, [1].into_iter()).unwrap_err();
                assert!(error.to_string().contains(message), "{operation}: {error}");
                let after = table.read_editable(MAX_EDITABLE).unwrap();
                assert_eq!(after.features, read.features);
                assert_eq!(after.versions, read.versions);
                assert_eq!(after.matched, 1);
                assert_eq!(conn.query_row("SELECT last_change FROM gpkg_contents", [], |r| r.get::<_, String>(0)).unwrap(), "before");
            }
        }
    }

    #[test]
    fn gpkg_reserves_loaded_keys_after_external_highest_key_deletion() {
        let (_dir, path) = fixture();
        let conn = Connection::open(&path).unwrap();
        conn.execute_batch("INSERT INTO roads (fid) VALUES (-1), (2), (3)").unwrap();
        let table = EditTable::open(&path, "roads").unwrap();
        let read = table.read_editable(MAX_EDITABLE).unwrap();
        conn.execute("DELETE FROM roads WHERE fid = 3", []).unwrap();
        let edits = Edits {
            inserts: vec![Feature { id: 999, geometry: None, attrs: vec![Value::Text("new".into()), Value::Null] }],
            ..Default::default()
        };
        let saved = table.save(&edits, &table.info().fields, read.versions.iter().map(|(k, _)| *k)).unwrap().unwrap();
        assert_eq!(saved.changes.inserted, vec![(999, 4)]);
        assert_eq!(saved.features[0].id, 4);
        assert_eq!(conn.query_row("SELECT count(*) FROM roads WHERE fid IN (3, 999)", [], |r| r.get::<_, i64>(0)).unwrap(), 0);
        assert_eq!(saved.changes.versions, vec![table.read_editable(MAX_EDITABLE).unwrap().versions[3]]);
    }

    #[test]
    fn gpkg_generated_keys_respect_autoincrement_history() {
        let (_dir, path) = fixture();
        let conn = Connection::open(&path).unwrap();
        conn.execute_batch(
            "ALTER TABLE roads RENAME TO old_roads;
             CREATE TABLE roads (fid INTEGER PRIMARY KEY AUTOINCREMENT, geom LINESTRING, name TEXT DEFAULT 'default', lanes INTEGER DEFAULT 2, photo BLOB);
             INSERT INTO roads SELECT * FROM old_roads;
             DROP TABLE old_roads;
             INSERT INTO roads (fid) VALUES (100);
             DELETE FROM roads WHERE fid = 100;",
        )
        .unwrap();
        let table = EditTable::open(&path, "roads").unwrap();
        let edits = Edits { inserts: vec![Feature { id: 999, geometry: None, attrs: vec![] }], ..Default::default() };
        let saved = table.save(&edits, &table.info().fields, [70].into_iter()).unwrap().unwrap();
        assert_eq!(saved.changes.inserted, vec![(999, 101)]);
        assert_eq!(saved.features[0].id, 101);
        assert_eq!(conn.query_row("SELECT seq FROM sqlite_sequence WHERE name = 'roads'", [], |r| r.get::<_, i64>(0)).unwrap(), 101);
    }

    #[test]
    fn gpkg_key_exhaustion_rolls_back_updates_and_earlier_inserts() {
        for high_water_source in ["table", "reserved", "sequence"] {
            let (_dir, path) = fixture();
            let conn = Connection::open(&path).unwrap();
            if high_water_source == "sequence" {
                conn.execute_batch(
                    "ALTER TABLE roads RENAME TO old_roads;
                     CREATE TABLE roads (fid INTEGER PRIMARY KEY AUTOINCREMENT, geom LINESTRING, name TEXT DEFAULT 'default', lanes INTEGER DEFAULT 2, photo BLOB);
                     INSERT INTO roads SELECT * FROM old_roads;
                     DROP TABLE old_roads;",
                )
                .unwrap();
            }
            if high_water_source != "reserved" {
                conn.execute("INSERT INTO roads (fid) VALUES (?1)", [i64::MAX - 1]).unwrap();
                if high_water_source == "sequence" {
                    conn.execute("DELETE FROM roads WHERE fid = ?1", [i64::MAX - 1]).unwrap();
                }
            }
            let table = EditTable::open(&path, "roads").unwrap();
            let before = table.read_editable(MAX_EDITABLE).unwrap();
            let mut changed = before.features[0].clone();
            changed.attrs[0] = Value::Text("partial".into());
            let edits = Edits {
                updates: vec![(changed, before.versions[0].1)],
                inserts: vec![Feature { id: 100, geometry: None, attrs: vec![] }, Feature { id: 101, geometry: None, attrs: vec![] }],
                ..Default::default()
            };
            let reserved = if high_water_source == "reserved" { (i64::MAX - 1).cast_unsigned() } else { 1 };
            let error = table.save(&edits, &table.info().fields, [reserved].into_iter()).unwrap_err();
            assert!(error.to_string().contains("no positive INTEGER PRIMARY KEY"), "{high_water_source}: {error}");
            let after = table.read_editable(MAX_EDITABLE).unwrap();
            assert_eq!(after.features, before.features, "{high_water_source}");
            assert_eq!(after.versions, before.versions, "{high_water_source}");
            assert_eq!(after.matched, before.matched, "{high_water_source}");
            assert_eq!(conn.query_row("SELECT last_change FROM gpkg_contents", [], |r| r.get::<_, String>(0)).unwrap(), "before");
            if high_water_source == "sequence" {
                assert_eq!(
                    conn.query_row("SELECT seq FROM sqlite_sequence WHERE name = 'roads'", [], |r| r.get::<_, i64>(0)).unwrap(),
                    i64::MAX - 1
                );
            }
        }
    }

    #[test]
    fn gpkg_declared_geometry_mismatches_roll_back_updates_and_inserts() {
        for (ty, geometry) in [
            ("POINT", line(4.0)),
            ("LINESTRING", Geometry::Point(Coord::new(4.0, 5.0))),
            ("POLYGON", line(4.0)),
            ("MULTIPOINT", line(4.0)),
            ("MULTILINESTRING", Geometry::Point(Coord::new(4.0, 5.0))),
            ("MULTIPOLYGON", line(4.0)),
            ("GEOMETRYCOLLECTION", Geometry::Point(Coord::new(4.0, 5.0))),
        ] {
            let (_dir, path) = fixture();
            let conn = Connection::open(&path).unwrap();
            conn.execute("UPDATE gpkg_geometry_columns SET geometry_type_name = ?1", [ty]).unwrap();
            let table = EditTable::open(&path, "roads").unwrap();
            let original = blob_at(&path, 1);
            assert!(table.save(&update(&table, Some(geometry.clone())), &table.info().fields, std::iter::empty()).is_err(), "{ty}");
            assert_eq!(blob_at(&path, 1), original, "{ty}");
            let read = table.read_editable(MAX_EDITABLE).unwrap();
            let mut changed = read.features[0].clone();
            changed.attrs[0] = Value::Text("must roll back".into());
            let inserted = Feature { id: 99, geometry: Some(geometry), attrs: vec![Value::Text("new".into()), Value::Null] };
            let edits = Edits { updates: vec![(changed, read.versions[0].1)], inserts: vec![inserted], ..Default::default() };
            assert!(table.save(&edits, &table.info().fields, std::iter::empty()).is_err(), "{ty}");
            let stored: (i64, String) = conn.query_row("SELECT count(*), name FROM roads", [], |r| Ok((r.get(0)?, r.get(1)?))).unwrap();
            assert_eq!(stored, (1, "old".into()), "{ty}");
            assert_eq!(blob_at(&path, 1), original, "{ty}");
        }
    }

    #[test]
    fn gpkg_geometry_writes_keep_the_declared_single_or_multi_type() {
        let point = Coord::new(4.0, 5.0);
        let polygon = Geometry::Polygon(vec![vec![vec![point, Coord::new(5.0, 5.0), Coord::new(5.0, 6.0), point]]]);
        for (ty, geometry, wkb_type) in [
            ("POINT", Geometry::MultiPoint(vec![point]), 1_u32),
            ("LINESTRING", line(4.0), 2),
            ("POLYGON", polygon.clone(), 3),
            ("MULTIPOINT", Geometry::Point(point), 4),
            ("MULTILINESTRING", line(4.0), 5),
            ("MULTIPOLYGON", polygon, 6),
            ("GEOMETRY", Geometry::Point(point), 1),
        ] {
            let (_dir, path) = fixture();
            Connection::open(&path).unwrap().execute("UPDATE gpkg_geometry_columns SET geometry_type_name = ?1", [ty]).unwrap();
            let table = EditTable::open(&path, "roads").unwrap();
            table.save(&update(&table, Some(geometry)), &table.info().fields, std::iter::empty()).unwrap().unwrap();
            let bytes = blob_at(&path, 1);
            let (_, body) = split_blob(&bytes).unwrap();
            assert_eq!(body.get(1..5), Some(wkb_type.to_le_bytes().as_slice()), "{ty}");
        }
    }

    #[test]
    fn gpkg_rejects_non_integer_and_composite_primary_keys() {
        for schema in [
            "CREATE TABLE roads (fid INT PRIMARY KEY, geom BLOB, name TEXT)",
            "CREATE TABLE roads (fid INTEGER, geom BLOB, name TEXT, PRIMARY KEY (fid, name))",
            "CREATE TABLE roads (fid INTEGER, geom BLOB, name TEXT)",
        ] {
            let (_dir, path) = fixture();
            let conn = Connection::open(&path).unwrap();
            conn.execute_batch("DROP TABLE roads").unwrap();
            conn.execute_batch(schema).unwrap();
            let result = EditTable::open(&path, "roads");
            assert!(matches!(result, Err(DataError::Format(e)) if e.contains("no INTEGER PRIMARY KEY")));
        }
    }
}

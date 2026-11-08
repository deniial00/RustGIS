//! Undoable replacement of linked local data without replacing the layer.

use crate::{CmdResult, CommandSpec as C, Session};
use serde_json::Value;
#[cfg(not(target_arch = "wasm32"))]
use serde_json::json;

pub fn commands() -> Vec<C> {
    vec![
        C::new("layer.repairSource", "Repair Data Source", "Data › Data Source", repair_source).roles(&[]).effects(crate::policy::Effects::NONE.with(crate::policy::Effect::Files)).params(
            r#"{"layer"?: name|id (default: the active layer), "path": "the file or folder (a Zarr store, a File Geodatabase) the layer's data moved to", "table"?: "the dataset in a GeoPackage, GDAL or DuckDB file, or a raster's subdataset (default: the one the layer reads now)"}"#),
        C::new("map.repairTiles", "Repair Tile Archive", "Layers", repair_tiles).inputs(archive_inputs).roles(&[]).effects(crate::policy::Effects::NONE.with(crate::policy::Effect::Files)).params(
            r#"{"name"?: text | "index"?: number (0 = top) | "id"?: catalog id, "path": "the .pmtiles or .mbtiles file the archive moved to"}"#),
        C::new("map.reloadTiles", "Reload Data", "Layers", reload_tiles).inputs(archive_inputs).roles(&[]).effects(crate::policy::Effects::NONE.with(crate::policy::Effect::Files)).params(
            r#"{"name": text | "index": number (0 = top) | "id": catalog id} (give one selector) — rereads a local PMTiles or MBTiles archive at its linked path, keeping its presentation; undo cannot restore overwritten file contents"#),
    ]
}

/// Freeze the executor's selected archive without reading it or resolving a replacement path.
fn archive_inputs(input: &super::InputContext<'_>, p: &Value) -> Result<super::Inputs, String> {
    let i = super::tiles::find(input, p)?.ok_or("name, index or id is required")?;
    Ok(super::tiles::tile_inputs(input.map(), [i]))
}

#[cfg(not(target_arch = "wasm32"))]
fn replacement(p: &Value) -> Result<String, String> {
    let path = super::layer::s_req(p, "path")?.trim();
    if !rustgis_map::paths::is_local(path) {
        return Err(format!("{path}: give a file or folder on this computer (web addresses and databases aren't repaired here)"));
    }
    let path = rustgis_map::paths::local_path(path);
    let full = std::path::absolute(path).map_err(|e| format!("{path}: {e}"))?.display().to_string();
    if !std::path::Path::new(&full).exists() {
        return Err(format!("{full}: there's no such file or folder"));
    }
    Ok(full)
}

/// Reopens linked data after compatibility checks; undo restores the source and any reset label moves.
fn repair_source(s: &mut Session, p: &Value) -> CmdResult {
    #[cfg(target_arch = "wasm32")]
    {
        let _ = (s, p);
        Err("the web app can't link to files on this computer: add the file again with Add Data".into())
    }
    #[cfg(not(target_arch = "wasm32"))]
    {
        let id = s.layer_id(p)?;
        let full = replacement(p)?;
        let table = super::ps(p, "table").map(str::trim).filter(|t| !t.is_empty());
        let l = s.map().layer(id).ok_or("the layer no longer exists")?;
        if l.linked_file().is_none() {
            return Err(format!(
                "{}'s data is kept in the project or read from a database or web address: there's no file to repair",
                l.name
            ));
        }
        if let Some(rl) = &l.raster {
            let new = if rl.time_zarr.is_some() { super::zarr::relink_following(s, rl, &full)? } else { rl.relinked(&full, table)? };
            let l = s.map_mut().layer_mut(id).ok_or("the layer no longer exists")?;
            l.raster = Some(new);
            l.selection.clear();
            l.touch_geometry();
        } else {
            let src = l.source.as_ref().ok_or("the layer has no linked source")?;
            let new = src.relinked(&full, table)?;
            if let Some(info) = new.info() {
                if l.dataset.geometry_type != rustgis_data::GeometryType::None && info.geometry_type != l.dataset.geometry_type {
                    return Err(format!(
                        "{full} holds {} features; {} draws {} features: choose a file of {} features",
                        info.geometry_type.label(),
                        l.name,
                        l.dataset.geometry_type.label(),
                        l.dataset.geometry_type.label()
                    ));
                }
                let mut required: Vec<&str> = l.renderer.field().into_iter().collect();
                if l.labels.enabled {
                    required.push(&l.labels.default.field);
                    required.extend(l.labels.classes.iter().map(|c| c.field.as_str()));
                }
                if let Some(time) = &l.time {
                    required.push(&time.field);
                }
                required.retain(|f| l.dataset.fields.iter().any(|old| old.name.eq_ignore_ascii_case(f)));
                required.sort_unstable();
                required.dedup();
                let missing: Vec<&str> =
                    required.into_iter().filter(|f| !info.fields.iter().any(|new| new.name.eq_ignore_ascii_case(f))).collect();
                if !missing.is_empty() {
                    return Err(format!(
                        "{full} has no field {}, which {}'s symbology, labels or time slider use: choose a file that has them",
                        missing.join(", "),
                        l.name
                    ));
                }
                // Classes by an expression that reads on the old fields must read on the new ones.
                if let Some(e) = l.renderer.expression().filter(|e| rustgis_data::query::parse(e, &l.dataset).is_ok())
                    && let Err(why) =
                        rustgis_data::query::parse(e, &rustgis_data::Dataset::new("", info.geometry_type, info.fields.clone()))
                {
                    return Err(format!(
                        "{full} doesn't fit {}'s symbology, which classes by `{e}` ({why}): choose a file that has its fields",
                        l.name
                    ));
                }
            }
            let l = s.map_mut().layer_mut(id).ok_or("the layer no longer exists")?;
            let moved = l.replace_source(new);
            let name = l.name.clone();
            // A lock's feature ids and callout offsets belong to the old source too. The whole
            // lock goes, not just this layer's entries: while locked only the labels it names are
            // drawn, so dropping this layer's alone would hide its labels until the lock is
            // taken again. Cleared, every label is locked again where it is placed next.
            s.map_mut().label_lock = None;
            s.map_mut().label_lock_line_anchors.clear();
            if moved > 0 {
                s.notify(
                    format!("{moved} saved label move(s) reset in {name}: replacement data may reuse feature ids; undo restores them"),
                    false,
                );
            }
        }
        Ok(json!({"layer": id, "path": full, "table": s.map().layer(id).and_then(|l| l.linked_table())}))
    }
}

fn repair_tiles(s: &mut Session, p: &Value) -> CmdResult {
    #[cfg(target_arch = "wasm32")]
    {
        let _ = (s, p);
        Err("the web app can't link to files on this computer: add the file again with Add Data".into())
    }
    #[cfg(not(target_arch = "wasm32"))]
    {
        let prepared = super::reads::take::<Result<ReadArchive, String>>(s, "map.repairTiles");
        let i = super::tiles::find(s, p)?.ok_or("name, index or id is required")?;
        let t = s.map().tiles.get(i).ok_or("the tile layer no longer exists")?;
        if t.archive_file().is_none() {
            return Err(format!("{}: only a tile archive read from a file (PMTiles or MBTiles) can be repaired", t.name));
        }
        let full = replacement(p).map_err(|e| format!("{e}; correct the file and retry Repair Data Source"))?;
        let new = archive_candidate_after_read(t, &full, s.map().instance_id(), prepared)?;
        rustgis_render::rustgis_tiles::cache::reload_archive(&t.url);
        // The replacement path may already have been drawn by another layer.
        if t.url != full {
            rustgis_render::rustgis_tiles::cache::reload_archive(&full);
        }
        let slot = s.map_mut().tiles.get_mut(i).ok_or("the tile layer no longer exists")?;
        *slot = new;
        Ok(json!({"index": i, "path": full}))
    }
}

/// Repair and reload share one validation policy. Reading a candidate must not evict retained
/// tiles/metadata or reuse a connection to an archive replaced at the same path.
#[cfg(not(target_arch = "wasm32"))]
fn archive_candidate(t: &rustgis_map::TileLayer, path: &str) -> Result<rustgis_map::TileLayer, String> {
    use rustgis_render::rustgis_tiles::{mbtiles, pmtiles};
    let mut new = t.clone();
    new.url = path.to_string();
    new.error = None;
    new.repair()?;
    let vector = match new.kind {
        rustgis_map::TileKind::MbTiles => mbtiles::read_info_local(path)?.format.is_vector(),
        _ => pmtiles::read_info_local(path)?.tile_type.is_vector(),
    };
    if (!t.layers.is_empty() || !t.style_layers.is_empty()) && !vector {
        return Err(format!(
            "{path} holds raster tiles; {} draws vector tiles with its own layers or style: choose a vector tile archive",
            t.name
        ));
    }
    Ok(new)
}

/// An operation-local observation, not a file snapshot or a monitor baseline.
#[cfg(not(target_arch = "wasm32"))]
#[derive(PartialEq, Eq)]
struct FileRevision {
    len: u64,
    modified: web_time::SystemTime,
    #[cfg(unix)]
    identity_change: (u64, u64, i64, i64),
    #[cfg(windows)]
    created: u64,
}

#[cfg(not(target_arch = "wasm32"))]
impl FileRevision {
    fn read(path: &str, optional: bool) -> Result<Option<Self>, String> {
        let metadata = match std::fs::metadata(path) {
            Ok(metadata) => metadata,
            Err(e) if optional && e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(e) => {
                return Err(format!("{path}: could not observe archive metadata ({e}); finish writing or restore the file and retry"));
            }
        };
        // SQLite creates an empty -wal even for a read-only open of a clean WAL-mode archive,
        // and cannot remove it again. An empty sidecar holds no commits: same as no sidecar.
        if optional && metadata.len() == 0 {
            return Ok(None);
        }
        if !metadata.is_file() {
            return Err(format!("{path}: expected an archive file; restore the file and retry"));
        }
        let modified = metadata
            .modified()
            .map_err(|e| format!("{path}: could not observe the archive modification time ({e}); check file access and retry"))?;
        #[cfg(unix)]
        use std::os::unix::fs::MetadataExt;
        #[cfg(windows)]
        use std::os::windows::fs::MetadataExt;
        Ok(Some(Self {
            len: metadata.len(),
            modified,
            #[cfg(unix)]
            identity_change: (metadata.dev(), metadata.ino(), metadata.ctime(), metadata.ctime_nsec()),
            #[cfg(windows)]
            created: metadata.creation_time(),
        }))
    }
}

#[cfg(not(target_arch = "wasm32"))]
#[derive(PartialEq, Eq)]
struct ArchiveObservation {
    primary: Option<FileRevision>,
    // SQLite commits can change these without changing the main database. The -shm file
    // is reader coordination, not content, and is deliberately not observed.
    sqlite: Option<(Option<FileRevision>, Option<FileRevision>)>,
}

#[cfg(not(target_arch = "wasm32"))]
impl ArchiveObservation {
    fn read(path: &str) -> Result<Self, String> {
        let path = path.strip_prefix("file://").unwrap_or(path);
        let primary = FileRevision::read(path, false)?;
        let sqlite = std::path::Path::new(path).extension().is_some_and(|e| e.eq_ignore_ascii_case("mbtiles"));
        let sqlite = if sqlite {
            Some((FileRevision::read(&format!("{path}-wal"), true)?, FileRevision::read(&format!("{path}-journal"), true)?))
        } else {
            None
        };
        Ok(Self { primary, sqlite })
    }

    /// Metadata only: safe for final landing, with no archive scan or decompression.
    fn unchanged(&self, path: &str) -> Result<(), String> {
        if *self != Self::read(path)? {
            return Err(format!("{path}: the archive changed while it was read; finish writing and retry the command"));
        }
        Ok(())
    }
}

#[cfg(not(target_arch = "wasm32"))]
struct ValidatedArchive {
    layer: rustgis_map::TileLayer,
    observation: ArchiveObservation,
}

#[cfg(not(target_arch = "wasm32"))]
fn observed_candidate(t: &rustgis_map::TileLayer, path: &str) -> Result<ValidatedArchive, String> {
    // Keep the archive reader's path boundary ahead of the new metadata I/O too.
    rustgis_map::tiles::check_path(path, if t.kind == rustgis_map::TileKind::MbTiles { "mbtiles" } else { "pmtiles" })?;
    let observation = ArchiveObservation::read(path)?;
    let layer = archive_candidate(t, path)?;
    observation.unchanged(path)?;
    Ok(ValidatedArchive { layer, observation })
}

#[cfg(not(target_arch = "wasm32"))]
struct ReadArchive {
    map: u64,
    original: rustgis_map::TileLayer,
    path: String,
    result: Result<ValidatedArchive, String>,
}

/// Fresh validation for the desktop's single read worker; nothing is installed or invalidated.
#[cfg(not(target_arch = "wasm32"))]
pub(crate) fn archive_ahead(s: &Session, command: &str, p: &Value) -> super::reads::ReadWork {
    let request = (|| {
        let i = super::tiles::find(s, p)?.ok_or("name, index or id is required")?;
        let t = s.map().tiles.get(i).ok_or("the tile layer no longer exists")?;
        let linked = t.archive_file().ok_or("only a linked local PMTiles or MBTiles archive can be reloaded or repaired")?;
        let path = if command == "map.repairTiles" { replacement(p)? } else { linked.to_string() };
        Ok::<_, String>((s.map().instance_id(), t.clone(), path))
    })();
    let name = if command == "map.repairTiles" { "Repair Data Source" } else { "Reload Data" };
    super::reads::ahead(name, command.to_string(), move || {
        request.map(|(map, original, path)| {
            let result = observed_candidate(&original, &path);
            ReadArchive { map, original, path, result }
        })
    })
}

#[cfg(not(target_arch = "wasm32"))]
fn archive_candidate_after_read(
    t: &rustgis_map::TileLayer,
    path: &str,
    map: u64,
    prepared: Option<Result<ReadArchive, String>>,
) -> Result<rustgis_map::TileLayer, String> {
    let candidate = if let Some(prepared) = prepared {
        let prepared = prepared?;
        if prepared.map != map || prepared.original != *t || prepared.path != path {
            return Err("the archive or its settings changed while it was read; retry the command".into());
        }
        prepared.result?
    } else {
        observed_candidate(t, path)?
    };
    candidate.observation.unchanged(path)?;
    Ok(candidate.layer)
}

fn reload_tiles(s: &mut Session, p: &Value) -> CmdResult {
    #[cfg(target_arch = "wasm32")]
    {
        let _ = (s, p);
        Err("the web app can't reload files on this computer: add the file again with Add Data".into())
    }
    #[cfg(not(target_arch = "wasm32"))]
    {
        let prepared = super::reads::take::<Result<ReadArchive, String>>(s, "map.reloadTiles");
        let i = super::tiles::find(s, p)?.ok_or("name, index or id is required")?;
        let t = s.map().tiles.get(i).ok_or("the tile layer no longer exists")?;
        let path = t.archive_file().ok_or_else(|| {
            format!(
                "{}: Reload Data needs a linked local PMTiles or MBTiles file; add snapshots/uploads again, or use Reload Web Layers for web addresses",
                t.name
            )
        })?.to_string();
        let new = archive_candidate_after_read(t, &path, s.map().instance_id(), prepared).map_err(|e| {
            format!("{} ({path}): {e}; correct the file and retry Reload Data, or use Repair Data Source if it moved", t.name)
        })?;
        let cancel_view = s.style_view.as_ref().is_some_and(|(pending, _, map)| {
            *map == s.map().instance_id()
                && pending.kind == t.kind
                && pending.url == t.url
                && pending.bounds == t.bounds
                && pending.id == t.id
        });
        rustgis_render::rustgis_tiles::cache::reload_archive(&path);
        let slot = s.map_mut().tiles.get_mut(i).ok_or("the tile layer no longer exists")?;
        *slot = new;
        let message = format!("Reloaded {} from {path}; undo cannot restore overwritten file contents", slot.name);
        if cancel_view {
            s.style_view = None;
        }
        s.notify(message, false);
        Ok(json!({"index": i, "path": path}))
    }
}

#[cfg(all(test, not(target_arch = "wasm32")))]
mod tests {
    use super::*;

    #[test]
    fn empty_sqlite_sidecars_match_absent_ones_but_commits_do_not() {
        let dir = std::env::temp_dir().join(format!("rustgis-repair-observation-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("clean.mbtiles");
        std::fs::write(&path, b"main database").unwrap();
        let path = path.display().to_string();
        let observed = ArchiveObservation::read(&path).unwrap();
        for sidecar in ["-wal", "-journal"] {
            let sidecar = format!("{path}{sidecar}");
            std::fs::write(&sidecar, b"").unwrap();
            observed.unchanged(&path).expect("an empty sidecar created by a reader is not a commit");
            std::fs::write(&sidecar, b"committed frame").unwrap();
            assert!(observed.unchanged(&path).is_err(), "{sidecar}");
            std::fs::remove_file(&sidecar).unwrap();
            observed.unchanged(&path).unwrap();
        }
        std::fs::remove_dir_all(dir).unwrap();
    }
}

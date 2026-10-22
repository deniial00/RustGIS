//! Saved connections and session-only logins. Listing handlers are called directly: these
//! commands never edit the project, create undo snapshots or note recently added sources.

use std::collections::BTreeMap;

use serde_json::{Value, json};

use super::ps;
use crate::connections::{Connection, DATABASE_KINDS, DESKTOP_ONLY, KINDS};
use crate::render::rustgis_tiles::s3;
use crate::{CmdResult, CommandSpec as C, Session};

pub fn commands() -> Vec<C> {
    vec![
        C::new("connection.list", "List Connections", "Home › Add › Data Sources", list).view().params(
            r#"{"kind"?: "postgres"|"mysql"|"sqlserver"|"service"|"wms"|"wmts"|"wfs"|"wcs"|"ogcapi"|"arcgis"|"csw"|"stac"|"s3", "group"?: text}"#,
        ),
        C::new("connection.new", "New Connection", "Home › Add › Data Sources", new).view().params(
            r#"{"kind": "postgres"|"mysql"|"sqlserver"|"service"|"wms"|"wmts"|"wfs"|"wcs"|"ogcapi"|"arcgis"|"csw"|"stac"|"s3", "address"?: "as the kind's add command takes it, without its password or token (postgres:host=… dbname=… user=…, mysql://user@host/db, mssql://user@host/db, a service's or STAC API's https:// address, an S3 endpoint: AWS when absent)", "name"?: "unique (default from the address)", "group"?: text, "options"?: {"region"?: text, "buckets"?: "a,b", "credentials"?: "keys"|"profile"|"environment"|"none", "profile"?: text} (S3 only)}"#,
        ),
        C::new("connection.edit", "Edit Connection", "Home › Add › Data Sources", edit).view().params(
            r#"{"id": "a connection's id or name", "name"?: text, "group"?: text, "address"?: text, "options"?: "as connection.new (replaces them)", "kind"?: "its kind (unchanged)"}"#,
        ),
        C::new("connection.duplicate", "Duplicate Connection", "Home › Add › Data Sources", duplicate).view().params(r#"{"id": "a connection's id or name", "name"?: text}"#),
        C::new("connection.remove", "Remove Connection", "Home › Add › Data Sources", remove).view().params(r#"{"id": "a connection's id or name"}"#),
        C::new("connection.login", "Log In to Connection", "Home › Add › Data Sources", login_command).view().params(
            r#"{"id": "a connection's id or name", "password"?: "a database connection's, kept for this session only", "accessKeyId"?: text, "secretAccessKey"?: text, "sessionToken"?: "an S3 connection's keys (credentials: keys), kept for this session only"}"#,
        ),
        C::new("connection.test", "Test Connection", "Home › Add › Data Sources", test).view().params(
            r#"{"id": "a connection's id or name", "password"?: text, "accessKeyId"?: text, "secretAccessKey"?: text, "sessionToken"?: "as connection.login (logs in first)"}"#,
        ),
    ]
}

fn shown(c: &Connection) -> Value {
    let database = DATABASE_KINDS.contains(&c.kind.as_str());
    let available = !database || !cfg!(target_arch = "wasm32");
    let logged_in = if c.kind == "s3" {
        s3::connection(&c.name).is_some()
    } else if database {
        #[cfg(not(target_arch = "wasm32"))]
        {
            use rustgis_data::source::duckdb::{self, Database};
            match Database::parse(&c.address) {
                Database::Postgres(d) | Database::Mysql(d) | Database::Sqlserver(d) => duckdb::has_password(&d),
                Database::File(_) => false,
            }
        }
        #[cfg(target_arch = "wasm32")]
        {
            false
        }
    } else {
        false
    };
    let add_with = match c.kind.as_str() {
        "postgres" | "mysql" | "sqlserver" => "info.databaseTables {database: address}, then layer.addDatabase {database: address, layer}",
        "service" => "service.layers {url: address}, then service.add {url, layer}",
        "wms" | "wmts" | "wfs" | "ogcapi" | "arcgis" => "service.layers {url: address, type: kind}, then service.add {url, type, layer}",
        "wcs" => "service.layers {url: address, type: wcs}, then service.coverage {url, layer}",
        "csw" => "catalog.search {url: address, query}",
        "stac" => "stac.collections {url: address}, then stac.search {url, collections}",
        "s3" => "connection.login {id} once, then s3.list {connection: name}, then layer.addUrl {url: s3://…}",
        _ => "",
    };
    let mut out = serde_json::to_value(c).unwrap_or(Value::Null);
    out["available"] = json!(available);
    if !available {
        out["why"] = json!(DESKTOP_ONLY);
    }
    out["loggedIn"] = json!(logged_in);
    out["addWith"] = json!(add_with);
    out
}

fn found<'a>(s: &'a Session, p: &Value) -> Result<&'a Connection, String> {
    let id = ps(p, "id").unwrap_or("");
    s.connections.find(id).ok_or_else(|| format!("no connection '{id}'"))
}

fn options(p: &Value) -> Result<BTreeMap<String, String>, String> {
    let object = p.as_object().ok_or("options is an object of texts")?;
    object
        .iter()
        .map(|(k, v)| v.as_str().map(|t| (k.clone(), t.to_owned())).ok_or_else(|| "options is an object of texts".into()))
        .collect()
}

fn list(s: &mut Session, p: &Value) -> CmdResult {
    let kind = ps(p, "kind");
    if let Some(k) = kind
        && !KINDS.contains(&k)
    {
        return Err(format!("unknown kind '{k}' ({})", KINDS.join(", ")));
    }
    let group = ps(p, "group");
    Ok(json!({"connections": s.connections.connections.iter()
        .filter(|c| kind.is_none_or(|k| c.kind == k) && group.is_none_or(|g| c.group.chars().flat_map(char::to_lowercase).eq(g.chars().flat_map(char::to_lowercase))))
        .map(shown).collect::<Vec<_>>() }))
}

fn new(s: &mut Session, p: &Value) -> CmdResult {
    let c = Connection {
        kind: ps(p, "kind").ok_or_else(|| format!("kind is required ({})", KINDS.join(", ")))?.trim().into(),
        address: ps(p, "address").unwrap_or("").into(),
        name: ps(p, "name").unwrap_or("").into(),
        group: ps(p, "group").unwrap_or("").into(),
        options: p.get("options").map(options).transpose()?.unwrap_or_default(),
        ..Connection::default()
    };
    let explicit = !c.name.trim().is_empty();
    let (c, stripped) = s.connections.add(c, explicit)?;
    Ok(shown_stripped(&c, stripped))
}

fn shown_stripped(c: &Connection, stripped: bool) -> Value {
    let mut out = shown(c);
    if stripped {
        out["stripped"] = json!(true);
    }
    out
}

fn edit(s: &mut Session, p: &Value) -> CmdResult {
    let old = found(s, p)?.clone();
    let mut c = old.clone();
    if let Some(kind) = ps(p, "kind")
        && kind.trim() != c.kind
    {
        return Err("a connection's kind doesn't change: make a new one".into());
    }
    for (key, field) in [("name", &mut c.name), ("group", &mut c.group), ("address", &mut c.address)] {
        if let Some(value) = p.get(key) {
            *field = value.as_str().ok_or_else(|| format!("{key} is text"))?.into();
        }
    }
    if let Some(v) = p.get("options") {
        c.options = options(v)?;
    }
    let (c, stripped) = s.connections.update(&old.id, c)?;
    if c.kind == "s3" && (old.name != c.name || old.address != c.address || old.options != c.options) {
        s3::disconnect(&old.name);
    }
    Ok(shown_stripped(&c, stripped))
}

fn duplicate(s: &mut Session, p: &Value) -> CmdResult {
    let mut c = found(s, p)?.clone();
    c.id.clear();
    let explicit = ps(p, "name").is_some_and(|n| !n.trim().is_empty());
    if let Some(name) = ps(p, "name") {
        c.name = name.into();
    }
    let (c, _) = s.connections.add(c, explicit)?;
    Ok(shown(&c))
}

fn remove(s: &mut Session, p: &Value) -> CmdResult {
    let c = found(s, p)?.clone();
    s.connections.remove(&c.id);
    if c.kind == "s3" {
        s3::disconnect(&c.name);
    }
    Ok(json!({"removed": c.id}))
}

fn login(s: &mut Session, c: &Connection, p: &Value) -> Result<(), String> {
    if DATABASE_KINDS.contains(&c.kind.as_str()) {
        #[cfg(target_arch = "wasm32")]
        {
            return Err(DESKTOP_ONLY.into());
        }
        #[cfg(not(target_arch = "wasm32"))]
        {
            let pw = ps(p, "password").filter(|t| !t.is_empty()).ok_or("password is required")?;
            super::layer::database_param(&json!({"database": c.address, "password": pw}));
            return Ok(());
        }
    }
    if c.kind != "s3" {
        return Err(format!("{} connections don't log in yet", c.kind));
    }
    let mut q = json!({"name": c.name, "endpoint": c.address});
    for key in ["region", "buckets"] {
        if let Some(v) = c.options.get(key).filter(|v| !v.is_empty()) {
            q[key] = json!(v);
        }
    }
    match c.options.get("credentials").map(String::as_str).unwrap_or("none") {
        "keys" => {
            if ["accessKeyId", "secretAccessKey"].iter().any(|k| ps(p, k).is_none_or(|v| v.trim().is_empty())) {
                return Err("accessKeyId and secretAccessKey are required: an S3 connection's keys are typed each session".into());
            }
            for key in ["accessKeyId", "secretAccessKey", "sessionToken"] {
                if let Some(v) = p.get(key) {
                    q[key] = v.clone();
                }
            }
        }
        "profile" => {
            let profile = c.options.get("profile").filter(|v| !v.trim().is_empty()).ok_or("the connection names no AWS profile")?;
            q["profile"] = json!(profile);
        }
        "environment" => q["environment"] = json!(true),
        _ => q["anonymous"] = json!(true),
    }
    super::s3::connect(s, &q)?;
    Ok(())
}

fn login_command(s: &mut Session, p: &Value) -> CmdResult {
    let c = found(s, p)?.clone();
    login(s, &c, p)?;
    Ok(shown(&c))
}

fn test(s: &mut Session, p: &Value) -> CmdResult {
    let c = found(s, p)?.clone();
    if ["password", "accessKeyId", "secretAccessKey", "sessionToken"].iter().any(|k| ps(p, k).is_some_and(|v| !v.is_empty())) {
        login(s, &c, p)?;
    }
    let result = match c.kind.as_str() {
        "postgres" | "mysql" | "sqlserver" => {
            if cfg!(target_arch = "wasm32") {
                return Err(DESKTOP_ONLY.into());
            }
            super::layer::database_tables(s, &json!({"database": c.address}))?
        }
        "service" => super::service::layers(s, &json!({"url": c.address}))?,
        "wms" | "wmts" | "wfs" | "wcs" | "ogcapi" | "arcgis" => super::service::layers(s, &json!({"url": c.address, "type": c.kind}))?,
        "csw" => super::service::catalog_search(s, &json!({"url": c.address}))?,
        "stac" => super::stac::collections(s, &json!({"url": c.address}))?,
        "s3" => {
            if s3::connection(&c.name).is_none() {
                if c.options.get("credentials").is_some_and(|v| v == "keys") {
                    return Err("log in first: connection.login {id, accessKeyId, secretAccessKey}".into());
                }
                login(s, &c, &Value::Null)?;
            }
            super::s3::list(s, &json!({"connection": c.name, "limit": 100}))?
        }
        _ => return Err(format!("unknown kind '{}'", c.kind)),
    };
    Ok(json!({"connection": c.id, "result": result}))
}

//! User-initiated database reads use the requested saved ID, never a same-address row's login.

use serde_json::Value;
#[cfg(not(target_arch = "wasm32"))]
use serde_json::json;

use crate::Session;
use crate::connections::Connection;

/// Geometry-table details shared by saved-ID Browse and Test. Call only after the probe return:
/// a probe must neither read the secret store nor connect to a database.
pub(super) fn tables(s: &mut Session, c: &Connection) -> Result<Vec<Value>, String> {
    #[cfg(target_arch = "wasm32")]
    {
        let _ = (s, c);
        Err(crate::connections::DESKTOP_ONLY.into())
    }
    #[cfg(not(target_arch = "wasm32"))]
    {
        use rustgis_data::DataError;
        use rustgis_data::source::duckdb::{self, Database};

        let database = Database::parse(&c.address);
        let (kind, key) = match &database {
            Database::Postgres(key) => ("postgres", key),
            Database::Mysql(key) => ("mysql", key),
            Database::Sqlserver(key) => ("sqlserver", key),
            Database::File(_) => return Err("not a database server".into()),
        };
        // Clear even when loading fails: another saved ID's active password is not authority
        // to read this connection. Passwords explicitly typed for the address remain intact.
        duckdb::set_connection_password(key, &c.id, None);
        s.database_logins.drop_activation(key);
        let credential = if !s.database_logins.has_session_password(&c.id) && duckdb::has_explicit_password(key) {
            None
        } else {
            super::credential(s, c)?
        };
        if credential.is_none() && s.database_logins.is_rejected(&c.id) && !duckdb::has_explicit_password(key) {
            super::request_password(s, c);
            return Err("password is required".into());
        }
        let saved = credential.as_ref().is_some_and(|(_, saved)| *saved);
        if let Some((password, _)) = &credential {
            super::activate(s, c, password.expose());
            s.database_logins.select(kind, key, &c.id);
        }

        match read_tables(&database) {
            Ok(tables) => {
                s.database_logins.clear_requests_for(kind, key);
                Ok(tables)
            }
            Err(error) => {
                let error = match &credential {
                    Some((password, _)) => error.redact_secret(password.expose()),
                    None => error,
                };
                if matches!(&error, DataError::Login(_)) {
                    if saved {
                        s.database_logins.reject(&c.id);
                        duckdb::forget_connection_password(&c.id);
                        s.database_logins.drop_activation(key);
                    }
                    super::request_password(s, c);
                }
                Err(format!("{}: {error}", database.short()))
            }
        }
    }
}

#[cfg(not(target_arch = "wasm32"))]
fn read_tables(database: &rustgis_data::source::duckdb::Database) -> rustgis_data::Result<Vec<Value>> {
    use rustgis_data::source::{duckdb::Database, mysql, postgis, sqlserver};

    // Keep DataError intact through both connect and table listing so only an authentication
    // refusal suppresses a saved credential. A transport or query error must not do that.
    match database {
        Database::Postgres(address) => {
            let connection = postgis::connect(address, true)?;
            Ok(postgis::tables(&connection)?
                .into_iter()
                .map(|t| json!({"label": t.label(), "schema": t.schema, "name": t.name, "column": t.column, "kind": t.kind}))
                .collect())
        }
        Database::Mysql(address) => {
            let connection = mysql::connect(address, true)?;
            Ok(mysql::tables(&connection)?
                .into_iter()
                .map(|t| json!({"label": t.label(), "schema": t.schema, "name": t.name, "column": t.column, "kind": t.kind}))
                .collect())
        }
        Database::Sqlserver(address) => {
            let connection = sqlserver::connect(address, true)?;
            Ok(sqlserver::tables(&connection)?
                .into_iter()
                .map(|t| json!({"label": t.label(), "schema": t.schema, "name": t.name, "column": t.column, "kind": t.kind}))
                .collect())
        }
        Database::File(_) => Err(rustgis_data::DataError::Format("not a database server".into())),
    }
}

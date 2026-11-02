//! Service logins belong to a session/connection, never an origin-global request context.
use super::*;
use crate::{render::rustgis_tiles::logins, service_logins as web};
use rustgis_data::secrets::{MemorySecrets, SecretError, SecretStore};
use std::sync::Arc;

fn session(store: Option<Arc<dyn SecretStore>>) -> Session {
    let mut s = Session::new(Project::default());
    s.database_logins.set_store(store);
    s
}

fn new(s: &mut Session, p: Value) -> String {
    s.run("connection.new", &p).unwrap()["id"].as_str().unwrap().to_string()
}

fn bound(s: &mut Session, id: &str, url: &str) -> String {
    web::request(s, Some(id), url).unwrap()
}

fn header(address: &str) -> Option<(String, String)> {
    logins::apply(address).unwrap().and_then(|a| a.header)
}

fn project(origin: &str, id: &str) -> String {
    let mut p = Project::default();
    let mut t = rustgis_map::TileLayer::new("Secured", rustgis_map::TileKind::Raster, format!("{origin}/tiles/{{z}}/{{x}}/{{y}}.png"));
    t.connection = id.into();
    p.map_mut().tiles.push(t);
    serde_json::to_string(&p).unwrap()
}

fn requests(s: &mut Session) -> Value {
    s.run("connection.list", &Value::Null).unwrap()["loginRequests"].clone()
}

#[test]
fn web_login_goes_only_to_its_connections_origin() {
    let mut s = session(None);
    let id = new(&mut s, json!({"kind":"wms","address":"https://scope757.example.org/wms","auth":"header","keyName":"X-Api-Key"}));
    s.run("connection.login", &json!({"id":id,"apiKey":"scope-sentinel-757"})).unwrap();
    // A raw URL is never authority to use a service login, even at its origin.
    assert!(header("https://scope757.example.org/wms").is_none());
    let address = bound(&mut s, &id, "https://SCOPE757.example.org:443/tiles/1/2/3.png");
    assert_eq!(header(&address).unwrap(), ("X-Api-Key".into(), "scope-sentinel-757".into()));
    for other in [
        "https://other757.example.org/wms",
        "https://scope757.example.org.evil.example/wms",
        "https://scope757.example.org:8443/wms",
        "http://scope757.example.org/wms",
    ] {
        assert!(header(&bound(&mut s, &id, other)).is_none(), "{other}");
        assert!(header(&logins::inherit(&address, other)).is_none(), "{other}");
    }
    s.run("map.addTiles", &json!({"url":"https://scope757.example.org/tiles/{z}/{x}/{y}.png","name":"Named","connection":id})).unwrap();
    // Replacement has the same (URL, name), but the newly created layer still needs provenance.
    s.run("map.addTiles", &json!({"url":"https://scope757.example.org/tiles/{z}/{x}/{y}.png","name":"Named","connection":id})).unwrap();
    let tile = s.map().tiles.iter().find(|t| t.name == "Named").unwrap();
    assert_eq!(tile.connection, id);
    assert_eq!(header(&tile.url).unwrap().1, "scope-sentinel-757");
    let previous = tile.url.clone();
    s.run("connection.logout", &json!({"id":id})).unwrap();
    assert!(logins::apply(&previous).is_err());
    assert_eq!(web::request(&mut s, Some(&id), "https://scope757.example.org/wms").unwrap_err(), web::REQUIRED);
}

#[test]
fn web_login_methods_and_encoded_basic_size_are_checked() {
    let mut s = session(None);
    for bad in [
        json!({"kind":"wms","address":"https://m757.example.org","auth":"digest"}),
        json!({"kind":"wms","address":"https://m757.example.org","auth":"query","keyName":"key"}),
        json!({"kind":"wms","address":"https://m757.example.org","auth":"basic"}),
        json!({"kind":"wms","address":"https://m757.example.org","auth":"basic","user":"a:b"}),
        json!({"kind":"wms","address":"https://m757.example.org","auth":"header","keyName":"Cookie"}),
        json!({"kind":"wms","address":"https://m757.example.org","auth":"header","keyName":"X Key"}),
        json!({"kind":"s3","auth":"header","keyName":"X-Key"}),
    ] {
        assert!(s.run("connection.new", &bad).is_err(), "{bad}");
    }
    let id = new(&mut s, json!({"kind":"arcgis","address":"https://m757.example.org/rest/services","auth":"basic","user":"ana"}));
    s.run("connection.login", &json!({"id":id,"password":"pw-757"})).unwrap();
    let address = bound(&mut s, &id, "https://m757.example.org/rest/services");
    assert_eq!(header(&address).unwrap(), ("Authorization".into(), logins::basic("ana", "pw-757")));
    for bad in [
        json!({"id":id,"apiKey":"do-not-echo-757"}),
        json!({"id":id,"password":"bad\nline"}),
        json!({"id":id,"password":"x".repeat(3070)}),
        json!({"id":id,"password":"pw-757","allowInsecure":"yes"}),
    ] {
        let error = s.run("connection.login", &bad).unwrap_err();
        assert!(!error.contains("do-not-echo-757") && !error.contains("bad\nline") && !error.contains("pw-757"));
        assert_eq!(header(&address).unwrap().1, logins::basic("ana", "pw-757"));
    }
}

#[test]
fn plain_http_confirmation_applies_only_to_this_sessions_login_identity() {
    let mut s = session(None);
    let id = new(&mut s, json!({"kind":"wfs","address":"http://http757.example.org/wfs","auth":"basic","user":"ana"}));
    assert_eq!(s.run("connection.login", &json!({"id":id,"password":"http-sentinel-757"})).unwrap_err(), web::INSECURE);
    assert!(!s.database_logins.has_session_password(&id));
    s.run("connection.login", &json!({"id":id,"password":"http-sentinel-757","allowInsecure":true})).unwrap();
    assert!(header(&bound(&mut s, &id, "http://http757.example.org/features")).is_some());
    assert!(header(&bound(&mut s, &id, "https://http757.example.org/features")).is_none());
    // Reuse within this session needs no new confirmation; a later session has no credential.
    s.run("connection.login", &json!({"id":id})).unwrap();
    let prefs = s.connections.to_json();
    let mut later = session(None);
    later.connections = crate::connections::Connections::from_json(&prefs);
    assert_eq!(later.run("connection.login", &json!({"id":id})).unwrap_err(), web::REQUIRED);
    s.run("connection.logout", &json!({"id":id})).unwrap();
    assert_eq!(s.run("connection.login", &json!({"id":id,"password":"http-sentinel-757"})).unwrap_err(), web::INSECURE);
    s.run("connection.login", &json!({"id":id,"password":"http-sentinel-757","allowInsecure":true})).unwrap();
    s.run("connection.edit", &json!({"id":id,"address":"http://http757.example.org:8080/wfs"})).unwrap();
    assert_eq!(s.run("connection.login", &json!({"id":id})).unwrap_err(), web::REQUIRED);
}

#[test]
fn service_login_edit_and_remove_do_not_depend_on_a_credential_store() {
    let store = Arc::new(MemorySecrets::failing(SecretError::Denied));
    let mut s = session(Some(store.clone()));
    let id = new(&mut s, json!({"kind":"ogcapi","address":"https://memory757.example.org/ogc","auth":"header","keyName":"X-Key"}));
    assert_eq!(s.run("connection.login", &json!({"id":id,"apiKey":"memory-sentinel-757","remember":true})).unwrap_err(), web::SESSION_ONLY);
    assert!(!s.database_logins.has_session_password(&id));
    s.run("connection.login", &json!({"id":id,"apiKey":"memory-sentinel-757"})).unwrap();
    let address = bound(&mut s, &id, "https://memory757.example.org/features");
    assert_eq!(header(&address).unwrap().1, "memory-sentinel-757");
    let duplicate = s.run("connection.duplicate", &json!({"id":id,"name":"Another account"})).unwrap();
    assert_eq!(duplicate["loggedIn"], false);
    assert_eq!(s.run("connection.test", &json!({"id":duplicate["id"],"probe":true})).unwrap_err(), web::REQUIRED);
    s.run("connection.edit", &json!({"id":id,"name":"Renamed","address":"https://memory757.example.org/v2"})).unwrap();
    assert_eq!(header(&address).unwrap().1, "memory-sentinel-757");
    // A stale remembered marker cannot opt a service into credential-store access.
    let mut prefs: Value = serde_json::from_str(&s.connections.to_json()).unwrap();
    prefs["connections"][0]["remembered"] = json!(true);
    let mut later = session(Some(store));
    later.connections = crate::connections::Connections::from_json(&prefs.to_string());
    assert!(!later.connections.find(&id).unwrap().remembered);
    assert_eq!(later.run("connection.login", &json!({"id":id})).unwrap_err(), web::REQUIRED);
    s.run("connection.edit", &json!({"id":id,"address":"https://changed-memory757.example.org/ogc"})).unwrap();
    assert!(logins::apply(&address).is_err());
    s.run("connection.login", &json!({"id":id,"apiKey":"replacement-sentinel-757"})).unwrap();
    let replacement = bound(&mut s, &id, "https://changed-memory757.example.org/features");
    s.run("connection.remove", &json!({"id":id})).unwrap();
    assert!(logins::apply(&replacement).is_err());
}

#[test]
fn simultaneous_sessions_and_connections_never_share_or_replace_logins() {
    let origin = "https://sessions757.example.org";
    let definition = json!({"kind":"wms","address":format!("{origin}/wms"),"auth":"header","keyName":"X-Key"});
    let mut a = session(None);
    let mut b = session(None);
    let id = new(&mut a, definition.clone());
    assert_eq!(new(&mut b, definition.clone()), id);
    a.run("connection.login", &json!({"id":id,"apiKey":"session-a-757"})).unwrap();
    let a_address = bound(&mut a, &id, &format!("{origin}/wms"));
    assert!(web::request(&mut b, Some(&id), &format!("{origin}/wms")).is_err());
    assert!(web::request(&mut b, None, &a_address).is_err());
    b.run("connection.login", &json!({"id":id,"apiKey":"session-b-757"})).unwrap();
    let b_address = bound(&mut b, &id, &format!("{origin}/wms"));
    assert_ne!(a_address, b_address);
    assert_eq!(header(&a_address).unwrap().1, "session-a-757");
    assert_eq!(header(&b_address).unwrap().1, "session-b-757");
    let second =
        new(&mut a, json!({"name":"Other account", "kind":"wms","address":format!("{origin}/other"),"auth":"header","keyName":"X-Key"}));
    a.run("connection.login", &json!({"id":second,"apiKey":"second-a-757"})).unwrap();
    assert_eq!(header(&a_address).unwrap().1, "session-a-757");
    assert_eq!(header(&bound(&mut a, &second, &format!("{origin}/wms"))).unwrap().1, "second-a-757");
    drop(b);
    assert!(logins::apply(&b_address).is_err());
    assert_eq!(header(&a_address).unwrap().1, "session-a-757");
    drop(a);
    assert!(logins::apply(&a_address).is_err());
}

#[test]
fn a_project_without_its_connection_or_login_asks_then_rebinds_without_persisting_authority() {
    let origin = "https://open757.example.org";
    let mut s = session(None);
    s.run("project.openText", &json!({"text":project(origin,"c7")})).unwrap();
    let r = requests(&mut s);
    assert_eq!((r[0]["reason"].as_str(), r[0]["origin"].as_str(), r[0]["layers"].as_u64()), (Some("connection"), Some(origin), Some(1)));
    let id = new(&mut s, json!({"kind":"wmts","address":format!("{origin}/wmts"),"auth":"header","keyName":"X-Key"}));
    // A connection made meanwhile is nobody's choice for layers that recorded another one.
    assert_eq!(requests(&mut s)[0]["reason"], "connection");
    assert_eq!(s.map().tiles[0].connection, "c7");
    s.run("connection.login", &json!({"id":id,"apiKey":"open-sentinel-757"})).unwrap();
    assert_eq!(requests(&mut s), json!([]));
    assert_eq!(s.map().tiles[0].connection, id);
    let runtime = s.map().tiles[0].url.clone();
    assert_eq!(header(&runtime).unwrap().1, "open-sentinel-757");
    let (saved, _) = s.project.saved_text(None).unwrap();
    assert!(saved.contains(&format!(r#""connection":"{id}""#)) || saved.contains(&format!(r#""connection": "{id}""#)));
    assert!(!saved.contains("rustgis-login") && !saved.contains("open-sentinel-757"));
    assert_eq!(crate::sources::redact(&runtime), format!("{origin}/tiles/{{z}}/{{x}}/{{y}}.png"));
    s.notify("server echoed open-sentinel-757", true);
    let error =
        s.run("map.addTiles", &json!({"url":format!("{origin}/tiles/{{z}}/{{x}}/{{y}}.png"),"kind":"open-sentinel-757"})).unwrap_err();
    let visible = format!(
        "{}{}{}{:?}{:?}",
        s.run("connection.list", &Value::Null).unwrap(),
        s.connections.to_json(),
        saved,
        s.notices,
        s.database_logins
    );
    assert!(!error.contains("open-sentinel-757") && !visible.contains("open-sentinel-757"));
    assert!(crate::sources::holds_secret(&json!({"apiKey":"open-sentinel-757"})));
    // A persisted runtime token from another session is not adopted on load.
    let forged = serde_json::to_string(&s.project).unwrap();
    let mut other = session(None);
    other.run("project.openText", &json!({"text":forged})).unwrap();
    assert_eq!(requests(&mut other)[0]["reason"], "connection");
    assert!(header(&other.map().tiles[0].url).is_none());
    s.run("connection.edit", &json!({"id":id,"address":"https://moved-open757.example.org/wmts"})).unwrap();
    s.run("project.openText", &json!({"text":saved})).unwrap();
    assert_eq!(requests(&mut s)[0]["reason"], "connection");
}

#[test]
fn recorded_connections_at_one_origin_remain_independent_and_unassigned_layers_ask() {
    let origin = "https://two757.example.org";
    let mut s = session(None);
    let a = new(&mut s, json!({"kind":"wms","name":"A","address":format!("{origin}/wms"),"auth":"header","keyName":"X-Key"}));
    let b = new(&mut s, json!({"kind":"wms","name":"B","address":format!("{origin}/wms?map=b")}));
    s.run("project.openText", &json!({"text":project(origin, "")})).unwrap();
    assert_eq!(requests(&mut s)[0]["reason"], "choose");
    s.run("connection.login", &json!({"id":a,"apiKey":"a-sentinel-757"})).unwrap();
    assert_eq!(s.map().tiles[0].connection, a);
    assert_eq!(header(&s.map().tiles[0].url).unwrap().1, "a-sentinel-757");
    s.run("connection.login", &json!({"id":b})).unwrap();
    // Choosing anonymous B for unassigned requests cannot revoke A's recorded layer.
    assert_eq!(header(&s.map().tiles[0].url).unwrap().1, "a-sentinel-757");
    assert!(header(&web::request(&mut s, None, &format!("{origin}/wms")).unwrap()).is_none());
}

#[test]
fn public_connections_at_one_origin_never_ask_which_to_use() {
    // Two public connections of one server: any choice reads the same anonymously.
    let origin = "https://public757.example.org";
    let mut s = session(None);
    new(&mut s, json!({"kind":"wms","name":"Public WMS","address":format!("{origin}/wms")}));
    new(&mut s, json!({"kind":"wfs","name":"Public WFS","address":format!("{origin}/wfs")}));
    s.run("map.addTiles", &json!({"url":format!("{origin}/tiles/{{z}}/{{x}}/{{y}}.png"),"name":"Bare"})).unwrap();
    assert_eq!(requests(&mut s), json!([]));
    s.run("project.openText", &json!({"text":project(origin, "")})).unwrap();
    assert_eq!(requests(&mut s), json!([]));
}

#[test]
fn test_refuses_changed_endpoint_and_missing_login_instead_of_testing_anonymously() {
    let mut s = session(None);
    let id = new(&mut s, json!({"kind":"wms","address":"https://test757.example.org/wms","auth":"header","keyName":"X-Key"}));
    for changed in [
        json!({"address":"https://else757.example.org/wms"}),
        json!({"auth":"basic"}),
        json!({"user":"other"}),
        json!({"keyName":"X-Other"}),
        json!({"kind":"stac"}),
    ] {
        let mut p = changed;
        p["id"] = json!(id);
        p["probe"] = json!(true);
        p["apiKey"] = json!("not-forwarded-757");
        assert_eq!(s.run("connection.test", &p).unwrap_err(), web::SAVE_BEFORE_TEST);
    }
    assert_eq!(s.run("connection.test", &json!({"id":id,"probe":true})).unwrap_err(), web::REQUIRED);
    assert!(!s.database_logins.has_session_password(&id));
}

#[test]
fn junk_login_parameters_never_panic() {
    let mut s = session(Some(Arc::new(MemorySecrets::new())));
    let id = new(&mut s, json!({"kind":"csw","address":"https://junk757.example.org/csw","auth":"header","keyName":"X-Key"}));
    let junk = [
        json!(null),
        json!([]),
        json!({"id":id,"apiKey":7,"password":{"a":1},"remember":[],"allowInsecure":null}),
        json!({"id":id,"auth":"\u{0}","user":"\u{feff}","keyName":"x".repeat(10_000)}),
        json!({"id":id,"apiKey":"\u{7f}","allowInsecure":true}),
        json!({"id":[id],"connection":{"id":id}}),
    ];
    for cmd in
        ["connection.new", "connection.edit", "connection.login", "connection.logout", "connection.test", "map.addTiles", "service.add"]
    {
        for p in &junk {
            let _ = s.run(cmd, p);
        }
    }
}

#[test]
fn a_group_child_read_in_place_is_counted_once_and_refresh_respects_logout() {
    let origin = "https://group757.example.org";
    let mut s = session(Some(Arc::new(MemorySecrets::new())));
    let id = new(&mut s, json!({"kind":"ogcapi","address":format!("{origin}/ogc"),"auth":"basic","user":"ana"}));
    let mut layer = rustgis_map::Layer::new(41, rustgis_data::Dataset::new("Roads", rustgis_data::GeometryType::None, Vec::new()));
    layer.connection = id.clone();
    layer.source = Some(serde_json::from_value(json!({"path":format!("{origin}/ogc/collections/roads/items")})).unwrap());
    s.project.map_mut().layers.push(layer);
    s.project.map_mut().service_groups.push(rustgis_map::ServiceGroup {
        id: 5,
        name: "Imported".into(),
        url: format!("{origin}/ogc/collections"),
        kind: "ogcapi".into(),
        expanded: true,
        children: vec![rustgis_map::ServiceChild {
            layer: 41,
            collection: "roads".into(),
            source_url: format!("{origin}/ogc/collections/roads"),
        }],
    });
    web::resolve(&mut s, None);
    assert_eq!(requests(&mut s)[0]["layers"], 1);
    s.run("connection.login", &json!({"id":id,"password":"group-sentinel-757"})).unwrap();
    assert_eq!(
        header(s.map().layers[0].source.as_ref().unwrap().path.as_deref().unwrap()).unwrap().1,
        logins::basic("ana", "group-sentinel-757")
    );
    assert!(header(&s.map().service_groups[0].url).is_some());
    s.run("connection.logout", &json!({"id":id})).unwrap();
    assert_eq!(s.run("service.refresh", &json!({"group":5,"text":"not a document"})).unwrap_err(), web::REQUIRED);
    assert_eq!(s.run("connection.list", &Value::Null).unwrap()["connections"][0]["loggedIn"], false);
}

#[test]
fn a_definition_reloaded_with_another_identity_cannot_reuse_a_session_secret() {
    let mut s = session(None);
    let id = new(&mut s, json!({"kind":"wms","address":"https://identity757.example.org/wms","auth":"header","keyName":"X-Key"}));
    s.run("connection.login", &json!({"id":id,"apiKey":"identity-sentinel-757"})).unwrap();
    let mut prefs: Value = serde_json::from_str(&s.connections.to_json()).unwrap();
    prefs["connections"][0]["address"] = json!("https://changed757.example.org/wms");
    s.connections = crate::connections::Connections::from_json(&prefs.to_string());
    assert_eq!(web::request(&mut s, Some(&id), "https://changed757.example.org/wms").unwrap_err(), web::UNUSABLE);
    assert!(header("https://changed757.example.org/wms").is_none());
    assert_eq!(s.run("connection.list", &Value::Null).unwrap()["connections"][0]["loggedIn"], false);
}

#[test]
fn logins_drop_the_tile_cache_only_when_one_ends_or_is_replaced() {
    let mut s = session(None);
    let public = new(&mut s, json!({"kind":"wms","name":"Public","address":"https://cache-public757.example.org/wms"}));
    let private = new(
        &mut s,
        json!({"kind":"wms","name":"Private","address":"https://cache-private757.example.org/wms","auth":"header","keyName":"X-Key"}),
    );
    let start = web::cache_clears();
    // Opening a connection without a login method, and a first login, change no cached response.
    s.run("connection.login", &json!({"id":public})).unwrap();
    s.run("connection.login", &json!({"id":public})).unwrap();
    s.run("connection.login", &json!({"id":private,"apiKey":"cache-sentinel-757"})).unwrap();
    assert_eq!(web::cache_clears(), start);
    // Logging in again without typing (choosing it again) keeps the binding and the cache.
    let first = bound(&mut s, &private, "https://cache-private757.example.org/wms");
    s.run("connection.login", &json!({"id":private})).unwrap();
    assert_eq!(bound(&mut s, &private, "https://cache-private757.example.org/wms"), first);
    assert_eq!(web::cache_clears(), start);
    // Replacing or ending a login leaves authenticated responses behind: those go.
    s.run("connection.login", &json!({"id":private,"apiKey":"replacement-sentinel-757"})).unwrap();
    assert_eq!(web::cache_clears(), start + 1);
    s.run("connection.logout", &json!({"id":private})).unwrap();
    assert_eq!(web::cache_clears(), start + 2);
    s.run("connection.logout", &json!({"id":private})).unwrap();
    assert_eq!(web::cache_clears(), start + 2, "there was no login to end");
}

#[test]
fn a_recorded_connection_that_is_gone_is_asked_about_not_replaced_by_the_only_other_one() {
    let origin = "https://gone757.example.org";
    let mut s = session(None);
    let other = new(&mut s, json!({"kind":"wms","name":"Other","address":format!("{origin}/wms"),"auth":"header","keyName":"X-Key"}));
    s.run("connection.login", &json!({"id":other,"apiKey":"other-sentinel-757"})).unwrap();
    s.run("project.openText", &json!({"text":project(origin,"deleted")})).unwrap();
    assert_eq!(requests(&mut s)[0]["reason"], "connection");
    assert_eq!(s.map().tiles[0].connection, "deleted");
    assert!(header(&s.map().tiles[0].url).is_none(), "the other connection's login is not sent");
    // Choosing it is the user's to do.
    s.run("connection.login", &json!({"id":other})).unwrap();
    assert_eq!(requests(&mut s), json!([]));
    assert_eq!(s.map().tiles[0].connection, other);
    assert_eq!(header(&s.map().tiles[0].url).unwrap().1, "other-sentinel-757");

    // Layers that record no connection still use the origin's only one.
    let mut fresh = session(None);
    let sole = new(&mut fresh, json!({"kind":"wms","address":format!("{origin}/wms"),"auth":"header","keyName":"X-Key"}));
    fresh.run("connection.login", &json!({"id":sole,"apiKey":"sole-sentinel-757"})).unwrap();
    fresh.run("project.openText", &json!({"text":project(origin,"")})).unwrap();
    assert_eq!(requests(&mut fresh), json!([]));
    assert_eq!(fresh.map().tiles[0].connection, sole);

    // Nor does an unrecorded layer's use of it become the choice for a layer that recorded a gone one.
    let mut mixed = session(None);
    let sole = new(&mut mixed, json!({"kind":"wms","address":format!("{origin}/wms")}));
    let mut p = Project::default();
    for (name, recorded) in [("Unrecorded", ""), ("Gone", "deleted")] {
        let mut t = rustgis_map::TileLayer::new(name, rustgis_map::TileKind::Raster, format!("{origin}/{name}/{{z}}/{{x}}/{{y}}.png"));
        t.connection = recorded.into();
        p.map_mut().tiles.push(t);
    }
    mixed.run("project.openText", &json!({"text":serde_json::to_string(&p).unwrap()})).unwrap();
    let recorded = |s: &Session, name: &str| s.map().tiles.iter().find(|t| t.name == name).unwrap().connection.clone();
    assert_eq!(recorded(&mixed, "Unrecorded"), sole);
    assert_eq!(recorded(&mixed, "Gone"), "deleted");
    assert_eq!(requests(&mut mixed)[0]["reason"], "connection");
}

#[test]
fn an_unsaved_anonymous_test_never_carries_a_saved_login() {
    let mut s = session(None);
    let address = "https://probe757.example.org/wms";
    let id = new(&mut s, json!({"kind":"wms","address":address,"auth":"header","keyName":"X-Key"}));
    s.run("connection.login", &json!({"id":id,"apiKey":"probe-sentinel-757"})).unwrap();
    let unsaved = s.run("connection.test", &json!({"kind":"wms","address":address,"probe":true})).unwrap();
    assert!(header(unsaved["fetch"].as_str().unwrap()).is_none(), "{unsaved}");
    let saved = s.run("connection.test", &json!({"id":id,"probe":true})).unwrap();
    assert_eq!(header(saved["fetch"].as_str().unwrap()).unwrap().1, "probe-sentinel-757");
    // The anonymous read ended with the test.
    assert!(header(&web::request(&mut s, None, address).unwrap()).is_some());
}

#[test]
fn plugins_cannot_send_a_saved_web_service_login() {
    super::isolate_plugins();
    let mut s = session(None);
    let origin = "https://plugin757.example.org";
    let id = new(&mut s, json!({"kind":"wms","address":format!("{origin}/wms"),"auth":"header","keyName":"X-Key"}));
    s.run("connection.login", &json!({"id":id,"apiKey":"plugin-sentinel-757"})).unwrap();
    super::plugin("eng-login-757", &["edit", "network"]);
    let tiles = format!("{origin}/tiles/{{z}}/{{x}}/{{y}}.png");
    // Naming the connection is refused, as logging in to it is.
    for command in ["map.addTiles", "layer.addUrl", "service.layers"] {
        let e = super::forward(&mut s, "eng-login-757", command, json!({"url":tiles,"connection":id})).unwrap_err();
        assert!(e.contains("web-service connection"), "{command}: {e}");
    }
    assert!(s.map().tiles.is_empty());
    // Not naming it, the only one at the origin isn't used for it either; the user's own command is.
    super::forward(&mut s, "eng-login-757", "map.addTiles", json!({"url":tiles})).unwrap();
    assert!(header(&s.map().tiles[0].url).is_none());
    assert!(s.map().tiles[0].connection.is_empty());
    s.run("map.addTiles", &json!({"url":tiles,"name":"Mine"})).unwrap();
    let mine = s.map().tiles.iter().find(|t| t.name == "Mine").unwrap();
    assert_eq!(header(&mine.url).unwrap().1, "plugin-sentinel-757");
    rustgis_plugins::unload("eng-login-757");
}

#[test]
fn only_web_services_compare_a_test_with_their_saved_definition() {
    let mut s = session(None);
    let db = new(&mut s, json!({"kind":"postgres","address":"postgres:host=db757.example.org dbname=gis user=ana"}));
    let out = s
        .run(
            "connection.test",
            &json!({"id":db,"probe":true,"kind":"mysql","address":"postgres:host=elsewhere dbname=gis user=ana","auth":null,"user":7}),
        )
        .unwrap();
    assert_eq!(out["connection"], db);
}

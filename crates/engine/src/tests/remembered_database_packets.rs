use super::*;
use rustgis_data::secrets::{MemorySecrets, SecretStore};
use std::io::{Read, Write};
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
    mpsc,
};

struct Server {
    address: String,
    packets: mpsc::Receiver<Vec<u8>>,
    stop: Arc<AtomicBool>,
    worker: Option<std::thread::JoinHandle<()>>,
}
impl Server {
    fn new() -> Self {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        listener.set_nonblocking(true).unwrap();
        let stop = Arc::new(AtomicBool::new(false));
        let stopped = stop.clone();
        let (send, packets) = mpsc::channel();
        let worker = std::thread::spawn(move || {
            while !stopped.load(Ordering::Relaxed) {
                let (mut stream, _) = match listener.accept() {
                    Ok(pair) => pair,
                    Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                        std::thread::sleep(std::time::Duration::from_millis(2));
                        continue;
                    }
                    Err(_) => break,
                };
                // macOS accepts can inherit the listener's nonblocking flag.
                stream.set_nonblocking(false).unwrap();
                stream.set_read_timeout(Some(std::time::Duration::from_secs(3))).unwrap();
                let mut size = [0; 4];
                if stream.read_exact(&mut size).is_err() {
                    continue;
                }
                let size = u32::from_be_bytes(size);
                if !(4..=65536).contains(&size) {
                    continue;
                }
                if stream.read_exact(&mut vec![0; (size - 4) as usize]).is_err() {
                    continue;
                }
                if stream.write_all(b"R\0\0\0\x08\0\0\0\x03").is_err() {
                    continue;
                }
                let mut tag = [0];
                let mut size = [0; 4];
                if stream.read_exact(&mut tag).is_err() || tag[0] != b'p' || stream.read_exact(&mut size).is_err() {
                    continue;
                }
                let size = u32::from_be_bytes(size);
                if !(4..=65536).contains(&size) {
                    continue;
                }
                let mut password = vec![0; (size - 4) as usize];
                if stream.read_exact(&mut password).is_err() {
                    continue;
                }
                let echoed = String::from_utf8_lossy(&password).trim_end_matches('\0').to_string();
                send.send(password).unwrap();
                let body = format!("SFATAL\0VFATAL\0C28P01\0Mrefused {echoed}\0\0");
                let _ = stream.write_all(b"E");
                let _ = stream.write_all(&((body.len() + 4) as u32).to_be_bytes());
                let _ = stream.write_all(body.as_bytes());
            }
        });
        Self {
            address: format!("postgres:host=127.0.0.1 port={port} user=packet_user dbname=gis sslmode=disable"),
            packets,
            stop,
            worker: Some(worker),
        }
    }
    #[track_caller]
    fn take(&self) -> Vec<u8> {
        self.packets.recv_timeout(std::time::Duration::from_secs(3)).unwrap()
    }
    fn empty(&self) {
        assert!(self.packets.try_recv().is_err(), "unexpected password packet");
    }
}
impl Drop for Server {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        if let Some(worker) = self.worker.take() {
            worker.join().unwrap();
        }
    }
}
fn app(store: Arc<MemorySecrets>, prefs: &str) -> Session {
    let mut s = Session::new(Project::default());
    s.database_logins.set_store(Some(store));
    s.connections = crate::connections::Connections::from_json(prefs);
    s
}
fn project(address: &str) -> String {
    let mut p = world().project;
    for layer in p.map_mut().layers.iter_mut().take(2) {
        layer.source = Some(serde_json::from_value(json!({"postgis":{"database":address,"table":"roads","family":"Lines"}})).unwrap());
    }
    serde_json::to_string(&p).unwrap()
}
fn remember(s: &mut Session, name: &str, address: &str, password: &str) -> String {
    let id =
        s.run("connection.new", &json!({"kind":"postgres","name":name,"address":address})).unwrap()["id"].as_str().unwrap().to_string();
    assert_eq!(s.run("connection.login", &json!({"id":id,"password":password,"remember":true})).unwrap()["passwordSaved"], true);
    id
}

// Driver passwords are process-global. Isolate packet scenarios from unrelated full-suite
// project resets (and ambient-password precedence tests) without mutating the parent's environment.
fn isolated(name: &str) -> bool {
    isolated_with(name, &[])
}

/// The same isolation, with extra environment for the child (an ambient password, say).
fn isolated_with(name: &str, extra: &[(&str, &str)]) -> bool {
    if std::env::var("RUSTGIS_PACKET_SCENARIO").as_deref() == Ok(name) {
        return false;
    }
    let mut command = std::process::Command::new(std::env::current_exe().unwrap());
    command
        .args(["--exact", &format!("tests::remembered_database_packets::{name}"), "--test-threads=1"])
        .env("RUSTGIS_PACKET_SCENARIO", name)
        .env_remove("PGPASSWORD")
        .env_remove("PGPASSFILE");
    for (key, value) in extra {
        command.env(key, value);
    }
    let output = command.output().unwrap();
    assert!(output.status.success(), "isolated database scenario failed: {}", String::from_utf8_lossy(&output.stdout));
    true
}
#[test]
fn remembered_database_unique_packet_refusal_and_replacement() {
    if isolated("remembered_database_unique_packet_refusal_and_replacement") {
        return;
    }
    let server = Server::new();
    let store = Arc::new(MemorySecrets::new());
    let mut first = app(store.clone(), "{}");
    let id = remember(&mut first, "One", &server.address, "saved-packet-sentinel");
    let prefs = first.connections.to_json();
    let mut s = app(store.clone(), &prefs);
    let text = project(&server.address);
    s.run("project.openText", &json!({"text":text})).unwrap();
    assert!(s.map().layers.iter().take(2).all(|l| l.source.as_ref().is_some_and(|src| src.login)), "{:?}", s.notices);
    assert_eq!(server.take(), b"saved-packet-sentinel\0");
    server.empty();
    assert!(s.map().layers.iter().take(2).all(|l| l.source.as_ref().unwrap().login));
    assert_eq!(s.run("connection.list", &Value::Null).unwrap()["loginRequests"][0]["reason"], "password");
    assert!(!s.notices.iter().any(|n| n.text.contains("saved-packet-sentinel")));
    assert!(s.map().layers.iter().take(2).all(|l| !l.source.as_ref().unwrap().error.as_ref().unwrap().contains("saved-packet-sentinel")));
    let saved = store.load(&format!("connection/{id}")).unwrap();
    s.run("project.openText", &json!({"text":text})).unwrap();
    server.empty();
    s.run("connection.login", &json!({"id":id,"password":"unchecked-new"})).unwrap();
    assert_eq!(server.take(), b"unchecked-new\0");
    assert_eq!(server.take(), b"unchecked-new\0");
    assert_eq!(store.load(&format!("connection/{id}")).unwrap(), saved);
    assert!(s.database_logins.is_rejected(&id));
    // A refused password is not "available for this session", and the store keeps the old value.
    let refused = s.run("connection.login", &json!({"id":id,"password":"refused-with-remember","remember":true})).unwrap();
    assert_eq!(refused["passwordSaved"], false, "{refused}");
    assert!(refused.get("warning").is_none(), "a refused password must not be called available: {refused}");
    assert_eq!(server.take(), b"refused-with-remember\0");
    assert_eq!(server.take(), b"refused-with-remember\0");
    assert_eq!(store.load(&format!("connection/{id}")).unwrap(), saved);
    assert!(s.database_logins.is_rejected(&id));
    s.run("project.new", &Value::Null).unwrap();
    assert_eq!(s.run("connection.login", &json!({"id":id,"password":"checked-new","remember":true})).unwrap()["passwordSaved"], true);
    assert!(!s.database_logins.is_rejected(&id));
}
#[test]
fn remembered_database_ambiguity_requires_exact_id_packet() {
    if isolated("remembered_database_ambiguity_requires_exact_id_packet") {
        return;
    }
    let server = Server::new();
    let store = Arc::new(MemorySecrets::new());
    let mut first = app(store.clone(), "{}");
    let one = remember(&mut first, "One", &server.address, "first-sentinel");
    let two = remember(&mut first, "Two", &server.address, "second-sentinel");
    let mut s = app(store.clone(), &first.connections.to_json());
    let text = project(&server.address);
    s.run("project.openText", &json!({"text":text})).unwrap();
    server.empty();
    let listing = s.run("connection.list", &Value::Null).unwrap();
    assert_eq!(listing["loginRequests"][0]["reason"], "choose");
    assert_eq!(listing["loginRequests"][0]["connections"], json!([one, two]));
    assert!(s.connections.for_layer(&s.map().layers[0]).is_none());
    s.run("connection.login", &json!({"id":two})).unwrap();
    assert_eq!(server.take(), b"second-sentinel\0");
    server.empty();
    // Fresh sessions still see ambiguity; no prior automatic activation authorizes a choice.
    let mut fresh = app(store.clone(), &first.connections.to_json());
    fresh.run("project.openText", &json!({"text":text})).unwrap();
    server.empty();
    assert_eq!(fresh.run("connection.list", &Value::Null).unwrap()["loginRequests"][0]["reason"], "choose");
    for id in [&one, &two] {
        fresh.run("connection.forgetPassword", &json!({"id":id})).unwrap();
    }
    let mut forgotten = app(store, &fresh.connections.to_json());
    forgotten.run("project.openText", &json!({"text":text})).unwrap();
    server.empty();
}

#[test]
fn remembered_database_exact_address_and_explicit_precedence_packets() {
    if isolated("remembered_database_exact_address_and_explicit_precedence_packets") {
        return;
    }
    let server = Server::new();
    let store = Arc::new(MemorySecrets::new());
    let mut seed = app(store.clone(), "{}");
    let id = remember(&mut seed, "Exact", &server.address, "exact-saved-sentinel");
    let prefs = seed.connections.to_json();
    let port = server.address.split_whitespace().find_map(|s| s.strip_prefix("port=")).unwrap();
    for address in [
        format!("postgresql://packet_user@127.0.0.1:{port}/gis?sslmode=disable"),
        server.address.replace("user=packet_user dbname=gis", "dbname=gis user=packet_user"),
        server.address.replace("user=packet_user", "user=other"),
    ] {
        let mut s = app(store.clone(), &prefs);
        s.run("project.openText", &json!({"text":project(&address)})).unwrap();
        server.empty();
    }
    let mut explicit = app(store, &prefs);
    let (_, key) = crate::connections::database_key(&server.address).unwrap();
    rustgis_data::source::duckdb::remember_password(&key, "explicit-sentinel");
    explicit.run("project.openText", &json!({"text":project(&server.address)})).unwrap();
    assert_eq!(server.take(), b"explicit-sentinel\0");
    assert_eq!(server.take(), b"explicit-sentinel\0");
    server.empty();
    // An explicit login without a password must not replace the address's own password with the
    // saved one either: the address's wins, as it does when the project opens.
    explicit.run("connection.login", &json!({"id":id})).unwrap();
    assert_eq!(server.take(), b"explicit-sentinel\0");
    assert_eq!(server.take(), b"explicit-sentinel\0");
    server.empty();
}

/// An explicitly initiated connection prefers the saved password to an ambient one (`PGPASSWORD`).
/// The ambient value is set in a child process: it is process-global, and the rest of the suite
/// must not see it.
#[test]
fn remembered_database_saved_password_wins_over_ambient_environment() {
    if isolated_with("remembered_database_saved_password_wins_over_ambient_environment", &[("PGPASSWORD", "ambient-wrong-sentinel")]) {
        return;
    }
    let server = Server::new();
    let store = Arc::new(MemorySecrets::new());
    let mut seed = app(store.clone(), "{}");
    let id = remember(&mut seed, "Ambient", &server.address, "ambient-saved-sentinel");
    let mut s = app(store, &seed.connections.to_json());
    // `connection.test` is explicitly initiated, so the driver may consult the ambient password:
    // the saved one must be sent instead.
    assert!(s.run("connection.test", &json!({"id":id})).is_err(), "the refusing server must fail the listing");
    assert_eq!(server.take(), b"ambient-saved-sentinel\0");
    server.empty();
}

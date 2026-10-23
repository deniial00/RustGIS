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
        Self::start(false)
    }

    /// When authentication succeeds, fail the first catalogue query instead of the password.
    fn start(authenticate: bool) -> Self {
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
                if authenticate {
                    // AuthenticationOk and ReadyForQuery: the client's session setup is a
                    // simple Query containing three SET statements.
                    if stream.write_all(b"R\0\0\0\x08\0\0\0\0Z\0\0\0\x05I").is_err() {
                        continue;
                    }
                    let mut tag = [0];
                    let mut size = [0; 4];
                    if stream.read_exact(&mut tag).is_err() || tag[0] != b'Q' || stream.read_exact(&mut size).is_err() {
                        continue;
                    }
                    let size = u32::from_be_bytes(size);
                    if !(4..=65536).contains(&size) || stream.read_exact(&mut vec![0; (size - 4) as usize]).is_err() {
                        continue;
                    }
                    for _ in 0..3 {
                        stream.write_all(b"C\0\0\0\x08SET\0").unwrap();
                    }
                    stream.write_all(b"Z\0\0\0\x05I").unwrap();
                    // The catalogue lookup uses an extended query. Consume through Sync so
                    // the server's non-login error occurs after successful authentication.
                    loop {
                        let mut size = [0; 4];
                        if stream.read_exact(&mut tag).is_err() || stream.read_exact(&mut size).is_err() {
                            break;
                        }
                        let size = u32::from_be_bytes(size);
                        if !(4..=65536).contains(&size) || stream.read_exact(&mut vec![0; (size - 4) as usize]).is_err() {
                            break;
                        }
                        if tag[0] == b'S' {
                            break;
                        }
                    }
                }
                let body = if authenticate {
                    format!("SERROR\0VERROR\0C42P01\0Mfixture listing failed {echoed}\0\0")
                } else {
                    format!("SFATAL\0VFATAL\0C28P01\0Mrefused {echoed}\0\0")
                };
                let _ = stream.write_all(b"E");
                let _ = stream.write_all(&((body.len() + 4) as u32).to_be_bytes());
                let _ = stream.write_all(body.as_bytes());
                if authenticate {
                    let _ = stream.write_all(b"Z\0\0\0\x05I");
                    // A catalogue error is nonfatal: keep the connection alive until the
                    // client closes it, rather than racing its error response with TCP EOF.
                    let mut terminate = [0; 5];
                    let _ = stream.read_exact(&mut terminate);
                }
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
    let stdout = String::from_utf8_lossy(&output.stdout);
    let mut summaries = stdout.lines().filter(|line| line.starts_with("test result:"));
    let ran_one = summaries.next().is_some_and(|line| line.starts_with("test result: ok. 1 passed; 0 failed; 0 ignored;"))
        && summaries.next().is_none();
    assert!(
        output.status.success() && ran_one,
        "isolated database scenario failed or did not run exactly one test:\n{stdout}\n{}",
        String::from_utf8_lossy(&output.stderr)
    );
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

#[test]
fn remembered_database_forget_preserves_active_owner_and_explicit_input() {
    if isolated("remembered_database_forget_preserves_active_owner_and_explicit_input") {
        return;
    }
    use rustgis_data::source::{duckdb, postgis};
    let server = Server::new();
    let store = Arc::new(MemorySecrets::new());
    let mut s = app(store.clone(), "{}");
    let a = remember(&mut s, "A", &server.address, "owner-a");
    let b = remember(&mut s, "B", &server.address, "owner-b");
    let (_, key) = crate::connections::database_key(&server.address).unwrap();
    s.run("connection.forgetPassword", &json!({"id":a})).unwrap();
    assert_eq!(s.database_logins.selection("postgres", &key), Some(b.as_str()));
    assert!(postgis::connect(&key, false).is_err());
    assert_eq!(server.take(), b"owner-b\0");
    assert_eq!(s.database_logins.session_password(&a).unwrap().expose(), "owner-a");
    assert!(store.load(&format!("connection/{a}")).unwrap().is_none());

    duckdb::remember_password(&key, "address-typed");
    s.run("connection.forgetPassword", &json!({"id":a})).unwrap();
    assert!(duckdb::has_explicit_password(&key));
    assert!(postgis::connect(&key, false).is_err());
    assert_eq!(server.take(), b"address-typed\0");

    s.run("connection.login", &json!({"id":b,"password":"owner-b"})).unwrap();
    // A loaded/active cache value can differ from the ID's latest explicit session value.
    // Forget must restore the session value, not preserve the old owned slot by accident.
    s.database_logins.set_session_password(&b, "session-b-new");
    s.run("connection.forgetPassword", &json!({"id":b})).unwrap();
    assert!(postgis::connect(&key, false).is_err());
    assert_eq!(server.take(), b"session-b-new\0");
    assert_eq!(s.database_logins.selection("postgres", &key), Some(b.as_str()));
    server.empty();
}

#[test]
fn remembered_database_forget_saved_only_clears_its_activation() {
    if isolated("remembered_database_forget_saved_only_clears_its_activation") {
        return;
    }
    let server = Server::new();
    let store = Arc::new(MemorySecrets::new());
    let mut seed = app(store.clone(), "{}");
    let id = remember(&mut seed, "Saved", &server.address, "saved-only");
    let mut s = app(store, &seed.connections.to_json());
    let (_, key) = crate::connections::database_key(&server.address).unwrap();
    s.run("connection.login", &json!({"id":id})).unwrap();
    assert!(!s.database_logins.has_session_password(&id));
    s.run("connection.forgetPassword", &json!({"id":id})).unwrap();
    assert!(!rustgis_data::source::duckdb::has_password(&key));
    assert!(!s.database_logins.activations().iter().any(|(_, active)| active == &id));
    server.empty();
}

#[test]
fn remembered_database_test_refusal_restores_prior_session_and_active_password() {
    if isolated("remembered_database_test_refusal_restores_prior_session_and_active_password") {
        return;
    }
    use rustgis_data::source::{duckdb, postgis};
    let server = Server::new();
    let store = Arc::new(MemorySecrets::new());
    let mut s = app(store.clone(), "{}");
    let a = remember(&mut s, "A", &server.address, "prior-a");
    let b = remember(&mut s, "B", &server.address, "prior-b");
    let (_, key) = crate::connections::database_key(&server.address).unwrap();
    let saved = store.load(&format!("connection/{a}")).unwrap();
    for (active, expected) in [(Some(b.as_str()), "prior-b"), (Some(a.as_str()), "prior-a"), (None, "prior-address")] {
        if let Some(id) = active {
            s.run("connection.login", &json!({"id":id,"password":expected})).unwrap();
        } else {
            duckdb::remember_password(&key, expected);
        }
        let selection = s.database_logins.selection("postgres", &key).map(str::to_string);
        let activations = s.database_logins.activations();
        let error = s.run("connection.test", &json!({"id":a,"password":"test-refused","remember":true})).unwrap_err();
        assert!(!error.contains("test-refused"));
        assert_eq!(server.take(), b"test-refused\0");
        assert_eq!(s.database_logins.session_password(&a).unwrap().expose(), "prior-a");
        assert_eq!(s.database_logins.selection("postgres", &key), selection.as_deref());
        assert_eq!(s.database_logins.activations(), activations);
        assert_eq!(duckdb::has_explicit_password(&key), active.is_none());
        assert!(postgis::connect(&key, false).is_err());
        let mut packet = expected.as_bytes().to_vec();
        packet.push(0);
        assert_eq!(server.take(), packet);
        assert_eq!(store.load(&format!("connection/{a}")).unwrap(), saved);
        assert!(!s.database_logins.is_rejected(&a));
    }
    duckdb::set_connection_password(&key, &a, Some("cleanup-owned"));
    duckdb::forget_connection_password(&a);
    let mut fresh = app(Arc::new(MemorySecrets::new()), "{}");
    fresh.run("connection.new", &json!({"kind":"postgres","name":"Empty","address":server.address})).unwrap();
    assert!(fresh.run("connection.test", &json!({"id":"c1","password":"test-refused"})).is_err());
    assert_eq!(server.take(), b"test-refused\0");
    assert!(!fresh.database_logins.has_session_password("c1"));
    assert!(fresh.database_logins.selection("postgres", &key).is_none());
    assert!(!duckdb::has_password(&key));
    server.empty();
}

#[test]
fn remembered_database_test_listing_failure_keeps_authenticated_session() {
    if isolated("remembered_database_test_listing_failure_keeps_authenticated_session") {
        return;
    }
    let server = Server::start(true);
    let store = Arc::new(MemorySecrets::new());
    let mut s = app(store.clone(), "{}");
    let a = remember(&mut s, "A", &server.address, "prior-a");
    let b = remember(&mut s, "B", &server.address, "prior-b");
    let error = s.run("connection.test", &json!({"id":a,"password":"accepted-new","remember":true})).unwrap_err();
    assert!(error.contains("fixture listing failed"), "{error}");
    assert!(!error.contains("accepted-new"));
    assert_eq!(server.take(), b"accepted-new\0");
    let (_, key) = crate::connections::database_key(&server.address).unwrap();
    assert_eq!(s.database_logins.session_password(&a).unwrap().expose(), "accepted-new");
    assert_eq!(s.database_logins.selection("postgres", &key), Some(a.as_str()));
    assert!(s.database_logins.activations().contains(&(key.clone(), a.clone())));
    assert!(s.database_logins.requests(s.opened).next().is_none());
    let envelope: Value = serde_json::from_str(store.load(&format!("connection/{a}")).unwrap().unwrap().expose()).unwrap();
    assert_eq!(envelope["password"], "prior-a");
    assert_eq!(s.database_logins.session_password(&b).unwrap().expose(), "prior-b");
    server.empty();
}

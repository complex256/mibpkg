//! The server end to end over a real socket (loopback), with a recording runner instead of
//! spawning `mibpkg gem`.
mod common;
use mibpkg::index::Collection;
use mibpkg::web::auth::{host_ok, Auth, LoginError};
use mibpkg::web::jobs::RecordingRunner;
use mibpkg::web::settings::{hash_pin, WebSettings};
use mibpkg::web::{serve, App};
use serde_json::Value;
use std::io::{Read, Write};
use std::net::{SocketAddr, TcpListener, TcpStream};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

struct Server {
    addr: SocketAddr,
    app: Arc<App>,
    runner: Arc<RecordingRunner>,
    exits: Arc<AtomicUsize>,
    _f: common::Fixture,
}

fn start_with(f: common::Fixture, runner: RecordingRunner) -> Server {
    let runner = Arc::new(runner);
    let exits = Arc::new(AtomicUsize::new(0));
    let e = exits.clone();
    let app = Arc::new(
        App::new(f.layout.clone(), Box::new(|| None))
            .with_runner(Box::new(runner.clone()))
            .with_exit(Box::new(move || {
                e.fetch_add(1, Ordering::SeqCst);
            })),
    );
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap();
    app.port.store(addr.port(), Ordering::SeqCst);
    let a = app.clone();
    std::thread::spawn(move || serve(a, listener));
    Server { addr, app, runner, exits, _f: f }
}

/// A fixture with demo 1.0.0 on the card (installable).
fn fixture_with_card() -> common::Fixture {
    let f = common::fixture();
    let src = common::pkg_src(f.root(), "demo", "1.0.0", "sleep 30", "");
    Collection::load(&common::collection(&f.sd(), "complex256", &[src])).unwrap();
    f
}

fn start() -> Server {
    start_with(fixture_with_card(), RecordingRunner::default())
}

struct Reply {
    status: u16,
    head: String,
    body: Vec<u8>,
}

impl Reply {
    fn json(&self) -> Value {
        serde_json::from_slice(&self.body).unwrap_or(Value::Null)
    }
}

fn send(addr: SocketAddr, raw: &[u8]) -> Reply {
    let mut s = TcpStream::connect(addr).unwrap();
    s.set_read_timeout(Some(Duration::from_secs(5))).unwrap();
    s.write_all(raw).unwrap();
    let mut out = Vec::new();
    let _ = s.read_to_end(&mut out);
    let split = out.windows(4).position(|w| w == b"\r\n\r\n").map(|i| i + 4).unwrap_or(out.len());
    let head = String::from_utf8_lossy(&out[..split]).to_string();
    let status = head.split(' ').nth(1).and_then(|s| s.parse().ok()).unwrap_or(0);
    Reply { status, head, body: out[split..].to_vec() }
}

fn get(srv: &Server, path: &str, cookie: Option<&str>) -> Reply {
    let c = cookie.map(|c| format!("Cookie: {c}\r\n")).unwrap_or_default();
    send(srv.addr, format!("GET {path} HTTP/1.1\r\nHost: 127.0.0.1:{}\r\n{c}\r\n", srv.addr.port()).as_bytes())
}

fn write(srv: &Server, method: &str, path: &str, body: &[u8], cookie: Option<&str>) -> Reply {
    let c = cookie.map(|c| format!("Cookie: {c}\r\n")).unwrap_or_default();
    let mut raw = format!(
        "{method} {path} HTTP/1.1\r\nHost: 127.0.0.1\r\nX-Mibpkg: 1\r\nContent-Type: application/json\r\nContent-Length: {}\r\n{c}\r\n",
        body.len()
    )
    .into_bytes();
    raw.extend_from_slice(body);
    send(srv.addr, &raw)
}

fn wait_done(srv: &Server, job: &str) -> Value {
    let end = Instant::now() + Duration::from_secs(5);
    loop {
        let v = get(srv, &format!("/api/jobs/{job}?since=0"), None).json();
        if v["done"] == true || Instant::now() > end {
            return v;
        }
        std::thread::sleep(Duration::from_millis(20));
    }
}

#[test]
fn hosts_must_be_ip_literals_or_localhost() {
    for ok in ["127.0.0.1", "10.255.1.1:80", "localhost", "localhost:8080", "[::1]:80", "100.64.0.55"] {
        assert!(host_ok(Some(ok)), "{ok}");
    }
    for bad in [None, Some(""), Some("evil.example"), Some("10.255.1.1.nip.io"), Some("localhost.evil"), Some("1.2.3.4:x")] {
        assert!(!host_ok(bad), "{bad:?}");
    }
}

#[test]
fn the_page_and_status_are_served() {
    let srv = start();
    let r = get(&srv, "/", None);
    assert_eq!(r.status, 200);
    assert!(r.head.contains("text/html"));
    assert!(String::from_utf8_lossy(&r.body).contains("<html"));
    let r = get(&srv, "/api/status", None);
    assert_eq!(r.status, 200);
    assert!(r.head.contains("Cache-Control: no-store"));
    assert_eq!(r.json()["version"], env!("CARGO_PKG_VERSION"));
    assert_eq!(get(&srv, "/api/nothing", None).status, 404);
    assert_eq!(get(&srv, "/../../etc/passwd", None).status, 404);
}

#[test]
fn writes_need_the_header_and_requests_need_a_good_host() {
    let srv = start();
    let r = send(srv.addr, b"POST /api/actions HTTP/1.1\r\nHost: 127.0.0.1\r\nContent-Length: 18\r\n\r\n{\"action\":\"check\"}");
    assert_eq!(r.status, 403);
    let r = send(srv.addr, b"GET /api/status HTTP/1.1\r\nHost: evil.example\r\n\r\n");
    assert_eq!(r.status, 403);
    assert!(srv.runner.seen().is_empty());
}

#[test]
fn an_action_runs_as_a_job_and_reports_its_result() {
    let srv = start();
    let r = write(&srv, "POST", "/api/actions", br#"{"action":"install","package":"demo","collection":"complex256"}"#, None);
    assert_eq!(r.status, 202, "{}", String::from_utf8_lossy(&r.body));
    let job = r.json()["job"].as_str().unwrap().to_string();
    let v = wait_done(&srv, &job);
    assert_eq!(v["result"], "OK: recorded");
    assert_eq!(v["label"], "Install demo");
    assert_eq!(srv.runner.seen(), vec![vec!["install", "demo", "complex256"]]);
    let r = write(&srv, "POST", "/api/actions", br#"{"action":"restart","package":"$(reboot)"}"#, None);
    assert_eq!(r.status, 400);
    assert_eq!(srv.runner.seen().len(), 1);
}

#[test]
fn busy_while_a_job_runs_or_another_process_holds_the_lock() {
    let srv = start_with(fixture_with_card(), RecordingRunner::holding());
    let r = write(&srv, "POST", "/api/actions", br#"{"action":"check"}"#, None);
    assert_eq!(r.status, 202);
    let first = r.json()["job"].clone();
    let r = write(&srv, "POST", "/api/actions", br#"{"action":"rescan"}"#, None);
    assert_eq!(r.status, 409);
    assert_eq!(r.json()["job"], first);
    assert_eq!(get(&srv, "/api/status", None).json()["job"], first);

    // GEM (another live process) holds the lock and no web job is running.
    let srv = start();
    std::fs::write(srv.app.layout.run("lock"), std::process::id().to_string()).unwrap();
    let r = write(&srv, "POST", "/api/actions", br#"{"action":"check"}"#, None);
    assert_eq!(r.status, 409);
    assert_eq!(r.json()["busy"], true);
    assert!(r.json()["job"].is_null());
    assert!(srv.runner.seen().is_empty());
}

#[test]
fn a_pin_guards_the_api() {
    let f = fixture_with_card();
    let mut s = WebSettings::default();
    s.pin = Some(hash_pin("2468"));
    mibpkg::web::settings::save(&f.layout, &s).unwrap();
    let srv = start_with(f, RecordingRunner::default());
    assert_eq!(get(&srv, "/api/status", None).status, 401);
    assert_eq!(get(&srv, "/api/session", None).json()["pin_required"], true);
    assert_eq!(get(&srv, "/", None).status, 200, "the page itself loads (it shows the PIN pad)");
    let r = write(&srv, "POST", "/api/login", br#"{"pin":"1111"}"#, None);
    assert_eq!(r.status, 401);
    let r = write(&srv, "POST", "/api/login", br#"{"pin":"2468"}"#, None);
    assert_eq!(r.status, 200);
    let cookie_line = r.head.lines().find(|l| l.starts_with("Set-Cookie:")).unwrap().to_string();
    assert!(cookie_line.contains("HttpOnly") && cookie_line.contains("SameSite=Strict"), "{cookie_line}");
    let cookie = cookie_line.trim_start_matches("Set-Cookie: ").split(';').next().unwrap().to_string();
    assert_eq!(get(&srv, "/api/status", Some(&cookie)).status, 200);
    assert_eq!(get(&srv, "/api/session", Some(&cookie)).json()["authed"], true);
    assert_eq!(get(&srv, "/api/status", Some("mibpkg_session=forged")).status, 401);
}

#[test]
fn wrong_pins_lock_logins_out_with_doubling_delays() {
    let auth = Auth::default();
    let stored = hash_pin("2468");
    let t = Instant::now();
    for _ in 0..4 {
        assert!(matches!(auth.login(Some(&stored), "0000", t), Err(LoginError::Wrong)));
    }
    assert!(matches!(auth.login(Some(&stored), "0000", t), Err(LoginError::Wrong)));
    match auth.login(Some(&stored), "2468", t + Duration::from_secs(1)) {
        Err(LoginError::Locked(d)) => assert!(d <= Duration::from_secs(60) && d > Duration::from_secs(50)),
        other => panic!("{other:?}"),
    }
    assert!(matches!(auth.login(Some(&stored), "0000", t + Duration::from_secs(61)), Err(LoginError::Wrong)));
    match auth.login(Some(&stored), "2468", t + Duration::from_secs(62)) {
        Err(LoginError::Locked(d)) => assert!(d > Duration::from_secs(100), "{d:?}"),
        other => panic!("{other:?}"),
    }
    let token = auth.login(Some(&stored), "2468", t + Duration::from_secs(300)).unwrap();
    assert!(auth.check(Some(&format!("a=b; mibpkg_session={token}")), t + Duration::from_secs(301)));
    assert!(!auth.check(Some(&format!("mibpkg_session={token}")), t + Duration::from_secs(301 + 8 * 86400)));
}

#[test]
fn upload_inspect_and_install_builds_the_install_file_argv() {
    let srv = start();
    let f = &srv._f;
    let src = common::pkg_src(f.root(), "fresh", "1.0.0", "sleep 1", "");
    let out = f.root().join("fresh.mpk");
    mibpkg::archive::pack(&src, &out).unwrap();
    let bytes = std::fs::read(&out).unwrap();
    let mut raw = format!("POST /api/uploads HTTP/1.1\r\nHost: 127.0.0.1\r\nX-Mibpkg: 1\r\nContent-Type: application/octet-stream\r\nContent-Length: {}\r\n\r\n", bytes.len()).into_bytes();
    raw.extend_from_slice(&bytes);
    let r = send(srv.addr, &raw);
    assert_eq!(r.status, 200, "{}", String::from_utf8_lossy(&r.body));
    let v = r.json();
    assert_eq!(v["name"], "fresh");
    assert_eq!(v["preselected"], "-");
    let id = v["upload_id"].as_str().unwrap().to_string();
    let r = write(&srv, "POST", &format!("/api/uploads/{id}/install"), br#"{"collection":"elsewhere"}"#, None);
    assert_eq!(r.status, 400);
    let r = write(&srv, "POST", &format!("/api/uploads/{id}/install"), br#"{"collection":"-"}"#, None);
    assert_eq!(r.status, 202, "{}", String::from_utf8_lossy(&r.body));
    wait_done(&srv, r.json()["job"].as_str().unwrap());
    assert_eq!(srv.runner.seen(), vec![vec!["install-file".to_string(), "fresh".into(), id.clone(), "-".into()]]);
    let r = write(&srv, "POST", "/api/uploads/0123456789abcdef0123456789abcdef/install", br#"{"collection":"-"}"#, None);
    assert_eq!(r.status, 404);
}

#[test]
fn settings_refuse_to_lock_you_out_unless_confirmed() {
    let srv = start();
    // Loopback is always allowed, so any listen choice keeps this client in: save goes through.
    let r = write(&srv, "PUT", "/api/settings", br#"{"listen":["tailscale"],"port":8080}"#, None);
    assert_eq!(r.status, 200, "{}", String::from_utf8_lossy(&r.body));
    let argv = srv.runner.seen();
    assert_eq!(argv[0][0], "web-settings");
    std::thread::sleep(Duration::from_millis(300));
    assert_eq!(srv.exits.load(Ordering::SeqCst), 1, "the server restarts to apply them");
    let r = write(&srv, "PUT", "/api/settings", br#"{"listen":["any","hotspot"],"port":80}"#, None);
    assert_eq!(r.status, 400);
    let r = write(&srv, "PUT", "/api/settings", br#"{"listen":["hotspot"],"port":80,"pin":"12"}"#, None);
    assert_eq!(r.status, 400);
}

#[test]
fn too_many_connections_get_503() {
    let srv = start();
    let idle: Vec<TcpStream> = (0..6).map(|_| TcpStream::connect(srv.addr).unwrap()).collect();
    std::thread::sleep(Duration::from_millis(200));
    let r = get(&srv, "/api/status", None);
    assert_eq!(r.status, 503);
    drop(idle);
}

#[test]
fn mibpkg_web_runs_against_a_root() {
    let f = fixture_with_card();
    std::fs::create_dir_all(f.layout.home()).unwrap();
    std::fs::write(mibpkg::web::settings::path(&f.layout), "port = 0\n").unwrap();
    let mut child = std::process::Command::new(env!("CARGO_BIN_EXE_mibpkg"))
        .args(["--root", f.root().to_str().unwrap(), "web"])
        .stdout(std::process::Stdio::null())
        .spawn()
        .unwrap();
    let url_file = f.layout.run("web.url");
    let end = Instant::now() + Duration::from_secs(10);
    while !url_file.exists() && Instant::now() < end {
        std::thread::sleep(Duration::from_millis(50));
    }
    let text = std::fs::read_to_string(&url_file).unwrap();
    let port: u16 = text.lines().next().unwrap().trim_start_matches("port ").parse().unwrap();
    let r = send(format!("127.0.0.1:{port}").parse().unwrap(), b"GET /api/status HTTP/1.1\r\nHost: localhost\r\n\r\n");
    let _ = child.kill();
    let _ = child.wait();
    assert_eq!(r.status, 200);
}

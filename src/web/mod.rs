//! The web interface (spec 2026-10-05-web-interface-design.md): `mibpkg web`, started by the
//! supervisor. Reads are served in-process; every write runs `mibpkg gem <verb>` under the lock.
pub mod actions;
pub mod api;
pub mod assets;
pub mod auth;
pub mod http;
pub mod jobs;
pub mod net;
pub mod routes;
pub mod settings;
pub mod upload;

use crate::paths::Layout;
use crate::sys::{RealSys, Sys};
use crate::unit::UnitInfo;
use anyhow::{Context, Result};
use jobs::{Jobs, ProcessRunner, Runner};
use net::Allowed;
use settings::WebSettings;
use std::io::{BufReader, ErrorKind};
use std::net::{TcpListener, TcpStream};
use std::path::Path;
use std::sync::atomic::{AtomicBool, AtomicU16, AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::Duration;

/// Connections served at once; more get 503 (a phone opens a handful in parallel).
pub const MAX_CONNECTIONS: usize = 6;
/// How long a client may take to send a request (uploads get longer for their body).
const READ_TIMEOUT: Duration = Duration::from_secs(10);
const UPLOAD_TIMEOUT: Duration = Duration::from_secs(60);
const WRITE_TIMEOUT: Duration = Duration::from_secs(30);

/// Everything a request handler needs, shared by all connection threads.
pub struct App {
    pub layout: Layout,
    pub unit: UnitInfo,
    /// Read once at start: changing settings restarts the web process.
    pub settings: WebSettings,
    /// Why web.toml was ignored, if it was.
    pub settings_warning: Option<String>,
    pub sys: RealSys,
    pub allowed: Allowed,
    /// The port actually bound (0 until bound).
    pub port: AtomicU16,
    pub jobs: Jobs,
    pub uploads: upload::Uploads,
    pub auth: auth::Auth,
    pub runner: Box<dyn Runner>,
    /// Set by a settings save: after this response, ask the supervisor to restart us with them.
    pub restart_after_response: AtomicBool,
}

impl App {
    /// `probe` returns `ifconfig` output (`Allowed::ifconfig_probe()` on the unit).
    pub fn new(layout: Layout, probe: Box<dyn Fn() -> Option<String> + Send + Sync>) -> App {
        let (settings, settings_warning) = settings::load(&layout);
        let unit = crate::unit::detect(&layout);
        let allowed = Allowed::new(&settings, probe);
        let sys = RealSys { mounts: layout.root == Path::new("/") };
        // The running binary runs the actions (on the unit: /tmp/mibpkg, like GEM's gem.sh).
        let exe = std::env::current_exe().unwrap_or_else(|_| layout.tmp_bin());
        let runner = Box::new(ProcessRunner { exe, root: layout.root.clone(), log: Some(layout.run("log")) });
        App {
            unit,
            settings,
            settings_warning,
            sys,
            allowed,
            port: AtomicU16::new(0),
            jobs: Jobs::default(),
            uploads: upload::Uploads::default(),
            auth: auth::Auth::default(),
            runner,
            restart_after_response: AtomicBool::new(false),
            layout,
        }
    }

    pub fn with_runner(mut self, runner: Box<dyn Runner>) -> App {
        self.runner = runner;
        self
    }

    /// Whether a mibpkg action (from GEM, the CLI or this server) holds the action lock.
    pub fn lock_busy(&self) -> bool {
        std::fs::read_to_string(self.layout.run("lock"))
            .ok()
            .and_then(|s| s.trim().parse::<u32>().ok())
            .map_or(false, |pid| self.sys.process_alive(pid))
    }
}

/// Serve one connection: filter by local address, read, route, write, close.
fn connection(app: &App, stream: TcpStream) {
    let local = match stream.local_addr() {
        Ok(a) => a.ip(),
        Err(_) => return,
    };
    if !app.allowed.permits(local) {
        return; // closed without a word: this interface is not one the user chose
    }
    let _ = stream.set_read_timeout(Some(READ_TIMEOUT));
    let _ = stream.set_write_timeout(Some(WRITE_TIMEOUT));
    // Read and write through &TcpStream: TcpStream::try_clone needs F_DUPFD_CLOEXEC, which
    // QNX 6.5 rejects.
    let mut reader = BufReader::new(&stream);
    let mut writer = &stream;
    let limit = |method: &str, path: &str| -> u64 {
        if method == "POST" && path == "/api/uploads" {
            // A phone on Wi-Fi can take a while to send a few MB.
            let _ = stream.set_read_timeout(Some(UPLOAD_TIMEOUT));
            crate::fetch::MAX_PACKAGE
        } else {
            routes::MAX_BODY
        }
    };
    // Head first: refusals (host, header, PIN) happen before any body byte is read, and upload
    // bodies go straight to their /tmp file instead of into memory.
    let response = match http::read_head(&mut reader, limit) {
        Ok((mut req, length)) => {
            let gzip = req.header("accept-encoding").map_or(false, |a| a.contains("gzip"));
            if let Some(refused) = routes::precheck(app, &req) {
                Some((refused, gzip))
            } else if req.method == "POST" && req.path == "/api/uploads" {
                let _ = http::send_continue(&req, &mut writer);
                Some((routes::upload_streaming(app, &req, length, &mut reader), gzip))
            } else {
                match http::read_body(&mut reader, &mut writer, &mut req, length) {
                    Ok(()) => Some((routes::handle(app, &req, local), gzip)),
                    Err(_) => None,
                }
            }
        }
        Err(http::ReadError::TooLarge) => Some((http::Response::error(413, "request too large"), false)),
        Err(http::ReadError::LengthRequired) => Some((http::Response::error(411, "Content-Length required"), false)),
        Err(http::ReadError::BadRequest(why)) => Some((http::Response::error(400, why), false)),
        Err(http::ReadError::Io(_)) => None,
    };
    if let Some((r, gzip)) = response {
        let _ = http::write_response(&mut writer, r, gzip);
    }
    if app.restart_after_response.swap(false, Ordering::SeqCst) {
        // Give the browser a moment to read the reply, then have the supervisor restart us with
        // the new settings. A requested restart starts a fresh service, so saving settings often
        // never looks like a crash loop (exiting would count toward "failed").
        std::thread::sleep(Duration::from_millis(200));
        if let Err(e) = crate::control::request_restart(&app.layout, crate::cli::WEB_SERVICE) {
            eprintln!("mibpkg web: requesting a restart: {e:#}");
        }
    }
}

/// Accept connections forever, one thread each, at most `MAX_CONNECTIONS` at once.
pub fn serve(app: Arc<App>, listener: TcpListener) {
    let active = Arc::new(AtomicUsize::new(0));
    for stream in listener.incoming() {
        let Ok(stream) = stream else { continue };
        app.jobs.sweep();
        app.uploads.sweep(&app.layout);
        if active.load(Ordering::SeqCst) >= MAX_CONNECTIONS {
            let _ = stream.set_write_timeout(Some(Duration::from_secs(2)));
            let mut w = &stream;
            let _ = http::write_response(&mut w, http::Response::error(503, "too many connections; try again"), false);
            continue;
        }
        active.fetch_add(1, Ordering::SeqCst);
        let (app, active) = (app.clone(), active.clone());
        std::thread::spawn(move || {
            connection(&app, stream);
            active.fetch_sub(1, Ordering::SeqCst);
        });
    }
}

/// Write /tmp/mibpkg.web.url: "port N", then "<class> <url>" lines (for GEM and `status`).
fn write_url_file(app: &App) {
    let port = app.port.load(Ordering::Relaxed);
    let mut text = format!("port {port}\n");
    for (class, url) in app.allowed.urls(port) {
        text.push_str(&format!("{class} {url}\n"));
    }
    if std::fs::read_to_string(app.layout.run("web.url")).ok().as_deref() != Some(text.as_str()) {
        let _ = crate::fsutil::atomic_write(&app.layout.run("web.url"), text.as_bytes());
    }
}

/// `mibpkg web`: bind, then serve until killed.
pub fn run(layout: Layout) -> Result<()> {
    let app = App::new(layout, Allowed::ifconfig_probe());
    if let Some(w) = &app.settings_warning {
        eprintln!("mibpkg web: {w}");
    }
    if !app.settings.enabled {
        println!("mibpkg web: disabled in web.toml");
        return Ok(());
    }
    let port = app.settings.port;
    let listener = match TcpListener::bind(("0.0.0.0", port)) {
        Err(e) if e.kind() == ErrorKind::AddrInUse && port == 80 => {
            eprintln!("mibpkg web: port 80 is in use; using 8080");
            TcpListener::bind(("0.0.0.0", 8080)).context("binding port 8080")?
        }
        r => r.with_context(|| format!("binding port {port}"))?,
    };
    app.port.store(listener.local_addr()?.port(), Ordering::SeqCst);
    app.uploads.sweep(&app.layout);
    jobs::sweep_leftovers(&app.layout);
    println!("mibpkg web: listening on port {}", app.port.load(Ordering::SeqCst));
    let app = Arc::new(app);
    write_url_file(&app);
    // Interfaces come and go (Tailscale, the hotspot): keep the URL file current.
    let a = app.clone();
    std::thread::spawn(move || loop {
        std::thread::sleep(Duration::from_secs(30));
        write_url_file(&a);
    });
    serve(app, listener);
    Ok(())
}

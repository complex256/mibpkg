//! URL → handler, with the checks every request passes first: Host, the X-Mibpkg header on
//! writes, and the PIN session when a PIN is set.
use super::actions::{parse_action, validate, Known};
use super::api;
use super::auth::{host_ok, session_cookie, LoginError};
use super::http::{Request, Response};
use super::jobs::StartError;
use super::settings::{hash_pin, random_id, valid_pin};
use super::App;
use serde::Deserialize;
use serde_json::json;
use std::net::IpAddr;
use std::sync::atomic::Ordering;
use std::time::{Duration, Instant};

/// Largest request body except uploads.
pub const MAX_BODY: u64 = 64 * 1024;
/// How long a settings save may take before the request gives up waiting.
const SETTINGS_WAIT: Duration = Duration::from_secs(30);

pub fn handle(app: &App, req: &Request, local: IpAddr) -> Response {
    if let Some(refused) = precheck(app, req) {
        return refused;
    }
    if !req.path.starts_with("/api/") {
        return static_file(req);
    }
    let r = api_route(app, req, local);
    r.with_header("Cache-Control", "no-store")
}

/// The checks every request passes, using only its head (so they run before any body is read):
/// Host, the X-Mibpkg header on API writes, and the PIN session. Some(reply) = refused.
pub fn precheck(app: &App, req: &Request) -> Option<Response> {
    if !host_ok(req.header("host")) {
        return Some(Response::error(403, "the address must be the car's IP address"));
    }
    if !req.path.starts_with("/api/") {
        return None;
    }
    if req.method != "GET" && req.header("x-mibpkg") != Some("1") {
        return Some(Response::error(403, "missing X-Mibpkg header").with_header("Cache-Control", "no-store"));
    }
    let open = req.path == "/api/session" || req.path == "/api/login";
    if app.settings.pin.is_some() && !open && !app.auth.check(req.header("cookie"), Instant::now()) {
        return Some(Response::error(401, "PIN required").with_header("Cache-Control", "no-store"));
    }
    None
}

/// POST /api/uploads with the body still unread: stream it into the upload file, then inspect.
pub fn upload_streaming(app: &App, req: &Request, length: u64, r: &mut dyn std::io::Read) -> Response {
    let reply = match app.uploads.store_from(&app.layout, r, length) {
        Ok(id) => inspect_upload(app, &id),
        Err(super::upload::StoreError::Busy) => Response::json(409, &json!({ "busy": true, "error": "another upload is in progress" })),
        Err(super::upload::StoreError::Io(e)) => Response::error(400, format!("the upload did not arrive whole: {e}")),
    };
    let _ = req;
    reply.with_header("Cache-Control", "no-store")
}

/// GET of an embedded front-end file. The ETag is the mibpkg version: files change only with it.
fn static_file(req: &Request) -> Response {
    if req.method != "GET" {
        return Response::error(405, "method not allowed");
    }
    let Some((ctype, bytes)) = super::assets::get(&req.path) else {
        return Response::error(404, "not found");
    };
    let etag = format!("\"{}\"", env!("CARGO_PKG_VERSION"));
    if req.header("if-none-match") == Some(etag.as_str()) {
        return Response::new(304, ctype, Vec::new()).with_header("ETag", etag);
    }
    Response::new(200, ctype, bytes.into_owned()).with_header("ETag", etag).with_header("Cache-Control", "no-cache")
}

/// An API request that passed `precheck`.
fn api_route(app: &App, req: &Request, local: IpAddr) -> Response {
    let seg: Vec<&str> = req.path.trim_start_matches("/api/").split('/').collect();
    let cookie = req.header("cookie");

    match (req.method.as_str(), seg.as_slice()) {
        ("GET", ["session"]) => Response::json(
            200,
            &json!({
                "pin_required": app.settings.pin.is_some(),
                "authed": app.settings.pin.is_none() || app.auth.check(cookie, Instant::now()),
            }),
        ),
        ("POST", ["login"]) => login(app, req),
        ("POST", ["logout"]) => {
            app.auth.logout(cookie);
            Response::json(200, &json!({ "ok": true }))
        }
        ("GET", ["status"]) => with_job(api::status(app), app),
        ("GET", ["packages"]) => api::packages(app),
        ("GET", ["logs"]) => api::logs(app),
        ("GET", ["logs", id]) => {
            let bytes = req.query("bytes").and_then(|b| b.parse().ok()).unwrap_or(64 * 1024);
            api::log_tail(app, id, bytes)
        }
        ("GET", ["packages", name, "config", n]) => match n.parse() {
            Ok(n) => api::config_get(app, name, n),
            Err(_) => Response::error(404, "no such config"),
        },
        ("PUT", ["packages", name, "config", n]) => config_set(app, req, name, n),
        ("GET", ["sources"]) => api::sources(app),
        ("POST", ["sources"]) | ("DELETE", ["sources"]) => source_change(app, req),
        ("GET", ["settings"]) => api::settings(app),
        ("PUT", ["settings"]) => settings_save(app, req, local),
        ("POST", ["actions"]) => action(app, req),
        ("GET", ["jobs", id]) => match app.jobs.get(id) {
            Some(job) => Response::json(200, &job.json(req.query("since").and_then(|s| s.parse().ok()).unwrap_or(0))),
            None => Response::error(404, "no such job (finished jobs are kept 10 minutes)"),
        },
        ("POST", ["uploads"]) => upload(app, req),
        ("POST", ["uploads", id, "install"]) => upload_install(app, req, id),
        ("DELETE", ["uploads", id]) => {
            app.uploads.discard(id);
            Response::json(200, &json!({ "ok": true }))
        }
        (_, ["session" | "login" | "logout" | "status" | "packages" | "logs" | "sources" | "settings" | "actions" | "jobs" | "uploads", ..]) => {
            Response::error(405, "method not allowed")
        }
        _ => Response::error(404, "not found"),
    }
}

/// Status plus the id of the running web job, so a reloaded page can re-attach to it.
fn with_job(mut r: Response, app: &App) -> Response {
    if let Ok(mut v) = serde_json::from_slice::<serde_json::Value>(&r.body) {
        v["job"] = app.jobs.running().map(|j| json!(j.id)).unwrap_or(serde_json::Value::Null);
        r.body = serde_json::to_vec(&v).unwrap_or(r.body);
    }
    r
}

fn json_body<T: for<'de> Deserialize<'de>>(req: &Request) -> Result<T, Response> {
    serde_json::from_slice(&req.body).map_err(|e| Response::error(400, format!("bad request body: {e}")))
}

/// Start a job unless something is already running (a web job, or GEM holding the lock).
fn try_start(app: &App, label: String, package: Option<String>, argv: Vec<String>) -> Result<std::sync::Arc<super::jobs::Job>, Response> {
    if let Some(j) = app.jobs.running() {
        return Err(Response::json(409, &json!({ "busy": true, "job": j.id, "error": format!("busy: {} is running", j.label) })));
    }
    if app.lock_busy() {
        return Err(Response::json(409, &json!({ "busy": true, "job": null, "error": "busy: another mibpkg action is running (GEM?)" })));
    }
    app.jobs.start(app.runner.as_ref(), label, package, argv).map_err(|e| match e {
        StartError::Busy(j) => Response::json(409, &json!({ "busy": true, "job": j.map(|j| j.id.clone()), "error": "busy" })),
        StartError::Spawn(e) => Response::error(500, format!("could not start mibpkg: {e}")),
    })
}

/// `try_start` as a reply: 202 with the job id the browser follows.
fn start_job(app: &App, label: String, package: Option<String>, argv: Vec<String>) -> Response {
    match try_start(app, label, package, argv) {
        Ok(job) => Response::json(202, &json!({ "job": job.id })),
        Err(r) => r,
    }
}

fn login(app: &App, req: &Request) -> Response {
    #[derive(Deserialize)]
    struct Body {
        pin: String,
    }
    let b: Body = match json_body(req) {
        Ok(b) => b,
        Err(r) => return r,
    };
    match app.auth.login(app.settings.pin.as_deref(), &b.pin, Instant::now()) {
        Ok(token) => Response::json(200, &json!({ "ok": true })).with_header("Set-Cookie", session_cookie(&token)),
        Err(LoginError::Wrong) => Response::error(401, "wrong PIN"),
        Err(LoginError::Locked(d)) => Response::json(429, &json!({ "error": "too many wrong PINs; wait and try again", "retry_after": d.as_secs() + 1 })),
    }
}

fn action(app: &App, req: &Request) -> Response {
    let a = match parse_action(&req.body) {
        Ok(a) => a,
        Err(e) => return Response::error(400, e),
    };
    match validate(&a, &Known::load(&app.layout)) {
        Ok(v) => start_job(app, v.label, v.package, v.argv),
        Err(e) => Response::error(400, e),
    }
}

/// Write `text` to /tmp/mibpkg.text.<id> for a verb to pick up (contents never go in argv).
fn text_file(app: &App, text: &[u8]) -> Result<String, Response> {
    let id = random_id();
    std::fs::write(app.layout.run(&format!("text.{id}")), text).map_err(|e| Response::error(500, format!("writing to /tmp: {e}")))?;
    Ok(id)
}

fn config_set(app: &App, req: &Request, name: &str, n: &str) -> Response {
    let known = Known::load(&app.layout);
    let n: usize = match n.parse() {
        Ok(n) => n,
        Err(_) => return Response::error(404, "no such config"),
    };
    if !crate::manifest::valid_name(name) || !known.installed.contains(name) {
        return Response::error(404, format!("{name} is not installed"));
    }
    let path = match crate::install::current_manifest(&app.layout, name).ok().and_then(|m| m.configs.get(n).map(|c| c.path.clone())) {
        Some(p) => p,
        None => return Response::error(404, format!("{name} has no config number {n}")),
    };
    if std::str::from_utf8(&req.body).is_err() {
        return Response::error(400, "config files must be text (UTF-8)");
    }
    let id = match text_file(app, &req.body) {
        Ok(id) => id,
        Err(r) => return r,
    };
    let r = start_job(app, format!("Save {path} for {name}"), Some(name.to_string()), vec!["config-set".into(), name.into(), n.to_string(), id.clone()]);
    if r.status != 202 {
        let _ = std::fs::remove_file(app.layout.run(&format!("text.{id}")));
    }
    r
}

fn source_change(app: &App, req: &Request) -> Response {
    #[derive(Deserialize)]
    #[serde(deny_unknown_fields)]
    struct Body {
        source: String,
    }
    let b: Body = match json_body(req) {
        Ok(b) => b,
        Err(r) => return r,
    };
    if let Err(e) = crate::online::index_url(&b.source) {
        return Response::error(400, format!("{e:#}"));
    }
    if req.method == "POST" {
        start_job(app, format!("Add source {}", b.source), None, vec!["source-add".into(), b.source])
    } else {
        if !crate::online::effective_sources(&app.layout).iter().any(|s| s.source == b.source) {
            return Response::error(404, format!("{} is not a source", b.source));
        }
        start_job(app, format!("Remove source {}", b.source), None, vec!["source-remove".into(), b.source])
    }
}

fn settings_save(app: &App, req: &Request, local: IpAddr) -> Response {
    #[derive(Deserialize)]
    #[serde(deny_unknown_fields)]
    struct Body {
        listen: Vec<String>,
        port: u16,
        #[serde(default)]
        pin: Option<String>,
        #[serde(default)]
        clear_pin: bool,
        #[serde(default)]
        confirm: bool,
    }
    let b: Body = match json_body(req) {
        Ok(b) => b,
        Err(r) => return r,
    };
    let mut s = app.settings.clone();
    s.listen = b.listen;
    s.port = b.port;
    if b.port == 0 {
        return Response::error(400, "choose a port from 1 to 65535");
    }
    if b.clear_pin {
        s.pin = None;
    }
    if let Some(pin) = &b.pin {
        if !valid_pin(pin) {
            return Response::error(400, "a PIN is 4 to 12 digits");
        }
        s.pin = Some(hash_pin(pin));
    }
    if let Err(e) = s.validate() {
        return Response::error(400, format!("{e:#}"));
    }
    if !b.confirm && !app.allowed.would_permit(local, &s) {
        return Response::json(
            409,
            &json!({ "needs_confirm": true, "error": "these settings would stop this connection from working; confirm to save anyway" }),
        );
    }
    let text = match serde_json::to_vec(&s) {
        Ok(t) => t,
        Err(e) => return Response::error(500, e.to_string()),
    };
    let id = match text_file(app, &text) {
        Ok(id) => id,
        Err(r) => return r,
    };
    let job = match try_start(app, "Save web settings".into(), None, vec!["web-settings".into(), id.clone()]) {
        Ok(j) => j,
        Err(r) => {
            let _ = std::fs::remove_file(app.layout.run(&format!("text.{id}")));
            return r;
        }
    };
    // Saving takes a moment (it needs the lock and a remount); wait so the reply says how it went.
    let end = Instant::now() + SETTINGS_WAIT;
    while !job.done() && Instant::now() < end {
        std::thread::sleep(Duration::from_millis(50));
    }
    let v = job.json(0);
    if v["ok"] != true {
        return Response::json(500, &json!({ "error": v["result"].as_str().unwrap_or("saving the settings did not finish"), "job": v }));
    }
    app.restart_after_response.store(true, Ordering::SeqCst);
    let port = s.port;
    let urls: Vec<_> = app.allowed.urls(port).into_iter().map(|(class, url)| json!({ "class": class, "url": url })).collect();
    Response::json(200, &json!({ "ok": true, "restarting": true, "urls": urls, "port": port }))
}

fn upload(app: &App, req: &Request) -> Response {
    let mut body = &req.body[..];
    upload_streaming(app, req, req.body.len() as u64, &mut body)
}

fn inspect_upload(app: &App, id: &str) -> Response {
    let id = id.to_string();
    let Some(path) = app.uploads.path(&id) else {
        return Response::error(500, "the upload disappeared");
    };
    let colls = crate::catalog::collections(&app.layout);
    Response::json(200, &super::upload::inspect(&app.layout, &app.unit, &colls, &id, &path))
}

fn upload_install(app: &App, req: &Request, id: &str) -> Response {
    #[derive(Deserialize)]
    #[serde(deny_unknown_fields)]
    struct Body {
        collection: String,
    }
    let Some(path) = app.uploads.path(id) else {
        return Response::error(404, "that upload is gone (uploads expire after 10 minutes); upload the file again");
    };
    let b: Body = match json_body(req) {
        Ok(b) => b,
        Err(r) => return r,
    };
    let colls = crate::catalog::collections(&app.layout);
    let i = super::upload::inspect(&app.layout, &app.unit, &colls, id, &path);
    let Some(name) = i.name.clone() else {
        return Response::error(400, i.problems.join("; "));
    };
    if !i.problems.is_empty() {
        return Response::error(400, i.problems.join("; "));
    }
    // An upgrade keeps its recorded collection (the verb ignores this argument then).
    let coll = if i.kind.as_deref() == Some("upgrade") {
        "-".to_string()
    } else if b.collection == "-" || colls.iter().any(|c| c.id() == b.collection) {
        b.collection
    } else {
        return Response::error(400, format!("{:?} is not a known collection", b.collection));
    };
    let verb = if i.kind.as_deref() == Some("upgrade") { "Update" } else { "Install" };
    start_job(app, format!("{verb} {name} from file"), Some(name.clone()), vec!["install-file".into(), name, id.to_string(), coll])
}

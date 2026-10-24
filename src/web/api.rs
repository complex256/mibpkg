//! Read-only JSON handlers. Nothing here writes to flash or takes the action lock.
use super::http::Response;
use super::App;
use crate::catalog;
use crate::install::{current_dir, current_manifest, updates_available};
use crate::state::State;
use serde_json::json;
use std::fs;
use std::io::Read;
use std::sync::atomic::Ordering;

/// Largest log tail and config file the API returns.
pub const MAX_TAIL: u64 = 256 * 1024;
pub const MAX_CONFIG: u64 = 64 * 1024;

fn read_text(path: &std::path::Path) -> Option<String> {
    fs::read_to_string(path).ok().map(|s| s.trim().to_string()).filter(|s| !s.is_empty())
}

/// GET /api/status
pub fn status(app: &App) -> Response {
    let layout = &app.layout;
    let state = State::load(layout).unwrap_or_default();
    let sup = crate::status::read(layout);
    let running = crate::control::supervisor_pid(layout, &app.sys).is_some();
    let updates = updates_available(&state, &catalog::collections(layout), &app.unit).len();
    Response::json(
        200,
        &json!({
            "version": env!("CARGO_PKG_VERSION"),
            "unit": app.unit.describe(),
            "supervisor": { "running": running, "services": sup.as_ref().map_or(0, |s| s.services.len()) },
            "disabled": crate::media::disabled(layout),
            "online": read_text(&layout.run("online")),
            "busy": app.lock_busy(),
            "updates": updates,
            "last_result": state.results.get("_"),
            "installed": state.packages.len(),
        }),
    )
}

/// GET /api/packages
pub fn packages(app: &App) -> Response {
    Response::json(200, &catalog::build(&app.layout, &app.unit))
}

/// The logs a user may read: mibpkg's own, the web server's, and every installed service's.
fn known_logs(app: &App) -> Vec<(String, String)> {
    let mut out = vec![("mibpkg".to_string(), "mibpkg".to_string()), (crate::cli::WEB_SERVICE.to_string(), "web server".to_string())];
    let state = State::load(&app.layout).unwrap_or_default();
    for inst in state.packages.values() {
        for id in &inst.services {
            out.push((id.clone(), id.clone()));
        }
    }
    out
}

/// GET /api/logs
pub fn logs(app: &App) -> Response {
    let list: Vec<_> = known_logs(app).into_iter().map(|(id, label)| json!({ "id": id, "label": label })).collect();
    Response::json(200, &list)
}

/// The last `want` bytes of `r`, starting at a line boundary. Reads strictly forward and never
/// seeks (lseek fails with EINVAL on the unit's QNX 6.5 std port), keeping only the newest `want`
/// bytes (plus one read chunk) in memory, so a huge log costs bounded memory.
pub fn tail_lines<R: Read>(mut r: R, want: u64) -> Vec<u8> {
    let want = want as usize;
    let mut buf: Vec<u8> = Vec::new();
    let mut chunk = [0u8; 16 * 1024];
    let mut dropped = false;
    loop {
        match r.read(&mut chunk) {
            Ok(0) => break,
            Ok(n) => {
                buf.extend_from_slice(&chunk[..n]);
                if buf.len() > want {
                    buf.drain(..buf.len() - want);
                    dropped = true;
                }
            }
            Err(e) if e.kind() == std::io::ErrorKind::Interrupted => {}
            Err(_) => break,
        }
    }
    // Started mid-line: drop the partial first line.
    if dropped {
        match buf.iter().position(|b| *b == b'\n') {
            Some(i) => {
                buf.drain(..=i);
            }
            None => buf.clear(),
        }
    }
    buf
}

/// GET /api/logs/<id>?bytes=N — the last N bytes, starting at a line boundary.
pub fn log_tail(app: &App, id: &str, bytes: u64) -> Response {
    if !known_logs(app).iter().any(|(k, _)| k == id) {
        return Response::error(404, format!("no log named {id:?}"));
    }
    let path = if id == "mibpkg" { app.layout.run("log") } else { app.layout.run(&format!("{id}.log")) };
    let want = bytes.clamp(1, MAX_TAIL);
    let text = fs::File::open(&path).map(|f| tail_lines(f, want)).unwrap_or_default();
    Response::new(200, "text/plain; charset=utf-8", String::from_utf8_lossy(&text).into_owned().into_bytes())
}

/// GET /api/packages/<name>/config/<n> — the file, or its example while it does not exist yet,
/// and the card's copy while the two are in conflict (`card`, else null).
pub fn config_get(app: &App, name: &str, n: usize) -> Response {
    let Ok(m) = current_manifest(&app.layout, name) else {
        return Response::error(404, format!("{name} is not installed"));
    };
    let Some(c) = m.configs.get(n) else {
        return Response::error(404, format!("{name} has no config number {n}"));
    };
    let path = app.layout.config_dir(name).join(&c.path);
    let exists = path.is_file();
    let read_capped = |p: &std::path::Path| -> String {
        let mut buf = Vec::new();
        if let Ok(f) = fs::File::open(p) {
            let _ = f.take(MAX_CONFIG).read_to_end(&mut buf);
        }
        String::from_utf8_lossy(&buf).into_owned()
    };
    let content = if exists {
        read_capped(&path)
    } else {
        match (&c.example, current_dir(&app.layout, name)) {
            (Some(e), Some(dir)) => read_capped(&dir.join(e)),
            _ => String::new(),
        }
    };
    // A conflict: the card's copy changed too and waits for a choice.
    let pending = crate::config::pending_path(&app.layout, name, &c.path);
    let card = pending.is_file().then(|| read_capped(&pending));
    Response::json(200, &json!({ "path": c.path, "required": c.required, "exists": exists, "content": content, "example": c.example, "card": card }))
}

/// GET /api/sources — enabled sources (with the collection their cached index holds and the last
/// check's error), sources that could be enabled (community list, SD card), and the list status.
pub fn sources(app: &App) -> Response {
    let layout = &app.layout;
    let cached = crate::online::collections(layout);
    let caches: Vec<crate::online::Cached> = fs::read_dir(layout.state_dir().join("online"))
        .map(|rd| rd.filter_map(|e| e.ok()).filter_map(|e| fs::read_to_string(e.path()).ok()).filter_map(|t| serde_json::from_str(&t).ok()).collect())
        .unwrap_or_default();
    let errors = crate::online::load_errors(layout);
    let sources: Vec<_> = crate::online::effective_sources(layout)
        .into_iter()
        .map(|s| {
            let c = caches.iter().find(|c| c.source == s.source);
            let packages = c.and_then(|c| cached.iter().find(|x| x.id() == c.index.collection.id)).map(|x| x.index.packages.len());
            json!({
                "source": s.source,
                "origin": s.origin,
                "enabled": s.enabled,
                "collection": c.map(|c| c.index.collection.id.clone()),
                "collection_name": c.map(|c| c.index.collection.name.clone()),
                "packages": packages,
                "error": errors.sources.get(&s.source),
            })
        })
        .collect();
    let list = crate::sourcelist::load_cache(layout);
    let card = crate::sourcelist::card_sources(layout);
    let available = crate::sourcelist::available(layout, list.as_ref().map(|l| l.entries.as_slice()).unwrap_or(&[]), &card.entries);
    let url = crate::sourcelist::list_url(&layout.settings.online_list).ok().flatten();
    Response::json(
        200,
        &json!({
            "sources": sources,
            "available": available,
            "card_skipped": card.skipped,
            "list": {
                "url": url,
                "fetched_at": list.as_ref().map(|l| l.fetched_at),
                "error": errors.list,
                "skipped": list.as_ref().map(|l| l.skipped.clone()).unwrap_or_default(),
            },
        }),
    )
}

/// GET /api/settings — never includes the PIN hash.
pub fn settings(app: &App) -> Response {
    let s = &app.settings;
    let urls: Vec<_> = app.allowed.urls(app.port.load(Ordering::Relaxed)).into_iter().map(|(class, url)| json!({ "class": class, "url": url })).collect();
    let up: Vec<_> = app.allowed.interfaces_up().into_iter().map(|(name, addr)| json!({ "name": name, "address": addr })).collect();
    Response::json(
        200,
        &json!({
            "enabled": s.enabled,
            "port": s.port,
            "bound_port": app.port.load(Ordering::Relaxed),
            "listen": s.listen,
            "pin_set": s.pin.is_some(),
            "interfaces": s.interfaces,
            "urls": urls,
            "interfaces_up": up,
            "warning": app.settings_warning,
        }),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{Cursor, Seek, SeekFrom};

    /// Like the unit's files: reads work, every seek fails.
    struct NoSeek<R>(R);
    impl<R: Read> Read for NoSeek<R> {
        fn read(&mut self, b: &mut [u8]) -> std::io::Result<usize> {
            self.0.read(b)
        }
    }
    impl<R> Seek for NoSeek<R> {
        fn seek(&mut self, _: SeekFrom) -> std::io::Result<u64> {
            Err(std::io::Error::from_raw_os_error(22))
        }
    }

    #[test]
    fn tail_short_log_is_whole() {
        assert_eq!(tail_lines(NoSeek(Cursor::new(b"a\nb\n".to_vec())), 100), b"a\nb\n");
    }

    #[test]
    fn tail_long_log_starts_at_a_line_and_is_bounded() {
        let log: String = (0..50_000).map(|i| format!("line {i}\n")).collect();
        let t = tail_lines(NoSeek(Cursor::new(log.clone().into_bytes())), 1000);
        assert!(t.len() <= 1000 && t.len() > 900);
        assert!(log.as_bytes().ends_with(&t));
        assert!(t.starts_with(b"line "));
        assert!(!log.is_empty() && t.ends_with(b"line 49999\n"));
    }

    #[test]
    fn tail_without_newline_is_empty_and_empty_log_is_empty() {
        assert!(tail_lines(NoSeek(Cursor::new(vec![b'x'; 5000])), 100).is_empty());
        assert!(tail_lines(NoSeek(Cursor::new(Vec::new())), 100).is_empty());
    }
}

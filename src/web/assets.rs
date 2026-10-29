//! The front end, embedded in the binary so what is in the repo's web/ directory is exactly what
//! the car serves. MIBPKG_WEB_DIR=<dir> serves files from disk instead (development).
//! Adding a file to web/ means adding a line here.
use std::borrow::Cow;

/// (URL path, file under web/, content type, embedded bytes)
const FILES: &[(&str, &str, &str, &[u8])] = &[
    ("/", "index.html", "text/html; charset=utf-8", include_bytes!("../../web/index.html")),
    ("/style.css", "style.css", "text/css; charset=utf-8", include_bytes!("../../web/style.css")),
    ("/js/api.js", "js/api.js", "text/javascript; charset=utf-8", include_bytes!("../../web/js/api.js")),
    ("/js/app.js", "js/app.js", "text/javascript; charset=utf-8", include_bytes!("../../web/js/app.js")),
    ("/js/dom.js", "js/dom.js", "text/javascript; charset=utf-8", include_bytes!("../../web/js/dom.js")),
    ("/js/icons.js", "js/icons.js", "text/javascript; charset=utf-8", include_bytes!("../../web/js/icons.js")),
    ("/js/jobs.js", "js/jobs.js", "text/javascript; charset=utf-8", include_bytes!("../../web/js/jobs.js")),
    ("/js/permissions.js", "js/permissions.js", "text/javascript; charset=utf-8", include_bytes!("../../web/js/permissions.js")),
    ("/js/router.js", "js/router.js", "text/javascript; charset=utf-8", include_bytes!("../../web/js/router.js")),
    ("/js/rows.js", "js/rows.js", "text/javascript; charset=utf-8", include_bytes!("../../web/js/rows.js")),
    ("/js/views/config.js", "js/views/config.js", "text/javascript; charset=utf-8", include_bytes!("../../web/js/views/config.js")),
    ("/js/views/discover.js", "js/views/discover.js", "text/javascript; charset=utf-8", include_bytes!("../../web/js/views/discover.js")),
    ("/js/views/installed.js", "js/views/installed.js", "text/javascript; charset=utf-8", include_bytes!("../../web/js/views/installed.js")),
    ("/js/views/login.js", "js/views/login.js", "text/javascript; charset=utf-8", include_bytes!("../../web/js/views/login.js")),
    ("/js/views/logs.js", "js/views/logs.js", "text/javascript; charset=utf-8", include_bytes!("../../web/js/views/logs.js")),
    ("/js/views/more.js", "js/views/more.js", "text/javascript; charset=utf-8", include_bytes!("../../web/js/views/more.js")),
    ("/js/views/package.js", "js/views/package.js", "text/javascript; charset=utf-8", include_bytes!("../../web/js/views/package.js")),
    ("/js/views/settings.js", "js/views/settings.js", "text/javascript; charset=utf-8", include_bytes!("../../web/js/views/settings.js")),
    ("/js/views/sources.js", "js/views/sources.js", "text/javascript; charset=utf-8", include_bytes!("../../web/js/views/sources.js")),
    ("/js/views/updates.js", "js/views/updates.js", "text/javascript; charset=utf-8", include_bytes!("../../web/js/views/updates.js")),
    ("/js/views/upload.js", "js/views/upload.js", "text/javascript; charset=utf-8", include_bytes!("../../web/js/views/upload.js")),
    ("/js/views/wifi.js", "js/views/wifi.js", "text/javascript; charset=utf-8", include_bytes!("../../web/js/views/wifi.js")),
];

/// The content type and bytes for a URL path, or None.
pub fn get(path: &str) -> Option<(&'static str, Cow<'static, [u8]>)> {
    let (_, file, ctype, bytes) = FILES.iter().find(|(p, _, _, _)| *p == path)?;
    if let Ok(dir) = std::env::var("MIBPKG_WEB_DIR") {
        if let Ok(disk) = std::fs::read(std::path::Path::new(&dir).join(file)) {
            return Some((ctype, Cow::Owned(disk)));
        }
    }
    Some((ctype, Cow::Borrowed(bytes)))
}

/// Every embedded URL path (tests check each one is served).
pub fn paths() -> impl Iterator<Item = &'static str> {
    FILES.iter().map(|(p, _, _, _)| *p)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::{Path, PathBuf};

    fn walk(dir: &Path, out: &mut Vec<PathBuf>) {
        for e in std::fs::read_dir(dir).unwrap() {
            let p = e.unwrap().path();
            if p.is_dir() {
                if p.file_name().and_then(|n| n.to_str()) != Some("tests") {
                    walk(&p, out);
                }
            } else {
                out.push(p);
            }
        }
    }

    #[test]
    fn every_web_file_is_embedded() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("web");
        let mut files = Vec::new();
        walk(&root, &mut files);
        let mut missing = Vec::new();
        for f in files {
            let rel = f.strip_prefix(&root).unwrap().to_string_lossy().replace('\\', "/");
            let url = if rel == "index.html" { "/".to_string() } else { format!("/{rel}") };
            if !FILES.iter().any(|(p, _, _, _)| *p == url) {
                missing.push(url);
            }
        }
        assert!(missing.is_empty(), "web files not embedded: {missing:?}");
    }
}

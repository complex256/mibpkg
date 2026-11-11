//! The front end, embedded in the binary so what is in the repo's web/dist directory is exactly
//! what the car serves. MIBPKG_WEB_DIR=<dir> serves files from disk instead (development).
//! web/dist is built by `npm --prefix web run build` (scripts/bundle.sh and CI run it too).
use std::borrow::Cow;

/// (URL path, file under web/dist, content type, embedded bytes)
const FILES: &[(&str, &str, &str, &[u8])] = &[
    ("/", "index.html", "text/html; charset=utf-8", include_bytes!("../../web/dist/index.html")),
    ("/style.css", "style.css", "text/css; charset=utf-8", include_bytes!("../../web/dist/style.css")),
    ("/app.js", "app.js", "text/javascript; charset=utf-8", include_bytes!("../../web/dist/app.js")),
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
                walk(&p, out);
            } else {
                out.push(p);
            }
        }
    }

    #[test]
    fn every_built_front_end_file_is_embedded() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("web/dist");
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
        assert!(missing.is_empty(), "web/dist files not embedded: {missing:?}");
    }
}

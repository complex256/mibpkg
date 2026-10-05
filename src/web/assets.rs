//! The front end, embedded in the binary so what is in the repo's web/ directory is exactly what
//! the car serves. MIBPKG_WEB_DIR=<dir> serves files from disk instead (development).
use std::borrow::Cow;

/// (URL path, file under web/, content type)
const FILES: &[(&str, &str, &str)] = &[("/", "index.html", "text/html; charset=utf-8")];

fn embedded(file: &str) -> Option<&'static [u8]> {
    match file {
        "index.html" => Some(include_bytes!("../../web/index.html")),
        _ => None,
    }
}

/// The content type and bytes for a URL path, or None.
pub fn get(path: &str) -> Option<(&'static str, Cow<'static, [u8]>)> {
    let (_, file, ctype) = FILES.iter().find(|(p, _, _)| *p == path)?;
    if let Ok(dir) = std::env::var("MIBPKG_WEB_DIR") {
        if let Ok(bytes) = std::fs::read(std::path::Path::new(&dir).join(file)) {
            return Some((ctype, Cow::Owned(bytes)));
        }
    }
    Some((ctype, Cow::Borrowed(embedded(file)?)))
}

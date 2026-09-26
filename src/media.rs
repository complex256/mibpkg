//! SD cards / USB sticks: collections to install from and the DISABLE marker.
use crate::index::Collection;
use crate::paths::Layout;
use anyhow::Result;
use std::fs;
use std::path::PathBuf;

pub fn roots(layout: &Layout) -> Vec<PathBuf> {
    layout.settings.media_roots.iter().map(|r| layout.p(r)).filter(|p| p.is_dir()).collect()
}

pub fn disabled(layout: &Layout) -> bool {
    roots(layout).iter().any(|r| r.join("mibpkg/DISABLE").exists())
}

pub fn scan(layout: &Layout) -> Vec<(PathBuf, Result<Collection>)> {
    let mut out = Vec::new();
    for r in roots(layout) {
        let mut dirs: Vec<PathBuf> = match fs::read_dir(r.join("mibpkg/collections")) {
            Ok(rd) => rd.filter_map(|e| e.ok()).map(|e| e.path()).filter(|p| p.is_dir()).collect(),
            Err(_) => continue,
        };
        dirs.sort();
        for d in dirs {
            let c = Collection::load(&d);
            out.push((d, c));
        }
    }
    out
}

pub fn loaded(scan: &[(PathBuf, Result<Collection>)]) -> Vec<Collection> {
    let mut out: Vec<Collection> = Vec::new();
    for (_, c) in scan {
        if let Ok(c) = c {
            if !out.iter().any(|o| o.id() == c.id()) {
                out.push(c.clone());
            }
        }
    }
    out
}

//! Talking to the supervisor through request files in /tmp (std-only: no signals).
use crate::fsutil::atomic_write;
use crate::paths::Layout;
use anyhow::Result;
use std::fs;
use std::path::Path;

fn touch(p: &Path) -> Result<()> {
    atomic_write(p, b"")?;
    Ok(())
}

pub fn request_reload(layout: &Layout) -> Result<()> {
    touch(&layout.run("reload"))
}
pub fn request_restart(layout: &Layout, id: &str) -> Result<()> {
    touch(&layout.run(&format!("restart.{id}")))
}
pub fn request_selfupdate(layout: &Layout) -> Result<()> {
    touch(&layout.run("selfupdate"))
}
pub fn request_stop(layout: &Layout) -> Result<()> {
    touch(&layout.run("stop"))
}

/// Consume a request file; true if it was there.
pub fn take(path: &Path) -> bool {
    fs::remove_file(path).is_ok()
}

/// Consume every restart request; returns the service ids.
pub fn take_restarts(layout: &Layout) -> Vec<String> {
    let dir = layout.p("/tmp");
    let mut ids = Vec::new();
    if let Ok(rd) = fs::read_dir(&dir) {
        for e in rd.filter_map(|e| e.ok()) {
            let n = e.file_name().to_string_lossy().to_string();
            if let Some(id) = n.strip_prefix("mibpkg.restart.") {
                if !id.ends_with(".tmp") && take(&e.path()) {
                    ids.push(id.to_string());
                }
            }
        }
    }
    ids.sort();
    ids
}

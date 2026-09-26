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

use crate::sys::Sys;
use std::process::{Command, Stdio};

/// Pid of a live supervisor, from /tmp/mibpkg.pid.
pub fn supervisor_pid(layout: &Layout, sys: &dyn Sys) -> Option<u32> {
    let pid: u32 = fs::read_to_string(layout.run("pid")).ok()?.trim().parse().ok()?;
    if sys.process_alive(pid) {
        Some(pid)
    } else {
        None
    }
}

/// Start `mibpkg supervise` in the background if none is running.
pub fn ensure_supervisor(layout: &Layout, sys: &dyn Sys) -> Result<()> {
    if supervisor_pid(layout, sys).is_some() {
        return Ok(());
    }
    let exe = std::env::current_exe().unwrap_or_else(|_| layout.tmp_bin());
    let log = fs::OpenOptions::new().create(true).append(true).open(layout.run("log"))?;
    Command::new(exe)
        .arg("--root")
        .arg(&layout.root)
        .arg("supervise")
        .stdin(Stdio::null())
        .stdout(log.try_clone()?)
        .stderr(log)
        .spawn()?;
    Ok(())
}

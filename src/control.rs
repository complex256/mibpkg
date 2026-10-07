//! Talking to the supervisor through request files in /tmp (std-only: no signals).
use crate::fsutil::atomic_write;
use crate::paths::Layout;
use anyhow::{Context, Result};
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
/// Ask the supervisor to restart through launch.sh with the new binary. Also marks the restart
/// as pending, so nothing else starts a supervisor (from an old binary) meanwhile.
pub fn request_selfupdate(layout: &Layout) -> Result<()> {
    touch(&layout.run("selfupdate.pending"))?;
    touch(&layout.run("selfupdate"))
}

/// How long a pending self-update holds off other supervisor starts. If launch.sh never starts
/// the new supervisor, the usual "make sure one runs" step takes over again after this.
const SELFUPDATE_GRACE: std::time::Duration = std::time::Duration::from_secs(60);

/// Whether a self-update restart is under way: launch.sh starts the next supervisor, from the new
/// binary. Starting one meanwhile would run the old binary and make launch.sh's exit (seen on the
/// unit 2026-10-07: the old supervisor kept running after a "successful" self-update).
pub fn supervisor_start_deferred(layout: &Layout) -> bool {
    fs::metadata(layout.run("selfupdate.pending"))
        .and_then(|m| m.modified())
        .ok()
        .and_then(|t| t.elapsed().ok())
        .map_or(false, |age| age < SELFUPDATE_GRACE)
}

/// The new supervisor is up: the self-update restart is complete.
pub fn selfupdate_done(layout: &Layout) {
    let _ = fs::remove_file(layout.run("selfupdate.pending"));
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
    if supervisor_pid(layout, sys).is_some() || supervisor_start_deferred(layout) {
        return Ok(());
    }
    let exe = std::env::current_exe().unwrap_or_else(|_| layout.tmp_bin());
    // Two opens, not File::try_clone: that needs fcntl(F_DUPFD_CLOEXEC), which QNX 6.5
    // rejects with EINVAL (seen on the unit, 2026-09-26).
    let open_log = || fs::OpenOptions::new().create(true).append(true).open(layout.run("log"));
    supervisor_command(&exe, &layout.root)
        .stdin(Stdio::null())
        .stdout(open_log().context("opening the supervisor log")?)
        .stderr(open_log().context("opening the supervisor log")?)
        .spawn()
        .context("spawning the supervisor")?;
    Ok(())
}

/// The command that runs `<exe> --root <root> supervise` in the background.
/// SIGHUP is ignored (and inherited by services): a supervisor started from an ssh
/// session must not die when the session closes.
pub fn supervisor_command(exe: &Path, root: &Path) -> Command {
    let mut c = Command::new("/bin/sh");
    c.arg("-c").arg("trap '' HUP; exec \"$0\" --root \"$1\" supervise").arg(exe).arg(root);
    c
}

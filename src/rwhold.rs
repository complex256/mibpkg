//! Read-write holds on `/mnt/app` and `/mnt/system` shared across processes.
//!
//! Several processes remount these read-write: the supervisor, a CLI action, GEM, and the SDK
//! tools. A process that remounts read-only when it is done would flip the mount under another
//! one still writing. So each process that holds a mount read-write leaves a holder file
//! `/tmp/mibpkg.rw.<tag>.<pid>`, and the last live holder to release is the one that goes
//! read-only. Holder files of dead processes are swept on release. The remount itself and the
//! holder bookkeeping run under the remount lock `/tmp/mibpkg.rwlock`.
//!
//! Holders are per pid, so a process keeps one `Ctx` (the supervisor's web threads share it);
//! nested guards within a process are refcounted by `ctx::Mounts` and touch the registry once.
//! A tool outside this protocol can still remount read-only at any time, which is why writes to
//! `/mnt/app` go through `mibsdk::put`, which reasserts read-write before it writes.
use crate::lock::Lock;
use crate::paths::Layout;
use crate::sys::Sys;
use anyhow::{bail, Context, Result};
use std::fs;
use std::path::{Path, PathBuf};
use std::time::Duration;

/// How long a remount waits for another process's remount lock.
pub const RWLOCK_WAIT: Duration = Duration::from_secs(3);

/// The holder-file tag of a mount that mibpkg remounts.
pub fn tag(mount: &str) -> Result<&'static str> {
    match mount {
        "/mnt/app" => Ok("app"),
        "/mnt/system" => Ok("system"),
        _ => bail!("{mount} is not a mount mibpkg remounts read-write"),
    }
}

/// Register `holder` as a read-write holder of `mount`, then remount it read-write (always:
/// something outside the protocol may have flipped it read-only). A failed remount withdraws
/// the holder file.
pub fn acquire(layout: &Layout, sys: &dyn Sys, mount: &str, holder: u32) -> Result<()> {
    acquire_in(&run_dir(layout), sys, mount, holder)
}

/// Withdraw `holder` and every dead holder of `mount`; remount read-only only when no live
/// holder remains.
pub fn release(layout: &Layout, sys: &dyn Sys, mount: &str, holder: u32) -> Result<()> {
    release_in(&run_dir(layout), sys, mount, holder)
}

/// Remount `mount` read-write again without touching the holder files.
pub fn reassert(layout: &Layout, sys: &dyn Sys, mount: &str) -> Result<()> {
    reassert_in(&run_dir(layout), sys, mount)
}

/// The directory of mibpkg's `/tmp/mibpkg.*` run files.
pub(crate) fn run_dir(layout: &Layout) -> PathBuf {
    layout.p("/tmp")
}

pub(crate) fn acquire_in(run_dir: &Path, sys: &dyn Sys, mount: &str, holder: u32) -> Result<()> {
    let tag = tag(mount)?;
    let _lock = remount_lock(run_dir, sys)?;
    let file = holder_file(run_dir, tag, holder);
    fs::write(&file, "").with_context(|| format!("writing {}", file.display()))?;
    if let Err(e) = sys.remount(mount, true) {
        let _ = fs::remove_file(&file);
        return Err(e);
    }
    Ok(())
}

pub(crate) fn release_in(run_dir: &Path, sys: &dyn Sys, mount: &str, holder: u32) -> Result<()> {
    let tag = tag(mount)?;
    let _lock = remount_lock(run_dir, sys)?;
    match fs::remove_file(holder_file(run_dir, tag, holder)) {
        Err(e) if e.kind() != std::io::ErrorKind::NotFound => return Err(e.into()),
        _ => {}
    }
    let prefix = format!("mibpkg.rw.{tag}.");
    let mut live = false;
    for entry in fs::read_dir(run_dir).with_context(|| format!("reading {}", run_dir.display()))? {
        let entry = entry?;
        let name = entry.file_name();
        let Some(pid) = name.to_str().and_then(|n| n.strip_prefix(&prefix)).and_then(|p| p.parse::<u32>().ok()) else {
            continue;
        };
        if sys.process_alive(pid) {
            live = true;
        } else {
            let _ = fs::remove_file(entry.path());
        }
    }
    if live {
        return Ok(());
    }
    sys.remount(mount, false)
}

pub(crate) fn reassert_in(run_dir: &Path, sys: &dyn Sys, mount: &str) -> Result<()> {
    tag(mount)?;
    let _lock = remount_lock(run_dir, sys)?;
    sys.remount(mount, true)
}

fn remount_lock(run_dir: &Path, sys: &dyn Sys) -> Result<Lock> {
    Lock::acquire_at(&run_dir.join("mibpkg.rwlock"), sys, RWLOCK_WAIT)
}

fn holder_file(run_dir: &Path, tag: &str, holder: u32) -> PathBuf {
    run_dir.join(format!("mibpkg.rw.{tag}.{holder}"))
}

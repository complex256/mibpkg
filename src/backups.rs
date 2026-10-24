//! Once-only backups of stock files mibpkg edits, so uninstall can restore them.
use crate::fsutil::copy_atomic;
use crate::paths::Layout;
use anyhow::{Context, Result};
use std::os::unix::fs::PermissionsExt;
use std::path::PathBuf;

const ABSENT_SUFFIX: &str = ".absent";

pub fn backup_path(layout: &Layout, unit_abs: &str) -> PathBuf {
    layout.state_dir().join("backups").join(unit_abs.trim_start_matches('/').replace('/', "%"))
}

fn tombstone_path(layout: &Layout, unit_abs: &str) -> PathBuf {
    let mut s = backup_path(layout, unit_abs).into_os_string();
    s.push(ABSENT_SUFFIX);
    PathBuf::from(s)
}

/// True when the file did not exist before mibpkg first touched it (restore deletes it).
pub fn is_tombstone(layout: &Layout, unit_abs: &str) -> bool {
    tombstone_path(layout, unit_abs).exists()
}

/// True when a backup of either form (a copy or a tombstone) is recorded.
pub fn has_backup(layout: &Layout, unit_abs: &str) -> bool {
    backup_path(layout, unit_abs).exists() || is_tombstone(layout, unit_abs)
}

/// Drop the recorded backup and/or tombstone; nothing recorded is success.
pub fn forget(layout: &Layout, unit_abs: &str) -> Result<()> {
    for p in [backup_path(layout, unit_abs), tombstone_path(layout, unit_abs)] {
        match std::fs::remove_file(&p) {
            Ok(()) => {}
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            Err(e) => return Err(e).with_context(|| format!("removing {}", p.display())),
        }
    }
    Ok(())
}

/// Record the file's original state once (a tombstone is not refreshed if the file appears later;
/// `forget` first to re-record): a copy if it exists, an empty `.absent` tombstone if not.
pub fn backup_once(layout: &Layout, unit_abs: &str) -> Result<PathBuf> {
    let dst = backup_path(layout, unit_abs);
    if has_backup(layout, unit_abs) {
        return Ok(dst);
    }
    let src = layout.p(unit_abs);
    match std::fs::metadata(&src) {
        Ok(m) => copy_atomic(&src, &dst, m.permissions().mode() & 0o7777)?,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            let t = tombstone_path(layout, unit_abs);
            if let Some(dir) = t.parent() {
                std::fs::create_dir_all(dir).with_context(|| format!("creating {}", dir.display()))?;
            }
            crate::fsutil::atomic_write(&t, b"").with_context(|| format!("writing {}", t.display()))?;
        }
        Err(e) => return Err(e).with_context(|| format!("reading {}", src.display())),
    }
    Ok(dst)
}

/// Record the file as absent before mibpkg's edit (a tombstone), whether or not it exists now:
/// for a file another tool created that mibpkg adopts as its own (the SDK's DSITracer marker
/// when `mibsdk-dev` recorded it as absent). Once only, like `backup_once`: an existing record
/// is kept.
pub fn record_absent(layout: &Layout, unit_abs: &str) -> Result<()> {
    if has_backup(layout, unit_abs) {
        return Ok(());
    }
    let t = tombstone_path(layout, unit_abs);
    if let Some(dir) = t.parent() {
        std::fs::create_dir_all(dir).with_context(|| format!("creating {}", dir.display()))?;
    }
    crate::fsutil::atomic_write(&t, b"").with_context(|| format!("writing {}", t.display()))
}

/// Put the file back to its recorded original state, then forget the record. Clearing matters for
/// tombstones: a stale one would otherwise delete a file mibpkg never created. A target already
/// gone counts as restored; calling this with no record is an error.
pub fn restore(layout: &Layout, unit_abs: &str) -> Result<()> {
    let src = backup_path(layout, unit_abs);
    let dst = layout.p(unit_abs);
    if is_tombstone(layout, unit_abs) {
        match std::fs::remove_file(&dst) {
            Ok(()) => {}
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            Err(e) => return Err(e).with_context(|| format!("removing {}", dst.display())),
        }
        return forget(layout, unit_abs);
    }
    let mode = std::fs::metadata(&src).with_context(|| format!("no backup of {unit_abs}"))?.permissions().mode();
    copy_atomic(&src, &dst, mode & 0o7777)?;
    forget(layout, unit_abs)
}

/// Restore every backed-up stock file to the version mibpkg saved before touching it. Used by a
/// full uninstall (purge) so no stock file is left patched before the mibpkg files are deleted.
pub fn restore_all(layout: &Layout) -> Result<()> {
    let dir = layout.state_dir().join("backups");
    let rd = match std::fs::read_dir(&dir) {
        Ok(rd) => rd,
        Err(_) => return Ok(()), // no backups dir = nothing to restore
    };
    let mut errors = Vec::new();
    for e in rd.filter_map(|e| e.ok()) {
        let name = e.file_name().to_string_lossy().into_owned();
        if name.ends_with(".tmp") {
            // Leftover of an interrupted atomic write: not a backup, drop it.
            let _ = std::fs::remove_file(e.path());
            continue;
        }
        let name = name.strip_suffix(ABSENT_SUFFIX).unwrap_or(&name);
        // backup_path encodes '/' as '%'; decode it back to the unit-absolute path.
        let unit_abs = format!("/{}", name.replace('%', "/"));
        if let Err(err) = restore(layout, &unit_abs) {
            errors.push(format!("{unit_abs}: {err:#}"));
        }
    }
    if !errors.is_empty() {
        anyhow::bail!("could not restore {} file(s): {}", errors.len(), errors.join("; "));
    }
    Ok(())
}

//! Once-only backups of stock files mibpkg edits, so uninstall can restore them.
use crate::fsutil::copy_atomic;
use crate::paths::Layout;
use anyhow::{Context, Result};
use std::os::unix::fs::PermissionsExt;
use std::path::PathBuf;

pub fn backup_path(layout: &Layout, unit_abs: &str) -> PathBuf {
    layout.state_dir().join("backups").join(unit_abs.trim_start_matches('/').replace('/', "%"))
}

pub fn backup_once(layout: &Layout, unit_abs: &str) -> Result<PathBuf> {
    let dst = backup_path(layout, unit_abs);
    if !dst.exists() {
        let src = layout.p(unit_abs);
        let mode = std::fs::metadata(&src).with_context(|| format!("reading {}", src.display()))?.permissions().mode();
        copy_atomic(&src, &dst, mode & 0o7777)?;
    }
    Ok(dst)
}

pub fn restore(layout: &Layout, unit_abs: &str) -> Result<()> {
    let src = backup_path(layout, unit_abs);
    let dst = layout.p(unit_abs);
    let mode = std::fs::metadata(&src).with_context(|| format!("no backup of {unit_abs}"))?.permissions().mode();
    copy_atomic(&src, &dst, mode & 0o7777)?;
    Ok(())
}

/// Restore every backed-up stock file to the version mibpkg saved before touching it. Used by a
/// full uninstall (purge) so no stock file is left patched before the mibpkg files are deleted.
pub fn restore_all(layout: &Layout) -> Result<()> {
    let dir = layout.state_dir().join("backups");
    let rd = match std::fs::read_dir(&dir) {
        Ok(rd) => rd,
        Err(_) => return Ok(()), // no backups dir = nothing to restore
    };
    for e in rd.filter_map(|e| e.ok()) {
        // backup_path encodes '/' as '%'; decode it back to the unit-absolute path.
        let unit_abs = format!("/{}", e.file_name().to_string_lossy().replace('%', "/"));
        restore(layout, &unit_abs)?;
    }
    Ok(())
}

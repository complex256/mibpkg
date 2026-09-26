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

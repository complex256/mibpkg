//! The mibsdk plugin SDK on the unit: its paths, whether it is installed and running, and the
//! verified writes every change to its files goes through. Nothing else in mibpkg names an SDK
//! path.
pub mod stamp;

use crate::ctx::Ctx;
use crate::fsutil;
use crate::install;
use crate::manifest::SdkSpec;
use crate::paths::Layout;
use anyhow::{anyhow, Context, Result};
use std::fs;
use std::io::{self, Write};
use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};
use std::path::{Path, PathBuf};

/// SDK install root: sdk.properties, grants.properties, plugins/<id>.jar, ...
pub const BASE: &str = "/mnt/app/eso/hmi/lsd/mibsdk";
/// State that must survive without a remount: boot.state, the ledger, user toggles, plugin data.
pub const PERSIST: &str = "/mnt/persist/mibsdk";
pub const RAM: &str = "/ramdisk/mibsdk";
pub const LSD: &str = "/mnt/app/eso/hmi/lsd";
/// Where the SDK's own jars live (`[sdk] jars`).
pub const JARS: &str = "/mnt/app/eso/hmi/lsd/jars";
/// The bundle list the HMI starts from (stock mode 0777).
pub const BUNDLES: &str = "/mnt/app/eso/hmi/lsd/bundles_dsitracer_enabled.properties";
/// Present when DSITracer is activated on this unit.
pub const MARKER: &str = "/mnt/app/eso/hmi/lsd/dsitracer_activated";
pub const TRACER_JAR: &str = "/mnt/app/eso/hmi/lsd/DSITracer.jar";
pub const HMI_ROOT: &str = "/mnt/app/eso/hmi";
/// Its first `HMI = ...` line names the HMI build (`unit::hmi_version`).
pub const VERSION_INFO: &str = "/ifs/version_info.txt";
/// The SDK runtime's log; it exists once the runtime has started this boot.
pub const LOG: &str = "/tmp/mibsdk.log";
pub const SDK_PKG: &str = crate::manifest::SDK_PACKAGE;
/// boot.state after a (re)install: no unhealthy boots counted against the new runtime.
pub const BOOT_STATE_RESET: &str = "unhealthyBoots=0\nlastShutdownClean=true\nwrapBoots=0\n";

/// The mount whose writes may be undone by another process remounting it read-only.
const APP: &str = "/mnt/app";
/// The temp file `put` writes next to its target (same file system, so the rename is atomic).
const PUT_TMP: &str = ".mibpkg.tmp";

/// The installed SDK package: its version, `[sdk]` spec and version directory.
pub fn installed_sdk(layout: &Layout) -> Option<(String, SdkSpec, PathBuf)> {
    let dir = install::current_dir(layout, SDK_PKG)?;
    let m = install::current_manifest(layout, SDK_PKG).ok()?;
    Some((m.package.version, m.sdk?, dir))
}

/// Present while an SDK install or removal waits for the next reboot to take effect.
pub fn reboot_flag(layout: &Layout) -> PathBuf {
    layout.run("sdk-reboot")
}

/// The runtime started this boot (its log exists) and no change is waiting for a reboot.
pub fn sdk_running(layout: &Layout) -> bool {
    layout.p(LOG).exists() && !reboot_flag(layout).exists()
}

/// File operations `put` performs, so tests can stand in for a mount flipped read-only.
pub trait FileOps {
    /// Create or truncate `p`, write `b`, set `mode`, fsync.
    fn write(&self, p: &Path, b: &[u8], mode: u32) -> io::Result<()>;
    fn read(&self, p: &Path) -> io::Result<Vec<u8>>;
    fn rename(&self, a: &Path, b: &Path) -> io::Result<()>;
    fn remove(&self, p: &Path) -> io::Result<()>;
}

/// The real file system.
pub struct StdOps;

impl FileOps for StdOps {
    fn write(&self, p: &Path, b: &[u8], mode: u32) -> io::Result<()> {
        let mut f = fs::OpenOptions::new().write(true).create(true).truncate(true).mode(mode).open(p)?;
        f.write_all(b)?;
        // The umask may have narrowed the create mode.
        f.set_permissions(fs::Permissions::from_mode(mode))?;
        fsutil::sync_result(f.sync_all())
    }
    fn read(&self, p: &Path) -> io::Result<Vec<u8>> {
        fs::read(p)
    }
    fn rename(&self, a: &Path, b: &Path) -> io::Result<()> {
        fsutil::rename(a, b)
    }
    fn remove(&self, p: &Path) -> io::Result<()> {
        fs::remove_file(p)
    }
}

/// Write `bytes` to the on-unit path `dst_abs` with `mode`, judged by its result: write a temp
/// file next to the target, read it back, rename it over the target, read the target back.
/// Another process may remount /mnt/app read-only at any moment, and a rename there can "succeed"
/// without replacing the target (QNX `mv` exits 0), so any failure re-asserts read-write and
/// retries once. For a target under /mnt/app the caller holds `ctx.mounts.rw("/mnt/app")`.
pub fn put(ctx: &Ctx, dst_abs: &str, bytes: &[u8], mode: u32) -> Result<()> {
    put_with(ctx, dst_abs, bytes, mode, &StdOps)
}

/// `put` with the bytes of the local file `src` (read once: the bytes read are the bytes written).
pub fn put_file(ctx: &Ctx, dst_abs: &str, src: &Path, mode: u32) -> Result<()> {
    let bytes = fs::read(src).with_context(|| format!("reading {}", src.display()))?;
    put(ctx, dst_abs, &bytes, mode)
}

#[doc(hidden)]
pub fn put_with(ctx: &Ctx, dst_abs: &str, bytes: &[u8], mode: u32, ops: &dyn FileOps) -> Result<()> {
    let dst = ctx.layout.p(dst_abs);
    let dir = dst.parent().ok_or_else(|| anyhow!("{dst_abs} has no parent directory"))?;
    let tmp = dir.join(PUT_TMP);
    let on_app = dst_abs == APP || dst_abs.starts_with(&format!("{APP}/"));
    remove_if_present(ops, &tmp).with_context(|| format!("removing the stale {}", tmp.display()))?;
    if on_app {
        ctx.mounts.reassert(APP).with_context(|| format!("writing {dst_abs}"))?;
    }
    let first = match put_once(ops, &tmp, &dst, bytes, mode) {
        Ok(()) => return Ok(()),
        Err(e) => e,
    };
    let _ = remove_if_present(ops, &tmp);
    if on_app {
        ctx.mounts
            .reassert(APP)
            .with_context(|| format!("writing {dst_abs} failed ({first}); re-asserting read-write failed"))?;
    }
    let r = put_once(ops, &tmp, &dst, bytes, mode);
    let _ = remove_if_present(ops, &tmp);
    r.map_err(|e| anyhow!("failed to write {dst_abs}: {e} (and before the retry: {first})"))
}

/// One attempt; leaves the temp file for the caller to remove.
fn put_once(ops: &dyn FileOps, tmp: &Path, dst: &Path, bytes: &[u8], mode: u32) -> std::result::Result<(), String> {
    ops.write(tmp, bytes, mode).map_err(|e| format!("writing the temp file: {e}"))?;
    if ops.read(tmp).map_err(|e| format!("reading the temp file back: {e}"))? != bytes {
        return Err("the temp file read back differs".into());
    }
    ops.rename(tmp, dst).map_err(|e| format!("moving it into place: {e}"))?;
    if ops.read(dst).map_err(|e| format!("reading it back: {e}"))? != bytes {
        return Err("it read back differently after the move".into());
    }
    Ok(())
}

fn remove_if_present(ops: &dyn FileOps, p: &Path) -> io::Result<()> {
    match ops.remove(p) {
        Err(e) if e.kind() != io::ErrorKind::NotFound => Err(e),
        _ => Ok(()),
    }
}

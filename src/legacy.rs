//! One-time migration of software that was installed by hand before mibpkg:
//! stop it, keep its config, drop its startup.sh line, move its dir aside.
use crate::config;
use crate::ctx::Ctx;
use crate::hook::{edit_startup, remove_lines};
use anyhow::Result;
use std::fs;
use std::time::Duration;

pub struct Legacy {
    pub package: &'static str,
    pub dir: &'static str,
    /// Control script inside `dir`, run as `sh <control> stop`.
    pub control: &'static str,
    /// Text identifying its startup.sh hook line.
    pub hook_marker: &'static str,
    /// (file in `dir`, config path in the package)
    pub configs: &'static [(&'static str, &'static str)],
}

pub const KNOWN: &[Legacy] = &[Legacy {
    package: "carmqtt",
    dir: "/mnt/app/root/carmqtt",
    control: "carmqtt.sh",
    hook_marker: "/mnt/app/root/carmqtt/carmqtt.sh",
    configs: &[("carmqtt.toml", "carmqtt.toml")],
}];

pub fn migrate(ctx: &Ctx, pkg: &str) -> Result<Option<String>> {
    let l = match KNOWN.iter().find(|l| l.package == pkg) {
        Some(l) => l,
        None => return Ok(None),
    };
    let dir = ctx.layout.p(l.dir);
    if !dir.is_dir() {
        return Ok(None);
    }
    let _rw = ctx.mounts.rw("/mnt/app")?;
    let control = dir.join(l.control);
    if control.is_file() {
        if let Err(e) = ctx.sys.run_script(&control, &["stop"], &[], Duration::from_secs(30)) {
            crate::warn!("warning: stopping the old {pkg}: {e:#}");
        }
    }
    for (old, new) in l.configs {
        let src = dir.join(old);
        let dst = ctx.layout.config_dir(pkg).join(new);
        if src.is_file() && !dst.exists() {
            config::import_file(&ctx.layout, pkg, new, &src)?;
        }
    }
    edit_startup(ctx, |t| Ok(Some(remove_lines(t, l.hook_marker).0)))?;
    let aside = ctx.layout.p(&format!("{}.pre-mibpkg", l.dir));
    if aside.exists() {
        return Ok(Some(format!(
            "migrated the old {pkg} install; old files left in {} ({} already exists)",
            l.dir,
            ctx.layout.unit_path(&aside)
        )));
    }
    fs::rename(&dir, &aside)?;
    Ok(Some(format!("migrated the old {pkg} install (old files kept in {}.pre-mibpkg)", l.dir)))
}

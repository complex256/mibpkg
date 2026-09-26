//! The single startup.sh line that starts the mibpkg supervisor, and the
//! validated edit path shared with legacy-hook removal.
use crate::backups::backup_once;
use crate::ctx::Ctx;
use anyhow::{bail, Context, Result};
use std::fs;

pub const MARKER: &str = "/mnt/app/root/mibpkg/bin/launch.sh";
pub const HOOK: [&str; 2] = [
    "    ## mibpkg supervisor. Best effort: detached, no stdio, never blocks boot.\n",
    "    ( sleep 30; [ -x /mnt/app/root/mibpkg/bin/launch.sh ] && /mnt/app/root/mibpkg/bin/launch.sh ) </dev/null >/dev/null 2>&1 &\n",
];
const STARTUP_UNIT: &str = "/mnt/system/etc/boot/startup.sh";

pub fn add_hook(text: &str) -> Result<String> {
    if text.contains(MARKER) {
        return Ok(text.to_string());
    }
    let lines: Vec<&str> = text.split_inclusive('\n').collect();
    let anchor = lines
        .iter()
        .position(|l| l.trim_start().starts_with("inetd") && l.contains("/dev/null"))
        .context("startup.sh has no `inetd > /dev/null` line to anchor the mibpkg hook")?;
    let mut out = String::with_capacity(text.len() + 200);
    for (i, l) in lines.iter().enumerate() {
        out.push_str(l);
        if i == anchor {
            if !l.ends_with('\n') {
                out.push('\n');
            }
            for h in HOOK {
                out.push_str(h);
            }
        }
    }
    Ok(out)
}

/// Drop every line containing `marker`, and the `##` comment line directly above it.
pub fn remove_lines(text: &str, marker: &str) -> (String, bool) {
    let lines: Vec<&str> = text.split_inclusive('\n').collect();
    let mut drop = vec![false; lines.len()];
    for (i, l) in lines.iter().enumerate() {
        if l.contains(marker) {
            drop[i] = true;
            if i > 0 && lines[i - 1].trim_start().starts_with("##") {
                drop[i - 1] = true;
            }
        }
    }
    let changed = drop.iter().any(|d| *d);
    let out = lines.iter().zip(&drop).filter(|(_, d)| !**d).map(|(l, _)| *l).collect();
    (out, changed)
}

/// Apply `edit` to startup.sh: backup once, stage next to it (keeping its mode),
/// `ksh -n` the staged file, then rename over. Returns false if nothing changed.
pub fn edit_startup<F>(ctx: &Ctx, edit: F) -> Result<bool>
where
    F: FnOnce(&str) -> Result<Option<String>>,
{
    let path = ctx.layout.startup_sh();
    let text = fs::read_to_string(&path).with_context(|| format!("reading {}", path.display()))?;
    let new = match edit(&text)? {
        Some(n) if n != text => n,
        _ => return Ok(false),
    };
    let _sys = ctx.mounts.rw("/mnt/system")?;
    let _app = ctx.mounts.rw("/mnt/app")?;
    let pre = path.with_file_name("startup.sh.pre-mibpkg");
    if !pre.exists() {
        fs::copy(&path, &pre).context("backing up startup.sh")?;
    }
    backup_once(&ctx.layout, STARTUP_UNIT)?;
    let staged = path.with_file_name("startup.sh.mibpkg-new");
    fs::copy(&path, &staged).context("staging startup.sh")?; // carries the mode
    fs::write(&staged, &new).context("writing staged startup.sh")?;
    if !ctx.sys.shell_syntax_ok(&staged) {
        let _ = fs::remove_file(&staged);
        bail!("the edited startup.sh failed `ksh -n`; startup.sh left untouched");
    }
    fs::rename(&staged, &path).context("replacing startup.sh")?;
    Ok(true)
}

pub fn install_hook(ctx: &Ctx) -> Result<bool> {
    edit_startup(ctx, |t| add_hook(t).map(Some))
}

pub fn uninstall_hook(ctx: &Ctx) -> Result<bool> {
    edit_startup(ctx, |t| Ok(Some(remove_lines(t, MARKER).0)))
}

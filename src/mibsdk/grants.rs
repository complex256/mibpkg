//! `grants.properties` (one `<id>=<perm>,<perm>` line per plugin, read by the runtime's
//! `Grants.load`) and the approval arithmetic of a plugin install or update.
//!
//! The runtime is all-or-nothing today: a plugin whose declared permissions are not all granted is
//! `BLOCKED_PERMISSIONS` and never runs. Approval is still kept per permission id so optional
//! permissions can be added later.
use super::catalog::is_permission;
use super::{propsline, put, GRANTS};
use crate::ctx::Ctx;
use crate::paths::Layout;
use anyhow::{bail, Context, Result};
use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::io;

const MODE: u32 = 0o644;

/// What an install or update asks the user for and what it then grants.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Approval {
    /// Declared permissions that need an explicit approval, in declared order.
    pub ask: Vec<String>,
    /// The plugin's grants line after the change.
    pub grant: BTreeSet<String>,
}

/// Every plugin's granted permissions, parsed as `Grants.load` does (comments `#`/`!` skipped,
/// keys and list items trimmed, empty items ignored, the last line of a key wins). A missing file
/// is no grants; an unreadable one is an error.
pub fn read(layout: &Layout) -> Result<BTreeMap<String, BTreeSet<String>>> {
    let text = match fs::read_to_string(layout.p(GRANTS)) {
        Ok(t) => t,
        Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(BTreeMap::new()),
        Err(e) => return Err(e).with_context(|| format!("reading {GRANTS}")),
    };
    let mut out = BTreeMap::new();
    for line in text.lines() {
        let t = line.trim_start();
        if t.is_empty() || t.starts_with('#') || t.starts_with('!') {
            continue;
        }
        let (k, v) = t.split_once('=').unwrap_or((t, ""));
        let perms = v.split(',').map(str::trim).filter(|s| !s.is_empty()).map(String::from).collect();
        out.insert(k.trim().to_string(), perms);
    }
    Ok(out)
}

/// The text of `grants.properties` (empty when missing).
fn current_text(layout: &Layout) -> Result<String> {
    match fs::read_to_string(layout.p(GRANTS)) {
        Ok(t) => Ok(t),
        Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(String::new()),
        Err(e) => Err(e).with_context(|| format!("reading {GRANTS}")),
    }
}

/// Sets `id`'s grants line to `perms` (empty = revoke all), keeping every other line. Caller
/// holds `ctx.mounts.rw("/mnt/app")` and the action lock.
pub fn set(ctx: &Ctx, id: &str, perms: &BTreeSet<String>) -> Result<()> {
    if let Some(bad) = perms.iter().find(|p| !is_permission(p)) {
        bail!("cannot grant {bad:?} to {id}: a permission is 1 or more of [A-Za-z0-9._-]");
    }
    let list: Vec<&str> = perms.iter().map(String::as_str).collect();
    let text = propsline::set(&current_text(&ctx.layout)?, id, &list.join(","))?;
    put(ctx, GRANTS, text.as_bytes(), MODE)
}

/// `id` has a grants line (keyed exactly `id`, as `drop_line` matches it).
pub fn has_line(layout: &Layout, id: &str) -> Result<bool> {
    Ok(propsline::drop(&current_text(layout)?, id).1)
}

/// Removes `id`'s grants line; whether there was one. Nothing is written when there was none.
/// Caller holds `ctx.mounts.rw("/mnt/app")` and the action lock.
pub fn drop_line(ctx: &Ctx, id: &str) -> Result<bool> {
    let (text, dropped) = propsline::drop(&current_text(&ctx.layout)?, id);
    if dropped {
        put(ctx, GRANTS, text.as_bytes(), MODE)?;
    }
    Ok(dropped)
}

/// `ask` = what `declared_new` adds over `declared_old` (all of it for a fresh install, `None`);
/// `grant` = what is still granted of `declared_new`, plus `ask`. A permission the user revoked
/// stays revoked across an update; one the new version no longer declares is dropped.
pub fn plan(declared_new: &[String], declared_old: Option<&[String]>, current: &BTreeSet<String>) -> Approval {
    let mut ask: Vec<String> = Vec::new();
    for p in declared_new {
        if !declared_old.is_some_and(|old| old.contains(p)) && !ask.contains(p) {
            ask.push(p.clone());
        }
    }
    let mut grant: BTreeSet<String> = declared_new.iter().filter(|p| current.contains(*p)).cloned().collect();
    grant.extend(ask.iter().cloned());
    Approval { ask, grant }
}

/// The declared permissions `grant` lacks, in declared order: non-empty means the runtime will
/// hold the plugin `BLOCKED_PERMISSIONS`.
pub fn missing(declared: &[String], grant: &BTreeSet<String>) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for p in declared {
        if !grant.contains(p) && !out.contains(p) {
            out.push(p.clone());
        }
    }
    out
}

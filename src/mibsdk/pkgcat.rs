//! The MMI's package catalog snapshot: `/tmp/mibpkg.catalog`.
//!
//! The SDK's Plugins menu (Installed / Updates / Available) reads this file instead of talking to mibpkg:
//! one `key=value` line per field, rewritten atomically after anything that changes what is installed or
//! offered (an install, update, rollback, remove, an online check, and the supervisor's start). The SDK
//! polls the file, so it never needs a socket, a lock or a second process.
//!
//! Format: `format=1`, `updated=<unix seconds>`, `count=<packages>`, then per package, sorted by name:
//!
//! ```text
//! pkg.<name>.desc=<one line>
//! pkg.<name>.kind=plugin|sdk|plain
//! pkg.<name>.installed=<version>          (installed only)
//! pkg.<name>.blocked=<perm>,...           (installed plugins: declared but not granted)
//! pkg.<name>.update=<version>             (a newer compatible version exists)
//! pkg.<name>.updatefrom=<collection id>
//! pkg.<name>.avail=<version>              (newest compatible version on offer)
//! pkg.<name>.availfrom=<collection id>
//! pkg.<name>.availname=<collection name>
//! pkg.<name>.compatible=true|false
//! pkg.<name>.minsdk=<version>             (a plugin's declared minimum SDK)
//! pkg.<name>.perms=<perm>,...             (declared by the offered/installed version)
//! pkg.<name>.granted=<perm>,...           (installed plugins)
//! pkg.<name>.perm.<perm>=<plain words>
//! ```
//!
//! Values are single-line: a newline becomes `\n`, a backslash `\\`, so the reader can split on the first
//! `=` and unescape. A failure to write never fails the command that triggered it: the file is a cache.
use crate::catalog::{self, Avail, PkgView};
use crate::paths::Layout;
use crate::unit;
use anyhow::Result;
use std::fs;
use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};

/// The snapshot's file name under the layout's run directory: `/tmp/mibpkg.catalog` on the unit.
pub const CATALOG: &str = "catalog";
/// The format this writer emits; the reader refuses anything else.
pub const FORMAT: u32 = 1;

/// The snapshot's path in `layout` (`/tmp/mibpkg.catalog` on the unit).
pub fn path(layout: &Layout) -> std::path::PathBuf {
    layout.run(CATALOG)
}

/// Rewrites the snapshot from the current catalog. Best effort: the caller logs a failure and carries on.
pub fn refresh(layout: &Layout) -> Result<()> {
    write_to(&path(layout), layout)
}

/// Rewrites `path` from the current catalog (`tests` use their own path).
pub fn write_to(path: &Path, layout: &Layout) -> Result<()> {
    let text = render(&catalog::build(layout, &unit::detect(layout)), now_secs());
    // Same directory, so the rename is atomic for a reader that polls the file.
    let tmp = path.with_extension("catalog.tmp");
    fs::write(&tmp, text.as_bytes())?;
    // /tmp on the unit is /dev/shmem, where a plain rename reports EXDEV: fsutil copies instead.
    crate::fsutil::rename(&tmp, path)?;
    Ok(())
}

fn now_secs() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0)
}

/// The file's text for `views`; pure, so tests can pin it.
pub fn render(views: &[PkgView], updated: u64) -> String {
    let mut out = String::new();
    out.push_str("# mibpkg package catalog for the SDK's Plugins menu; rewritten by mibpkg, do not edit.\n");
    line(&mut out, "format", &FORMAT.to_string());
    line(&mut out, "updated", &updated.to_string());
    line(&mut out, "count", &views.len().to_string());
    for v in views {
        let p = format!("pkg.{}", v.name);
        line(&mut out, &format!("{p}.desc"), &v.description);
        line(&mut out, &format!("{p}.kind"), kind(v));
        if let Some(i) = &v.installed {
            line(&mut out, &format!("{p}.installed"), &i.version);
            if !i.blocked.is_empty() {
                line(&mut out, &format!("{p}.blocked"), &i.blocked.join(","));
            }
            line(&mut out, &format!("{p}.granted"), &granted(i));
        }
        if let Some((up, from)) = update_of(v) {
            line(&mut out, &format!("{p}.update"), &up);
            line(&mut out, &format!("{p}.updatefrom"), &from);
        }
        if let Some(a) = offered(v) {
            line(&mut out, &format!("{p}.avail"), &a.version);
            line(&mut out, &format!("{p}.availfrom"), &a.collection);
            line(&mut out, &format!("{p}.availname"), &a.collection_name);
            line(&mut out, &format!("{p}.compatible"), if a.compatible { "true" } else { "false" });
            if let Some(m) = &a.min_sdk {
                line(&mut out, &format!("{p}.minsdk"), m);
            }
        }
        // The permissions the page shows: the offered version's, else the installed one's.
        let perms = offered(v).map(|a| a.permissions.as_slice())
            .or_else(|| v.installed.as_ref().map(|i| i.permissions.as_slice()))
            .unwrap_or(&[]);
        if !perms.is_empty() {
            let ids: Vec<&str> = perms.iter().map(|x| x.id.as_str()).collect();
            line(&mut out, &format!("{p}.perms"), &ids.join(","));
            for x in perms {
                line(&mut out, &format!("{p}.perm.{}", x.id), &x.text);
            }
        }
    }
    out
}

/// The newer version an installed package can move to, and the collection it comes from. The catalog's
/// own `update` names only the package's recorded collection; a package installed from an upload has
/// none, so the newest compatible offer is the update the MMI shows.
fn update_of(v: &PkgView) -> Option<(String, String)> {
    if let (Some(up), Some(from)) = (&v.update, &v.update_from) {
        return Some((up.clone(), from.clone()));
    }
    let installed = v.installed.as_ref()?;
    let a = offered(v)?;
    if !a.compatible || crate::index::cmp_versions(&a.version, &installed.version) != std::cmp::Ordering::Greater {
        return None;
    }
    Some((a.version.clone(), a.collection.clone()))
}

fn kind(v: &PkgView) -> &'static str {
    let k = v.installed.as_ref().and_then(|i| i.kind.as_deref()).or_else(|| offered(v).and_then(|a| a.kind.as_deref()));
    match k {
        Some("plugin") => "plugin",
        Some("sdk") => "sdk",
        _ => "plain",
    }
}

fn granted(i: &crate::catalog::Inst) -> String {
    let ids: Vec<&str> = i.permissions.iter().filter(|p| p.granted == Some(true)).map(|p| p.id.as_str()).collect();
    ids.join(",")
}

/// The version on offer the MMI names: the update's own entry, else the newest compatible one, else the
/// newest of any collection.
fn offered(v: &PkgView) -> Option<&Avail> {
    if let (Some(up), Some(from)) = (&v.update, &v.update_from) {
        if let Some(a) = v.available.iter().find(|a| &a.version == up && &a.collection == from) {
            return Some(a);
        }
    }
    v.available
        .iter()
        .filter(|a| a.compatible)
        .max_by(|a, b| crate::index::cmp_versions(&a.version, &b.version))
        .or_else(|| v.available.iter().max_by(|a, b| crate::index::cmp_versions(&a.version, &b.version)))
}

/// One `key=value` line with the value escaped.
fn line(out: &mut String, key: &str, value: &str) {
    out.push_str(key);
    out.push('=');
    for c in value.chars() {
        match c {
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => {}
            _ => out.push(c),
        }
    }
    out.push('\n');
}

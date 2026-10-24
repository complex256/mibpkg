//! The user's on/off switches, `PERSIST/plugins.properties`: one `<id>.off=true` line per plugin
//! switched off, owned by the runtime's `UserToggles`.
use super::{propsline, put, TOGGLES};
use crate::ctx::Ctx;
use crate::paths::Layout;
use anyhow::{Context, Result};
use std::collections::BTreeSet;
use std::fs;
use std::io;

const SUFFIX: &str = ".off";

/// A plugin id as the runtime's `ManifestParser.isValidId`: `[a-z0-9][a-z0-9-]{1,40}`.
fn valid_id(id: &str) -> bool {
    let b = id.as_bytes();
    (2..=41).contains(&b.len())
        && (b[0].is_ascii_lowercase() || b[0].is_ascii_digit())
        && b[1..].iter().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || *c == b'-')
}

/// The file `UserToggles` reads: the target, or its `.tmp` when the target is not a file (the
/// runtime died between delete and rename). A non-file target without a `.tmp` is read as is
/// (and fails).
fn source(layout: &Layout) -> String {
    let tmp = format!("{TOGGLES}.tmp");
    if layout.p(TOGGLES).is_file() || !layout.p(&tmp).exists() {
        TOGGLES.to_string()
    } else {
        tmp
    }
}

/// The ids switched off. As `UserToggles`: a missing file falls back to its `.tmp` (the runtime
/// died between delete and rename), and an unreadable file reads as empty (every plugin on).
pub fn off_ids(layout: &Layout) -> BTreeSet<String> {
    let Ok(text) = fs::read_to_string(layout.p(&source(layout))) else { return BTreeSet::new() };
    let mut out = BTreeSet::new();
    for line in text.lines() {
        let t = line.trim_start();
        if t.starts_with('#') || t.starts_with('!') {
            continue;
        }
        let Some((k, v)) = t.split_once('=') else { continue };
        if let Some(id) = k.trim().strip_suffix(SUFFIX) {
            if v.trim() == "true" && valid_id(id) {
                out.insert(id.to_string());
            }
        }
    }
    out
}

/// The file `UserToggles` reads has a `<id>.off` line (whatever its value), as `drop` matches it.
/// An unreadable file has none.
pub fn has_line(layout: &Layout, id: &str) -> bool {
    fs::read_to_string(layout.p(&source(layout))).is_ok_and(|t| propsline::drop(&t, &format!("{id}{SUFFIX}")).1)
}

/// Drops `<id>.off` so a reinstalled plugin starts on; whether there was such a line. Read from
/// the file `UserToggles` reads (the `.tmp` fallback included); the result is written to the
/// target, which the runtime then prefers. Nothing is written when there was no such line. PERSIST is writable without a remount; the caller holds the
/// action lock.
pub fn drop(ctx: &Ctx, id: &str) -> Result<bool> {
    let src = source(&ctx.layout);
    let text = match fs::read_to_string(ctx.layout.p(&src)) {
        Ok(t) => t,
        Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(false),
        Err(e) => return Err(e).with_context(|| format!("reading {src}")),
    };
    let (text, dropped) = propsline::drop(&text, &format!("{id}{SUFFIX}"));
    if dropped {
        put(ctx, TOGGLES, text.as_bytes(), 0o644)?;
    }
    Ok(dropped)
}

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

/// The ids switched off. As `UserToggles`: a missing file falls back to its `.tmp` (the runtime
/// died between delete and rename), and an unreadable file reads as empty (every plugin on).
pub fn off_ids(layout: &Layout) -> BTreeSet<String> {
    let file = layout.p(TOGGLES);
    let file = if file.is_file() { file } else { layout.p(&format!("{TOGGLES}.tmp")) };
    let Ok(text) = fs::read_to_string(file) else { return BTreeSet::new() };
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

/// Drops `<id>.off` so a reinstalled plugin starts on; whether there was such a line. Nothing is
/// written when there was none. PERSIST is writable without a remount; the caller holds the
/// action lock.
pub fn drop(ctx: &Ctx, id: &str) -> Result<bool> {
    let text = match fs::read_to_string(ctx.layout.p(TOGGLES)) {
        Ok(t) => t,
        Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(false),
        Err(e) => return Err(e).with_context(|| format!("reading {TOGGLES}")),
    };
    let (text, dropped) = propsline::drop(&text, &format!("{id}{SUFFIX}"));
    if dropped {
        put(ctx, TOGGLES, text.as_bytes(), 0o644)?;
    }
    Ok(dropped)
}

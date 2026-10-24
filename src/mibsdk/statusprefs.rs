//! The user's status-line choices, `PERSIST/statusline.properties`, owned by the runtime: an
//! `order=<id>,<id>,...` line (the saved order) and one `<id>.hidden=true` line per plugin whose
//! status item the user hid.
use super::{propsline, put, STATUSPREFS};
use crate::ctx::Ctx;
use crate::paths::Layout;
use anyhow::{Context, Result};
use std::fs;
use std::io;

const HIDDEN: &str = ".hidden";
const ORDER: &str = "order";

/// The file the runtime reads: the target, or its `.tmp` when the target is not a file (the
/// runtime died between delete and rename). A non-file target without a `.tmp` is read as is
/// (and fails).
fn source(layout: &Layout) -> String {
    let tmp = format!("{STATUSPREFS}.tmp");
    if layout.p(STATUSPREFS).is_file() || !layout.p(&tmp).exists() {
        STATUSPREFS.to_string()
    } else {
        tmp
    }
}

/// `text` without `id`'s hidden line and without `id` in the `order` list (the `order` line goes
/// when that empties it), and whether anything changed. Every other line is kept byte for byte.
fn strip(text: &str, id: &str) -> (String, bool) {
    let (text, hidden) = propsline::drop(text, &format!("{id}{HIDDEN}"));
    let Some(order) = propsline::get(&text, ORDER) else { return (text, hidden) };
    let ids: Vec<&str> = order.split(',').map(str::trim).filter(|s| !s.is_empty()).collect();
    if !ids.contains(&id) {
        return (text, hidden);
    }
    let rest: Vec<&str> = ids.into_iter().filter(|s| *s != id).collect();
    if rest.is_empty() {
        return (propsline::drop(&text, ORDER).0, true);
    }
    // `set` cannot fail here: the key and the joined ids hold no line break (they came from lines).
    match propsline::set(&text, ORDER, &rest.join(",")) {
        Ok(t) => (t, true),
        Err(_) => (text, hidden),
    }
}

/// The file the runtime reads has a `<id>.hidden` line (whatever its value) or `id` in its
/// `order` list, as `drop` matches them. An unreadable file has none.
pub fn has_line(layout: &Layout, id: &str) -> bool {
    fs::read_to_string(layout.p(&source(layout))).is_ok_and(|t| strip(&t, id).1)
}

/// Drops `id` from the status-line settings so a reinstalled plugin starts shown and unordered;
/// whether anything changed. Read from the file the runtime reads (the `.tmp` fallback
/// included); the result is written to the target, which the runtime then prefers. Nothing is
/// written when there was nothing to drop. PERSIST is writable without a remount; the caller
/// holds the action lock.
pub fn drop(ctx: &Ctx, id: &str) -> Result<bool> {
    let src = source(&ctx.layout);
    let text = match fs::read_to_string(ctx.layout.p(&src)) {
        Ok(t) => t,
        Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(false),
        Err(e) => return Err(e).with_context(|| format!("reading {src}")),
    };
    let (text, dropped) = strip(&text, id);
    if dropped {
        put(ctx, STATUSPREFS, text.as_bytes(), 0o644)?;
    }
    Ok(dropped)
}

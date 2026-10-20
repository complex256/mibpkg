//! The runtime's answer to a reload marker, read from its log (`LOG`): the dev channel logs
//! `dev channel: reload requested for <id>` (newer runtimes add ` token=<marker content>`), then
//! the reload logs `STATUS <id> <STATE> <reason>` lines (a loaded plugin first goes
//! `DISABLED reloading`).
//!
//! The log file is a snapshot of the runtime's 2000-line ring buffer, rewritten in place (on
//! /tmp, a rename over it fails), so any read may be empty or cut short. A reload line therefore
//! counts as the answer to our request only when it carries our token, or when it follows the
//! last line of a whole baseline snapshot taken before the marker was written. Counting lines is
//! never used: a short read would make an old line look new. Once the log shows a runtime that
//! logs tokens, only the token answers.
//!
//! Older runtimes (no token) leave a narrow window: a read cut short exactly at a line break that
//! reads the same twice in a row passes for a whole baseline, so an old reload line between the
//! cut and the real end could be taken for ours (a stale STATUS, or a purge after an old unload).
use super::LOG;
use crate::paths::Layout;
use std::fs;
use std::thread;
use std::time::{Duration, Instant};

/// How long an install, grant or removal waits for the runtime (the dev channel polls every
/// `dev.poll.ms`, 2000 by default).
pub const ACK_TIMEOUT: Duration = Duration::from_secs(6);
const POLL: Duration = Duration::from_millis(250);
/// How long `baseline` retries a read that is empty, cut short or still changing.
const BASELINE_WAIT: Duration = Duration::from_millis(1000);
const BASELINE_POLL: Duration = Duration::from_millis(50);

/// What the log held before a reload was requested.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Baseline {
    /// The last line of a whole snapshot (lines start with the uptime in ms, so it is unique in
    /// practice); None when no whole snapshot could be read, and then only a token can answer.
    anchor: Option<String>,
}

impl Baseline {
    /// No usable snapshot.
    pub fn unknown() -> Baseline {
        Baseline { anchor: None }
    }

    /// The baseline of the snapshot `text`: whole only when non-empty and ending in a line break
    /// (the runtime ends every line with one).
    pub fn of(text: &str) -> Baseline {
        let whole = !text.is_empty() && text.ends_with('\n');
        Baseline { anchor: whole.then(|| text.lines().last().unwrap_or_default().to_string()).filter(|l| !l.is_empty()) }
    }
}

fn log_text(layout: &Layout) -> Option<String> {
    // Missing: not started, or between the runtime's delete and write.
    fs::read(layout.p(LOG)).ok().map(|b| String::from_utf8_lossy(&b).into_owned())
}

/// The log before a reload request: two equal reads in a row that make a whole snapshot,
/// retried for up to a second; otherwise `Baseline::unknown()` (never "empty").
pub fn baseline(layout: &Layout) -> Baseline {
    let end = Instant::now() + BASELINE_WAIT;
    let mut prev: Option<String> = None;
    loop {
        let now = log_text(layout);
        if let (Some(a), Some(b)) = (&prev, &now) {
            let base = Baseline::of(b);
            if a == b && base.anchor.is_some() {
                return base;
            }
        }
        if Instant::now() >= end {
            return Baseline::unknown();
        }
        prev = now;
        thread::sleep(BASELINE_POLL);
    }
}

/// The token the runtime logs for a marker holding `marker`: its first 4096 bytes read as
/// Latin-1, only bytes 0x21-0x7e kept, at most 64 chars (empty: the runtime logs no token).
pub fn token_of(marker: &[u8]) -> String {
    marker.iter().take(4096).filter(|b| (0x21..=0x7e).contains(*b)).take(64).map(|&b| b as char).collect()
}

/// For a dev-channel reload line of exactly `id`: `Some(token)` (None when it carries none).
fn reload<'l>(line: &'l str, id: &str) -> Option<Option<&'l str>> {
    let rest = line.split_once("reload requested for ")?.1.trim();
    match rest.split_once(" token=") {
        Some((rid, tok)) if rid == id => Some(Some(tok.trim())),
        None if rest == id => Some(None),
        _ => None,
    }
}

/// `(STATE, reason)` of a `STATUS <id> <STATE> <reason>` line for exactly `id` (the runtime
/// writes `<uptime> STATUS <source> <msg>`; a line without the uptime is accepted too).
fn status(line: &str, id: &str) -> Option<(String, String)> {
    let rest = line.strip_prefix("STATUS ").or_else(|| line.split_once(" STATUS ").filter(|(up, _)| !up.contains(' ')).map(|(_, r)| r))?;
    let (src, msg) = rest.split_once(' ')?;
    if src != id {
        return None;
    }
    let (state, reason) = msg.split_once(' ').unwrap_or((msg, ""));
    (!state.is_empty()).then(|| (state.to_string(), reason.trim().to_string()))
}

/// The answer to our reload of `id` in the log `text`, if there is one yet: the last `STATUS <id>`
/// line after the reload line that is ours, unless that is `DISABLED reloading` (in progress).
///
/// Ours: the last reload line carrying `token`; else (no reload line in `text` carries a token,
/// so the runtime logs none) the first
/// token-less reload line after the baseline's anchor. Without the anchor in `text`, the ring
/// wrapped, but only a `stable` read (the same text twice in a row) is trusted with that: a read cut
/// short loses the anchor too. An unknown baseline is answered by the token alone.
#[doc(hidden)]
pub fn answer(text: &str, id: &str, base: &Baseline, token: Option<&str>, stable: bool) -> Option<(String, String)> {
    let lines: Vec<&str> = text.lines().collect();
    // A runtime that logs tokens: a token-less line is never ours.
    let token_runtime =
        lines.iter().any(|l| l.split_once("reload requested for ").is_some_and(|(_, r)| r.contains(" token=")));
    let by_token = token.and_then(|t| lines.iter().rposition(|l| reload(l, id) == Some(Some(t))));
    let trigger = match by_token {
        Some(i) => i,
        None if token_runtime => return None,
        None => {
            let anchor = base.anchor.as_deref()?;
            let from = match lines.iter().rposition(|l| *l == anchor) {
                Some(i) => i + 1,
                None if stable => 0,
                None => return None,
            };
            from + lines[from..].iter().position(|l| reload(l, id) == Some(None))?
        }
    };
    let last = lines[trigger + 1..].iter().rev().find_map(|l| status(l, id));
    last.filter(|(state, reason)| !(state == "DISABLED" && reason == "reloading"))
}

/// Poll the log every 250 ms until our reload of `id` (`token`: `token_of` the marker written)
/// has been answered (`answer`), or `timeout` passes. Returns the plugin's state and reason.
pub fn wait(layout: &Layout, id: &str, base: &Baseline, token: Option<&str>, timeout: Duration) -> Option<(String, String)> {
    let end = Instant::now() + timeout;
    let mut prev: Option<String> = None;
    loop {
        if let Some(text) = log_text(layout) {
            let stable = prev.as_deref() == Some(text.as_str());
            if let Some(a) = answer(&text, id, base, token, stable) {
                return Some(a);
            }
            prev = Some(text);
        } else {
            prev = None;
        }
        let now = Instant::now();
        if now >= end {
            return None;
        }
        thread::sleep(POLL.min(end - now));
    }
}

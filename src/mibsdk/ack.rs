//! The runtime's answer to a reload marker, read from its log (`LOG`): the dev channel logs
//! `dev channel: reload requested for <id>`, then the reload logs `STATUS <id> <STATE> <reason>`
//! lines (a loaded plugin first goes `DISABLED reloading`).
//!
//! The log file is a snapshot of the runtime's 2000-line ring buffer, rewritten as it goes, so a
//! count of reload lines is reliable until the ring wraps.
use super::LOG;
use crate::paths::Layout;
use std::fs;
use std::thread;
use std::time::{Duration, Instant};

/// How long an install, grant or removal waits for the runtime (the dev channel polls every
/// `dev.poll.ms`, 2000 by default).
pub const ACK_TIMEOUT: Duration = Duration::from_secs(6);
const POLL: Duration = Duration::from_millis(250);

fn log_text(layout: &Layout) -> String {
    // Missing (not started, or between the runtime's delete and rename): no lines yet.
    fs::read(layout.p(LOG)).map(|b| String::from_utf8_lossy(&b).into_owned()).unwrap_or_default()
}

/// The line is the dev channel's `reload requested for <id>` (the exact id).
fn is_reload(line: &str, id: &str) -> bool {
    line.split_once("reload requested for ").is_some_and(|(_, rest)| rest.trim() == id)
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

/// The number of `reload requested for <id>` lines in the log now.
pub fn count(layout: &Layout, id: &str) -> usize {
    log_text(layout).lines().filter(|l| is_reload(l, id)).count()
}

/// The answer after the `before`-th reload line, if there is one yet: the last `STATUS <id>` line
/// after the first new reload line, unless that is `DISABLED reloading` (still in progress).
fn answer(text: &str, id: &str, before: usize) -> Option<(String, String)> {
    let mut seen = 0;
    let mut last = None;
    for line in text.lines() {
        if seen <= before {
            if is_reload(line, id) {
                seen += 1;
            }
            continue;
        }
        if let Some(s) = status(line, id) {
            last = Some(s);
        }
    }
    last.filter(|(state, reason)| !(state == "DISABLED" && reason == "reloading"))
}

/// Poll the log every 250 ms until a reload of `id` beyond the first `before` ones has been
/// answered (`answer`), or `timeout` passes. Returns the plugin's state and reason.
pub fn wait(layout: &Layout, id: &str, before: usize, timeout: Duration) -> Option<(String, String)> {
    let end = Instant::now() + timeout;
    loop {
        if let Some(a) = answer(&log_text(layout), id, before) {
            return Some(a);
        }
        let now = Instant::now();
        if now >= end {
            return None;
        }
        thread::sleep(POLL.min(end - now));
    }
}

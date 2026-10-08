//! Live GEM status: values in the unit's persistence that GEM `per` rows poll while a screen
//! is open. The namespace is unlisted in persistence.json, so it lands in the SQLite default
//! backend (flash): write only when a value changes, and keep values short and free of detail
//! that changes without anything worth seeing (pids, times, counts).
use crate::status::{SvcState, SvcStatus};
use crate::paths::Layout;
use crate::sys::Sys;
use std::path::PathBuf;
use std::collections::HashMap;
use std::time::{Duration, Instant};

/// "mipk"; not mapped to any stock backend (verified on the unit 2026-09-28).
pub const NS: u32 = 1835626603;
pub const KEY_SUPERVISOR: u32 = 1;
pub const KEY_RESULT_GLOBAL: u32 = 2;
pub const KEY_ONLINE: u32 = 3;

fn fnv1a(s: &str) -> u32 {
    s.bytes().fold(0x811c_9dc5u32, |h, b| (h ^ b as u32).wrapping_mul(0x0100_0193))
}

/// Stable key for a service's status line.
pub fn service_key(id: &str) -> u32 {
    0x1000_0000 | (fnv1a(id) & 0x0FFF_FFFF)
}

/// Stable key for a package's last result ("_" = the global result).
pub fn result_key(pkg: &str) -> u32 {
    if pkg == "_" {
        KEY_RESULT_GLOBAL
    } else {
        0x2000_0000 | (fnv1a(pkg) & 0x0FFF_FFFF)
    }
}

/// GEM shows a live value on one line next to its label and never wraps it: every value is cut
/// to this many characters (seen on the unit, 2026-10-07).
pub const MAX_VALUE: usize = 20;

/// `text`, cut to `MAX_VALUE` characters with "…" when longer.
pub fn short(text: &str) -> String {
    if text.chars().count() <= MAX_VALUE {
        return text.to_string();
    }
    let mut out: String = text.chars().take(MAX_VALUE - 1).collect();
    out.push('…');
    out
}

/// A service's GEM value: its state only ("running", "waiting: tailnet", "needs config"). No pid,
/// restart count or wait time: those change without anything worth seeing happening, and each
/// change is a write to flash. The full line is in `mibpkg status` and the web UI.
pub fn persist_text(s: &SvcStatus) -> String {
    let text = match s.state {
        SvcState::Waiting => format!("waiting: {}", s.waiting_for.join(", ")),
        SvcState::NeedsConfig => "needs config".to_string(),
        other => other.to_string(),
    };
    short(&text)
}

/// An action result's GEM value: "OK", "busy", or "FAILED: <reason>" cut to fit. The whole line
/// is in GEM's Console, the web UI and state.
pub fn result_value(line: &str) -> String {
    if line.starts_with("OK:") {
        "OK".to_string()
    } else if line.starts_with("BUSY:") {
        "busy".to_string()
    } else {
        short(line)
    }
}

/// Publish an action's result line to its GEM row ("last result" for "_", or the package's row)
/// as its short outcome, unless that row already shows it.
pub fn publish_result(sys: &dyn Sys, layout: &Layout, key: &str, line: &str) {
    Publisher::with_memo(layout).set(sys, result_key(key), &result_value(line));
}

/// After a failed write, wait this long before trying again: a `pc` call to a persistence
/// service that isn't up yet can block for seconds, and the supervisor loop must not stall.
const RETRY_AFTER: Duration = Duration::from_secs(30);

/// Writes a value only when it differs from the last one written. With a memo (`with_memo`) the
/// last value of each row is also kept in /tmp/mibpkg.live.<key>, so separate processes (the
/// supervisor, each `gem` action) do not write the same value again either.
#[derive(Default)]
pub struct Publisher {
    last: HashMap<u32, String>,
    paused_until: Option<Instant>,
    memo: Option<PathBuf>,
}

impl Publisher {
    pub fn with_memo(layout: &Layout) -> Publisher {
        Publisher { memo: Some(layout.p("/tmp")), ..Default::default() }
    }

    fn memo_file(&self, key: u32) -> Option<PathBuf> {
        self.memo.as_ref().map(|d| d.join(format!("mibpkg.live.{key:08x}")))
    }

    pub fn set(&mut self, sys: &dyn Sys, key: u32, text: &str) {
        if self.last.get(&key).map(|s| s.as_str()) == Some(text) {
            return;
        }
        if let Some(f) = self.memo_file(key) {
            if std::fs::read_to_string(&f).ok().as_deref() == Some(text) {
                self.last.insert(key, text.to_string());
                return;
            }
        }
        if self.paused_until.map_or(false, |t| Instant::now() < t) {
            return; // retried on a later tick: `last` still differs
        }
        match sys.persist_write(NS, key, text) {
            Ok(()) => {
                self.paused_until = None;
                self.last.insert(key, text.to_string());
                if let Some(f) = self.memo_file(key) {
                    let _ = crate::fsutil::atomic_write(&f, text.as_bytes());
                }
            }
            Err(e) => {
                crate::warn!("mibpkg: live status: {e:#} (retrying in {}s)", RETRY_AFTER.as_secs());
                self.paused_until = Some(Instant::now() + RETRY_AFTER);
            }
        }
    }
}

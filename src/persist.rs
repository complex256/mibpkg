//! Live GEM status: values in the unit's persistence that GEM `per` rows poll while a screen
//! is open. The namespace is unlisted in persistence.json, so it lands in the SQLite default
//! backend (flash): write only when a value changes.
use crate::status::{SvcState, SvcStatus};
use crate::sys::Sys;
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

/// Like `status::describe`, without the ticking wait duration (it would rewrite flash every tick).
pub fn persist_text(s: &SvcStatus) -> String {
    if s.state == SvcState::Waiting {
        format!("waiting: {}", s.waiting_for.join(", "))
    } else {
        crate::status::describe(s)
    }
}

/// After a failed write, wait this long before trying again: a `pc` call to a persistence
/// service that isn't up yet can block for seconds, and the supervisor loop must not stall.
const RETRY_AFTER: Duration = Duration::from_secs(30);

/// Writes a value only when it differs from what this process last wrote.
#[derive(Default)]
pub struct Publisher {
    last: HashMap<u32, String>,
    paused_until: Option<Instant>,
}

impl Publisher {
    pub fn set(&mut self, sys: &dyn Sys, key: u32, text: &str) {
        if self.last.get(&key).map(|s| s.as_str()) == Some(text) {
            return;
        }
        if self.paused_until.map_or(false, |t| Instant::now() < t) {
            return; // retried on a later tick: `last` still differs
        }
        match sys.persist_write(NS, key, text) {
            Ok(()) => {
                self.paused_until = None;
                self.last.insert(key, text.to_string());
            }
            Err(e) => {
                eprintln!("mibpkg: live status: {e:#} (retrying in {}s)", RETRY_AFTER.as_secs());
                self.paused_until = Some(Instant::now() + RETRY_AFTER);
            }
        }
    }
}

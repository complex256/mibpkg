//! The service supervisor: starts boot.d services from /tmp copies, restarts
//! them with backoff, gives up on crash loops, keeps capped logs in RAM.
use std::collections::VecDeque;
use std::fs::{self, File, OpenOptions};
use std::io::Write;
use std::path::PathBuf;
use std::time::{Duration, Instant};

#[derive(Debug, Clone)]
pub struct Timing {
    pub backoff_start: Duration,
    pub backoff_max: Duration,
    pub fail_window: Duration,
    pub fail_count: usize,
    pub poll: Duration,
    pub stop_grace: Duration,
    pub config_recheck: Duration,
    /// A service running this long gets its backoff reset.
    pub stable_after: Duration,
}

impl Default for Timing {
    fn default() -> Self {
        Timing {
            backoff_start: Duration::from_secs(1),
            backoff_max: Duration::from_secs(300),
            fail_window: Duration::from_secs(600),
            fail_count: 5,
            poll: Duration::from_secs(1),
            stop_grace: Duration::from_secs(5),
            config_recheck: Duration::from_secs(10),
            stable_after: Duration::from_secs(60),
        }
    }
}

pub fn next_backoff(t: &Timing, prev: Option<Duration>) -> Duration {
    match prev {
        None => t.backoff_start,
        Some(p) => (p * 2).min(t.backoff_max),
    }
}

#[derive(Debug, Default)]
pub struct CrashWindow {
    exits: VecDeque<Instant>,
}

impl CrashWindow {
    /// Record an exit; true when `fail_count` exits fall within `fail_window`.
    pub fn record(&mut self, now: Instant, t: &Timing) -> bool {
        self.exits.push_back(now);
        while let Some(first) = self.exits.front() {
            if now.duration_since(*first) > t.fail_window {
                self.exits.pop_front();
            } else {
                break;
            }
        }
        self.exits.len() >= t.fail_count
    }
}

/// Append-only log that rotates to `<path>.1` once it would exceed `cap` bytes.
pub struct CappedLog {
    path: PathBuf,
    cap: u64,
    file: Option<File>,
    size: u64,
}

impl CappedLog {
    pub fn open(path: PathBuf, cap: u64) -> CappedLog {
        let size = fs::metadata(&path).map(|m| m.len()).unwrap_or(0);
        let file = OpenOptions::new().create(true).append(true).open(&path).ok();
        CappedLog { path, cap, file, size }
    }

    pub fn write(&mut self, data: &[u8]) {
        if self.size + data.len() as u64 > self.cap && self.size > 0 {
            self.file = None;
            let mut rotated = self.path.as_os_str().to_owned();
            rotated.push(".1");
            let _ = fs::rename(&self.path, PathBuf::from(rotated));
            self.file = OpenOptions::new().create(true).write(true).truncate(true).open(&self.path).ok();
            self.size = 0;
        }
        if let Some(f) = self.file.as_mut() {
            if f.write_all(data).is_ok() {
                self.size += data.len() as u64;
            }
        }
    }
}

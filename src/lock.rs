//! One mibpkg action at a time (GEM double taps, CLI + GEM).
use crate::paths::Layout;
use crate::sys::Sys;
use anyhow::{bail, Result};
use std::fs::{self, OpenOptions};
use std::io::{ErrorKind, Write};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

/// How often a waiting `acquire_at` retries a lock held by a live process.
const POLL: Duration = Duration::from_millis(50);

#[derive(Debug)]
pub struct Lock {
    path: PathBuf,
}

impl Lock {
    /// The action lock: fails at once if another live mibpkg holds it.
    pub fn acquire(layout: &Layout, sys: &dyn Sys) -> Result<Lock> {
        Lock::acquire_at(&layout.run("lock"), sys, Duration::ZERO)
    }

    /// A pid lock file at `path`. A lock left by a dead process is taken over; one held by a
    /// live process is retried every 50 ms for up to `wait`, then is "busy".
    ///
    /// Takeover is best-effort: /tmp is shmem on the unit, with no rename or link to swap a lock
    /// atomically. So a stale lock is removed only if it still names the dead pid that was seen,
    /// and a new lock counts as taken only if it still names this process after the pid is
    /// written. That closes the common races (a file vanishing mid-check, two takers of the same
    /// dead pid), though not every interleaving.
    ///
    /// (Unix semantics assumed: on Windows a removed file that is still open stays
    /// "delete pending", so `create_new` would fail with an access error rather than retry.
    /// mibpkg runs on QNX and is tested on Linux/macOS.)
    pub fn acquire_at(path: &Path, sys: &dyn Sys, wait: Duration) -> Result<Lock> {
        let me = std::process::id();
        let deadline = Instant::now() + wait;
        let mut steals = 0;
        loop {
            match OpenOptions::new().write(true).create_new(true).open(path) {
                Ok(mut f) => {
                    write!(f, "{me}")?;
                    drop(f);
                    if read_pid(path) == Some(me) {
                        return Ok(Lock { path: path.to_path_buf() });
                    }
                    // Another taker removed our new file as stale and made its own: theirs now.
                    std::thread::sleep(POLL);
                }
                Err(e) if e.kind() == ErrorKind::AlreadyExists => {
                    let mut holder = read_pid(path);
                    if holder.is_none() {
                        if !path.exists() {
                            continue; // released between our create and our read: try again
                        }
                        // Created but its pid not written yet: give the holder a moment.
                        std::thread::sleep(Duration::from_millis(100));
                        holder = read_pid(path);
                        if holder.is_none() && !path.exists() {
                            continue;
                        }
                    }
                    match holder {
                        Some(pid) if sys.process_alive(pid) => {
                            if Instant::now() >= deadline {
                                bail!("busy: another mibpkg action is running (pid {pid})")
                            }
                            std::thread::sleep(POLL);
                        }
                        stale => {
                            // Remove only the stale lock we saw, never one somebody took since.
                            let removed = read_pid(path) == stale && fs::remove_file(path).is_ok();
                            steals += 1;
                            if steals >= 2 && Instant::now() >= deadline {
                                bail!("busy: could not take {}", path.display())
                            }
                            if !removed {
                                // Somebody else moved first; don't spin on liveness checks.
                                std::thread::sleep(POLL);
                            }
                        }
                    }
                }
                Err(e) => return Err(e.into()),
            }
        }
    }
}

/// The pid a lock file names, if it exists and names one.
fn read_pid(path: &Path) -> Option<u32> {
    fs::read_to_string(path).ok().and_then(|s| s.trim().parse::<u32>().ok())
}

impl Drop for Lock {
    /// Removes the lock only while it still names this process: a lock another process took
    /// over (wrongly judged stale) is theirs now.
    fn drop(&mut self) {
        if read_pid(&self.path) == Some(std::process::id()) {
            let _ = fs::remove_file(&self.path);
        }
    }
}

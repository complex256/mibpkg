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
    pub fn acquire_at(path: &Path, sys: &dyn Sys, wait: Duration) -> Result<Lock> {
        let deadline = Instant::now() + wait;
        let mut steals = 0;
        loop {
            match OpenOptions::new().write(true).create_new(true).open(path) {
                Ok(mut f) => {
                    write!(f, "{}", std::process::id())?;
                    return Ok(Lock { path: path.to_path_buf() });
                }
                Err(e) if e.kind() == ErrorKind::AlreadyExists => {
                    let read = || fs::read_to_string(path).ok().and_then(|s| s.trim().parse::<u32>().ok());
                    let mut holder = read();
                    if holder.is_none() && path.exists() {
                        // Created but its pid not written yet: give the holder a moment.
                        std::thread::sleep(Duration::from_millis(100));
                        holder = read();
                    }
                    match holder {
                        Some(pid) if sys.process_alive(pid) => {
                            if Instant::now() >= deadline {
                                bail!("busy: another mibpkg action is running (pid {pid})")
                            }
                            std::thread::sleep(POLL);
                        }
                        _ => {
                            let _ = fs::remove_file(path);
                            steals += 1;
                            if steals >= 2 && Instant::now() >= deadline {
                                bail!("busy: could not take {}", path.display())
                            }
                        }
                    }
                }
                Err(e) => return Err(e.into()),
            }
        }
    }
}

impl Drop for Lock {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.path);
    }
}

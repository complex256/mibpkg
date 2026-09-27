//! One mibpkg action at a time (GEM double taps, CLI + GEM).
use crate::paths::Layout;
use crate::sys::Sys;
use anyhow::{bail, Result};
use std::fs::{self, OpenOptions};
use std::io::{ErrorKind, Write};
use std::path::PathBuf;

#[derive(Debug)]
pub struct Lock {
    path: PathBuf,
}

impl Lock {
    pub fn acquire(layout: &Layout, sys: &dyn Sys) -> Result<Lock> {
        let path = layout.run("lock");
        for _ in 0..2 {
            match OpenOptions::new().write(true).create_new(true).open(&path) {
                Ok(mut f) => {
                    write!(f, "{}", std::process::id())?;
                    return Ok(Lock { path });
                }
                Err(e) if e.kind() == ErrorKind::AlreadyExists => {
                    let read = || fs::read_to_string(&path).ok().and_then(|s| s.trim().parse::<u32>().ok());
                    let mut holder = read();
                    if holder.is_none() && path.exists() {
                        // Created but its pid not written yet: give the holder a moment.
                        std::thread::sleep(std::time::Duration::from_millis(100));
                        holder = read();
                    }
                    match holder {
                        Some(pid) if sys.process_alive(pid) => {
                            bail!("busy: another mibpkg action is running (pid {pid})")
                        }
                        _ => {
                            let _ = fs::remove_file(&path);
                        }
                    }
                }
                Err(e) => return Err(e.into()),
            }
        }
        bail!("busy: could not take {}", path.display())
    }
}

impl Drop for Lock {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.path);
    }
}

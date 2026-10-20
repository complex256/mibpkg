//! Per-invocation context: layout, side effects, unit identity, and the
//! read-write mount guard shared by everything that writes.
use crate::paths::Layout;
use crate::sys::Sys;
use crate::unit::{detect, UnitInfo};
use crate::rwhold;
use anyhow::{bail, Result};
use std::cell::RefCell;
use std::collections::BTreeMap;
use std::path::PathBuf;

/// Refcounted `mount -uw`: nested guards remount once; the last drop goes read-only — unless
/// another live process still holds the mount read-write (see `rwhold`).
pub struct Mounts<'a> {
    sys: &'a dyn Sys,
    run_dir: PathBuf,
    holder: u32,
    counts: RefCell<BTreeMap<String, u32>>,
}

pub struct RwGuard<'m, 'a> {
    mounts: &'m Mounts<'a>,
    mount: String,
}

impl<'a> Mounts<'a> {
    /// Mounts held on behalf of this process.
    pub fn new(sys: &'a dyn Sys, layout: &Layout) -> Mounts<'a> {
        Mounts::with_holder(sys, layout, std::process::id())
    }

    /// Mounts held on behalf of pid `holder` (tests stand in for another process with it).
    pub fn with_holder(sys: &'a dyn Sys, layout: &Layout, holder: u32) -> Mounts<'a> {
        Mounts { sys, run_dir: rwhold::run_dir(layout), holder, counts: RefCell::new(BTreeMap::new()) }
    }

    pub fn rw<'m>(&'m self, mount: &str) -> Result<RwGuard<'m, 'a>> {
        let mut c = self.counts.borrow_mut();
        let n = c.entry(mount.to_string()).or_insert(0);
        if *n == 0 {
            rwhold::acquire_in(&self.run_dir, self.sys, mount, self.holder)?;
        }
        *n += 1;
        Ok(RwGuard { mounts: self, mount: mount.to_string() })
    }

    /// Remount `mount` read-write again (something outside the protocol may have flipped it);
    /// only while this process holds it.
    pub fn reassert(&self, mount: &str) -> Result<()> {
        if self.counts.borrow().get(mount).copied().unwrap_or(0) == 0 {
            bail!("{mount} is not held read-write by this process");
        }
        rwhold::reassert_in(&self.run_dir, self.sys, mount)
    }
}

impl Drop for RwGuard<'_, '_> {
    fn drop(&mut self) {
        let mut c = self.mounts.counts.borrow_mut();
        if let Some(n) = c.get_mut(&self.mount) {
            *n -= 1;
            if *n == 0 {
                if let Err(e) = rwhold::release_in(&self.mounts.run_dir, self.mounts.sys, &self.mount, self.mounts.holder) {
                    crate::warn!("warning: {e:#}");
                }
            }
        }
    }
}

pub struct Ctx<'a> {
    pub layout: Layout,
    pub sys: &'a dyn Sys,
    pub mounts: Mounts<'a>,
    pub unit: UnitInfo,
    /// Step-by-step progress of an action; GEM's Console shows stdout live.
    pub progress: Box<dyn Fn(&str) + 'a>,
    /// How online packages and indexes are downloaded (unavailable unless set).
    pub fetcher: Box<dyn crate::fetch::Fetcher + 'a>,
    /// How a config changed both on the card and on the car is settled (`--config=keep|card`);
    /// None leaves the conflict for the user.
    pub config_choice: Option<crate::config::Choice>,
}

impl<'a> Ctx<'a> {
    pub fn new(layout: Layout, sys: &'a dyn Sys) -> Ctx<'a> {
        let unit = detect(&layout);
        let mounts = Mounts::new(sys, &layout);
        Ctx { layout, sys, mounts, unit, progress: Box::new(|m: &str| crate::say!("{m}")), fetcher: Box::new(crate::fetch::Unavailable), config_choice: None }
    }
}

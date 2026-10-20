//! Per-invocation context: layout, side effects, unit identity, and the
//! read-write mount guard shared by everything that writes.
use crate::paths::Layout;
use crate::sys::Sys;
use crate::unit::{detect, UnitInfo};
use crate::rwhold;
use anyhow::{bail, Result};
use std::collections::{BTreeMap, BTreeSet};
use std::path::PathBuf;
use std::sync::{Mutex, MutexGuard};

/// Refcounted `mount -uw`: nested guards remount once; the last drop goes read-only — unless
/// another live process still holds the mount read-write (see `rwhold`).
///
/// One `Mounts` per process (the one in its `Ctx`): holder files are per pid, so two `Mounts`
/// in one process share a holder file and the first to release would withdraw it while the other
/// still writes. `Mounts` is not `Sync` (`&dyn Sys` isn't), so no threads share it today; the
/// counts sit behind a mutex only so that sharing stays correct if `Sys` ever becomes `Sync`.
///
/// Dropping a `Mounts` retries any release that failed, and if that fails too, withdraws this
/// process's holder files so a scoped `Ctx` never leaves a mount read-write on its account.
pub struct Mounts<'a> {
    sys: &'a dyn Sys,
    run_dir: PathBuf,
    holder: u32,
    state: Mutex<HoldState>,
}

#[derive(Default)]
struct HoldState {
    /// Guards alive per mount.
    counts: BTreeMap<String, u32>,
    /// Mounts whose last release failed: our holder file may still keep them read-write.
    unreleased: BTreeSet<String>,
}

/// Attempts at a release when the last guard drops (each may wait `RWLOCK_WAIT` for the lock).
const RELEASE_ATTEMPTS: u32 = 2;

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
        Mounts { sys, run_dir: rwhold::run_dir(layout), holder, state: Mutex::new(HoldState::default()) }
    }

    fn state(&self) -> MutexGuard<'_, HoldState> {
        self.state.lock().unwrap_or_else(|e| e.into_inner())
    }

    pub fn rw<'m>(&'m self, mount: &str) -> Result<RwGuard<'m, 'a>> {
        let mut st = self.state();
        let n = st.counts.get(mount).copied().unwrap_or(0);
        if n == 0 {
            rwhold::acquire_in(&self.run_dir, self.sys, mount, self.holder)?;
            // Held again: the coming release settles any earlier failed one.
            st.unreleased.remove(mount);
        }
        st.counts.insert(mount.to_string(), n + 1);
        Ok(RwGuard { mounts: self, mount: mount.to_string() })
    }

    /// Remount `mount` read-write again (something outside the protocol may have flipped it);
    /// only while this process holds it.
    pub fn reassert(&self, mount: &str) -> Result<()> {
        let st = self.state();
        if st.counts.get(mount).copied().unwrap_or(0) == 0 {
            bail!("{mount} is not held read-write by this process");
        }
        rwhold::reassert_in(&self.run_dir, self.sys, mount)
    }

    /// Retry the releases that failed when their last guard dropped (a long-lived process calls
    /// this so a mount does not stay read-write on its account). The first failure is returned;
    /// a mount that fails stays pending.
    pub fn retry_releases(&self) -> Result<()> {
        let mut st = self.state();
        let pending: Vec<String> = st.unreleased.iter().cloned().collect();
        let mut first_err = None;
        for mount in pending {
            match rwhold::release_in(&self.run_dir, self.sys, &mount, self.holder) {
                Ok(()) => {
                    st.unreleased.remove(&mount);
                }
                Err(e) => {
                    first_err.get_or_insert(e);
                }
            }
        }
        first_err.map_or(Ok(()), Err)
    }
}

impl Drop for RwGuard<'_, '_> {
    fn drop(&mut self) {
        let m = self.mounts;
        let mut st = m.state();
        let Some(n) = st.counts.get_mut(&self.mount) else { return };
        *n -= 1;
        if *n > 0 {
            return;
        }
        let mut err = None;
        for _ in 0..RELEASE_ATTEMPTS {
            match rwhold::release_in(&m.run_dir, m.sys, &self.mount, m.holder) {
                Ok(()) => return,
                Err(e) => err = Some(e),
            }
        }
        st.unreleased.insert(self.mount.clone());
        if let Some(e) = err {
            crate::warn!("warning: {} may stay read-write: release failed ({e:#}); will retry", self.mount);
        }
    }
}

impl Drop for Mounts<'_> {
    fn drop(&mut self) {
        if self.retry_releases().is_ok() {
            return;
        }
        let st = self.state();
        for mount in &st.unreleased {
            match rwhold::withdraw_in(&self.run_dir, mount, self.holder) {
                Ok(()) => crate::warn!("warning: {mount}: release still failing; withdrew this process's read-write hold without remounting (the next release or sweep goes read-only)"),
                Err(e) => crate::warn!("warning: {mount} may stay read-write: release failed and the hold could not be withdrawn ({e:#})"),
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
    /// `--force`: removing the SDK goes ahead despite pushed plugins or overrides in the ledger
    /// (with a loud warning).
    pub force: bool,
    /// `--approve=<perm,…>`: the permissions the user approved for this action's plugin installs;
    /// None (and anything it does not cover) asks instead of installing.
    pub approved: Option<Vec<String>>,
}

impl<'a> Ctx<'a> {
    pub fn new(layout: Layout, sys: &'a dyn Sys) -> Ctx<'a> {
        let unit = detect(&layout);
        let mounts = Mounts::new(sys, &layout);
        Ctx { layout, sys, mounts, unit, progress: Box::new(|m: &str| crate::say!("{m}")), fetcher: Box::new(crate::fetch::Unavailable), config_choice: None, force: false, approved: None }
    }
}

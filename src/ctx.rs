//! Per-invocation context: layout, side effects, unit identity, and the
//! read-write mount guard shared by everything that writes.
use crate::paths::Layout;
use crate::sys::Sys;
use crate::unit::{detect, UnitInfo};
use anyhow::Result;
use std::cell::RefCell;
use std::collections::BTreeMap;

/// Refcounted `mount -uw`: nested guards remount once; the last drop goes read-only.
pub struct Mounts<'a> {
    sys: &'a dyn Sys,
    counts: RefCell<BTreeMap<String, u32>>,
}

pub struct RwGuard<'m, 'a> {
    mounts: &'m Mounts<'a>,
    mount: String,
}

impl<'a> Mounts<'a> {
    pub fn new(sys: &'a dyn Sys) -> Mounts<'a> {
        Mounts { sys, counts: RefCell::new(BTreeMap::new()) }
    }

    pub fn rw<'m>(&'m self, mount: &str) -> Result<RwGuard<'m, 'a>> {
        let mut c = self.counts.borrow_mut();
        let n = c.entry(mount.to_string()).or_insert(0);
        if *n == 0 {
            self.sys.remount(mount, true)?;
        }
        *n += 1;
        Ok(RwGuard { mounts: self, mount: mount.to_string() })
    }
}

impl Drop for RwGuard<'_, '_> {
    fn drop(&mut self) {
        let mut c = self.mounts.counts.borrow_mut();
        if let Some(n) = c.get_mut(&self.mount) {
            *n -= 1;
            if *n == 0 {
                if let Err(e) = self.mounts.sys.remount(&self.mount, false) {
                    eprintln!("warning: {e:#}");
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
}

impl<'a> Ctx<'a> {
    pub fn new(layout: Layout, sys: &'a dyn Sys) -> Ctx<'a> {
        let unit = detect(&layout);
        Ctx { layout, sys, mounts: Mounts::new(sys), unit }
    }
}

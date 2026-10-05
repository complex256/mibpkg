//! The web interface (spec 2026-10-05-web-interface-design.md): `mibpkg web`, started by the
//! supervisor. Reads are served in-process; every write runs `mibpkg gem <verb>` under the lock.
pub mod actions;
pub mod api;
pub mod http;
pub mod jobs;
pub mod net;
pub mod settings;

use crate::paths::Layout;
use crate::sys::{RealSys, Sys};
use crate::unit::UnitInfo;
use net::Allowed;
use settings::WebSettings;
use std::path::Path;
use std::sync::atomic::AtomicU16;

/// Everything a request handler needs, shared by all connection threads.
pub struct App {
    pub layout: Layout,
    pub unit: UnitInfo,
    /// Read once at start: changing settings restarts the web process.
    pub settings: WebSettings,
    /// Why web.toml was ignored, if it was.
    pub settings_warning: Option<String>,
    pub sys: RealSys,
    pub allowed: Allowed,
    /// The port actually bound (0 until bound).
    pub port: AtomicU16,
}

impl App {
    /// `probe` returns `ifconfig` output (`Allowed::ifconfig_probe()` on the unit).
    pub fn new(layout: Layout, probe: Box<dyn Fn() -> Option<String> + Send + Sync>) -> App {
        let (settings, settings_warning) = settings::load(&layout);
        let unit = crate::unit::detect(&layout);
        let allowed = Allowed::new(&settings, probe);
        let sys = RealSys { mounts: layout.root == Path::new("/") };
        App { layout, unit, settings, settings_warning, sys, allowed, port: AtomicU16::new(0) }
    }

    /// Whether a mibpkg action (from GEM, the CLI or this server) holds the action lock.
    pub fn lock_busy(&self) -> bool {
        std::fs::read_to_string(self.layout.run("lock"))
            .ok()
            .and_then(|s| s.trim().parse::<u32>().ok())
            .map_or(false, |pid| self.sys.process_alive(pid))
    }
}

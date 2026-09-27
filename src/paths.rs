//! Every path mibpkg touches, as seen on the unit, mapped under a root so the
//! whole engine runs against a temp dir in tests (root "/" on the unit).
use anyhow::{Context, Result};
use serde::Deserialize;
use std::path::{Path, PathBuf};

pub const HOME: &str = "/mnt/app/root/mibpkg";

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields, default)]
pub struct Settings {
    /// Package store. v1 keeps it on /mnt/app; a setting so SD storage can come later.
    pub pkgs_dir: String,
    /// Where SD cards / USB sticks are mounted, checked in order.
    pub media_roots: Vec<String>,
    /// GEM parent screen for the mibpkg entry.
    pub gem_parent: String,
    /// Overrides unit detection (e.g. "MHI2Q").
    pub unit: Option<String>,
    /// Alias overrides for start conditions, e.g. `hmi-ready = "file:/tmp/startupFinishedFlag"`.
    pub conditions: std::collections::BTreeMap<String, String>,
}

impl Default for Settings {
    fn default() -> Self {
        Settings {
            pkgs_dir: format!("{HOME}/pkgs"),
            media_roots: ["/fs/sda0", "/fs/sdb0", "/fs/sd0", "/fs/sd1", "/fs/usb0"]
                .iter()
                .map(|s| s.to_string())
                .collect(),
            gem_parent: "Main".into(),
            unit: None,
            conditions: Default::default(),
        }
    }
}

#[derive(Debug, Clone)]
pub struct Layout {
    pub root: PathBuf,
    pub settings: Settings,
}

impl Layout {
    pub fn load(root: impl Into<PathBuf>) -> Result<Layout> {
        let mut l = Layout { root: root.into(), settings: Settings::default() };
        let f = l.home().join("settings.toml");
        if f.exists() {
            let text = std::fs::read_to_string(&f).with_context(|| format!("reading {}", f.display()))?;
            l.settings = toml::from_str(&text).with_context(|| format!("parsing {}", f.display()))?;
            crate::conditions::validate_overrides(&l.settings)
                .with_context(|| format!("checking {}", f.display()))?;
        }
        Ok(l)
    }

    /// Map an absolute on-unit path under the root.
    pub fn p(&self, abs: &str) -> PathBuf {
        self.root.join(abs.trim_start_matches('/'))
    }
    pub fn home(&self) -> PathBuf {
        self.p(HOME)
    }
    pub fn bin_dir(&self) -> PathBuf {
        self.home().join("bin")
    }
    pub fn boot_d(&self) -> PathBuf {
        self.home().join("boot.d")
    }
    pub fn gem_dir(&self) -> PathBuf {
        self.home().join("gem")
    }
    pub fn state_dir(&self) -> PathBuf {
        self.home().join("state")
    }
    pub fn config_dir(&self, pkg: &str) -> PathBuf {
        self.state_dir().join("config").join(pkg)
    }
    pub fn pkgs_dir(&self) -> PathBuf {
        self.p(&self.settings.pkgs_dir)
    }
    pub fn pkg_dir(&self, pkg: &str) -> PathBuf {
        self.pkgs_dir().join(pkg)
    }
    pub fn engdefs(&self) -> PathBuf {
        self.p("/mnt/app/eso/hmi/engdefs")
    }
    pub fn startup_sh(&self) -> PathBuf {
        self.p("/mnt/system/etc/boot/startup.sh")
    }
    /// Flat runtime file in /tmp (the unit's /tmp cannot hold directories).
    pub fn run(&self, name: &str) -> PathBuf {
        self.p("/tmp").join(format!("mibpkg.{name}"))
    }
    pub fn tmp_bin(&self) -> PathBuf {
        self.p("/tmp/mibpkg")
    }
    /// The on-unit absolute path of a path under this layout.
    pub fn unit_path(&self, p: &Path) -> String {
        match p.strip_prefix(&self.root) {
            Ok(rel) => format!("/{}", rel.display()),
            Err(_) => p.display().to_string(),
        }
    }
}

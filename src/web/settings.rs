//! <home>/web.toml: the web interface's own settings. Written only by mibpkg (the `web-settings`
//! verb, under the action lock); settings.toml stays hand-owned.
use crate::fsutil::atomic_write;
use crate::paths::Layout;
use anyhow::{bail, Result};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::PathBuf;

/// Names allowed in `listen`.
pub const LISTEN_NAMES: &[&str] = &["hotspot", "tailscale", "any"];

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, default)]
pub struct WebSettings {
    pub enabled: bool,
    /// 0 = any free port (tests and development).
    pub port: u16,
    /// Interface classes the server answers on: "hotspot", "tailscale", or "any" alone.
    pub listen: Vec<String>,
    /// "<salt hex>:<sha-256 hex>" (see `hash_pin`); absent = no PIN.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pin: Option<String>,
    /// Overrides which interfaces belong to a class, e.g. `hotspot = ["uap0"]`.
    #[serde(skip_serializing_if = "BTreeMap::is_empty")]
    pub interfaces: BTreeMap<String, Vec<String>>,
}

impl Default for WebSettings {
    fn default() -> Self {
        WebSettings {
            enabled: true,
            port: 80,
            listen: vec!["hotspot".into(), "tailscale".into()],
            pin: None,
            interfaces: BTreeMap::new(),
        }
    }
}

fn is_hex(s: &str, len: usize) -> bool {
    s.len() == len && s.bytes().all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}

/// A server-generated id (uploads, text files): exactly 32 lowercase hex characters.
pub fn is_id(s: &str) -> bool {
    is_hex(s, 32)
}

impl WebSettings {
    pub fn validate(&self) -> Result<()> {
        if self.listen.is_empty() {
            bail!("listen on at least one of: hotspot, tailscale, any");
        }
        for l in &self.listen {
            if !LISTEN_NAMES.contains(&l.as_str()) {
                bail!("unknown listen value {l:?} (hotspot, tailscale or any)");
            }
        }
        if self.listen.iter().any(|l| l == "any") && self.listen.len() > 1 {
            bail!("\"any\" already covers every interface; use it alone");
        }
        if let Some(p) = &self.pin {
            let ok = p.split_once(':').map_or(false, |(salt, hash)| is_hex(salt, 32) && is_hex(hash, 64));
            if !ok {
                bail!("stored PIN is not in the expected format");
            }
        }
        for (class, names) in &self.interfaces {
            if class != "hotspot" && class != "tailscale" {
                bail!("interfaces: unknown class {class:?} (hotspot or tailscale)");
            }
            for n in names {
                if n.is_empty() || n.len() > 15 || !n.bytes().all(|b| b.is_ascii_lowercase() || b.is_ascii_digit()) {
                    bail!("interfaces: {n:?} is not an interface name");
                }
            }
        }
        Ok(())
    }

    pub fn any(&self) -> bool {
        self.listen.iter().any(|l| l == "any")
    }
}

pub fn path(layout: &Layout) -> PathBuf {
    layout.home().join("web.toml")
}

/// The settings, or the defaults plus a warning when the file is unreadable: the web interface
/// must always come up, since it may be the only way to fix things.
pub fn load(layout: &Layout) -> (WebSettings, Option<String>) {
    let p = path(layout);
    let text = match std::fs::read_to_string(&p) {
        Ok(t) => t,
        Err(_) => return (WebSettings::default(), None),
    };
    match toml::from_str::<WebSettings>(&text).map_err(anyhow::Error::from).and_then(|s| s.validate().map(|_| s)) {
        Ok(s) => (s, None),
        Err(e) => (WebSettings::default(), Some(format!("{}: {e:#}; using the defaults", p.display()))),
    }
}

/// Validate and write web.toml. Caller holds /mnt/app rw (and the action lock).
pub fn save(layout: &Layout, s: &WebSettings) -> Result<()> {
    s.validate()?;
    atomic_write(&path(layout), toml::to_string(s)?.as_bytes())?;
    Ok(())
}

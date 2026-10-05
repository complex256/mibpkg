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
        Err(e) => {
            // Someone who set a PIN expects it to survive a typo elsewhere in the file (or a field
            // a newer mibpkg wrote): keep a well-formed PIN even when everything else falls back.
            let mut defaults = WebSettings::default();
            let pin = toml::from_str::<toml::Value>(&text)
                .ok()
                .and_then(|v| v.get("pin").and_then(|p| p.as_str()).map(String::from))
                .filter(|p| WebSettings { pin: Some(p.clone()), ..WebSettings::default() }.validate().is_ok());
            let kept = if pin.is_some() { " (kept the PIN)" } else { "" };
            defaults.pin = pin;
            (defaults, Some(format!("{}: {e:#}; using the defaults{kept}", p.display())))
        }
    }
}

/// Validate and write web.toml. Caller holds /mnt/app rw (and the action lock).
pub fn save(layout: &Layout, s: &WebSettings) -> Result<()> {
    s.validate()?;
    atomic_write(&path(layout), toml::to_string(s)?.as_bytes())?;
    Ok(())
}

// ---------------------------------------------------------------------------------------------
// Randomness, PIN hashing
// ---------------------------------------------------------------------------------------------

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

/// 16 random bytes as 32 hex characters: upload ids, session tokens, PIN salts.
/// Reads /dev/urandom; if the unit has none, falls back to hashing the clock, the pid, a counter
/// and a stack address (unpredictable enough for a LAN-only PIN session, logged once).
pub fn random_id() -> String {
    use std::io::Read;
    let mut buf = [0u8; 16];
    let from_device = std::fs::File::open("/dev/urandom").and_then(|mut f| f.read_exact(&mut buf)).is_ok();
    if from_device {
        return hex(&buf);
    }
    use sha2::{Digest, Sha256};
    use std::sync::atomic::{AtomicU64, Ordering};
    static COUNTER: AtomicU64 = AtomicU64::new(0);
    static WARNED: std::sync::Once = std::sync::Once::new();
    WARNED.call_once(|| eprintln!("mibpkg web: /dev/urandom unavailable; using weaker randomness"));
    let nanos = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_nanos()).unwrap_or(0);
    let local = 0u8;
    let mut h = Sha256::new();
    h.update(nanos.to_le_bytes());
    h.update(std::process::id().to_le_bytes());
    h.update(COUNTER.fetch_add(1, Ordering::Relaxed).to_le_bytes());
    h.update((&local as *const u8 as usize).to_le_bytes());
    h.update(format!("{:?}", std::time::Instant::now()).as_bytes());
    hex(&h.finalize()[..16])
}

fn pin_digest(salt: &str, pin: &str) -> String {
    use sha2::{Digest, Sha256};
    let mut h = Sha256::new();
    h.update(salt.as_bytes());
    h.update(pin.as_bytes());
    hex(&h.finalize())
}

/// 4 to 12 digits.
pub fn valid_pin(pin: &str) -> bool {
    (4..=12).contains(&pin.len()) && pin.bytes().all(|b| b.is_ascii_digit())
}

/// "<salt>:<sha-256(salt + pin)>" with a fresh random salt.
pub fn hash_pin(pin: &str) -> String {
    let salt = random_id();
    let digest = pin_digest(&salt, pin);
    format!("{salt}:{digest}")
}

/// Whether `pin` matches a stored `hash_pin` value. Compares every byte (no early exit).
pub fn check_pin(stored: &str, pin: &str) -> bool {
    let Some((salt, want)) = stored.split_once(':') else { return false };
    let got = pin_digest(salt, pin);
    if got.len() != want.len() {
        return false;
    }
    got.bytes().zip(want.bytes()).fold(0u8, |acc, (a, b)| acc | (a ^ b)) == 0
}

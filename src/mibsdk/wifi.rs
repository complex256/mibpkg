//! The core Wi-Fi uplink config the SDK owns: `/mnt/persist/mibsdk/wifi.toml`.
//!
//! The SDK's own Plugins → SDK → Internet page and this web UI edit the same file. The SDK reads it
//! every second, so a save here applies without a restart. Passwords never leave the unit: the web
//! API returns whether one is set, never the value.
//!
//! Parsing and writing use the same `toml` crate as the package manifests: one typed struct, no
//! hand-rolled key/value splitting.
use crate::fsutil;
use anyhow::{bail, Context, Result};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::PathBuf;

use super::PERSIST;

/// The shared config file.
pub fn config_path() -> PathBuf {
    PathBuf::from(PERSIST).join("wifi.toml")
}

/// What the SDK runtime writes for this page (`/tmp/mibpkg.wifi.status`).
pub fn status_path() -> PathBuf {
    PathBuf::from("/tmp/mibpkg.wifi.status")
}

/// The values in `wifi.toml`. Missing keys use [`Default`], so a hand-written file may set only what
/// it needs; `#[serde(default)]` also keeps `dhcp` on when the key is absent.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Config {
    pub enabled: bool,
    pub ssid: String,
    pub password: String,
    /// True: DHCP. False: the static settings below.
    pub dhcp: bool,
    pub ip: String,
    pub netmask: String,
    pub gateway: String,
    /// Comma-separated DNS servers; empty keeps the car's resolver.
    pub dns: String,
}

impl Default for Config {
    fn default() -> Self {
        Config {
            enabled: false,
            ssid: String::new(),
            password: String::new(),
            dhcp: true,
            ip: String::new(),
            netmask: String::new(),
            gateway: String::new(),
            dns: String::new(),
        }
    }
}

/// The runtime's last reported state.
#[derive(Debug, Clone, Default)]
pub struct Status {
    pub enabled: bool,
    pub state: String,
    pub ssid: String,
    pub ipv4: String,
    pub netmask: String,
    pub gateway: String,
    pub dns: String,
    pub error: String,
    pub password_set: bool,
}

impl Status {
    pub fn connected(&self) -> bool {
        self.state == "connected"
    }
}

/// Read `wifi.toml`; a missing file is "not set up yet" (not an error).
pub fn load() -> Result<Option<Config>> {
    let path = config_path();
    let text = match fs::read_to_string(&path) {
        Ok(t) => t,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(e) => return Err(e).with_context(|| format!("cannot read {}", path.display())),
    };
    toml::from_str(&text)
        .map(Some)
        .with_context(|| format!("invalid {}", path.display()))
}

/// Read the runtime's status file; a missing file means the SDK has not run since boot.
pub fn status() -> Option<Status> {
    let text = fs::read_to_string(status_path()).ok()?;
    let mut m = BTreeMap::new();
    for line in text.lines() {
        if let Some((k, v)) = line.split_once('=') {
            m.insert(k.trim().to_string(), v.trim().to_string());
        }
    }
    Some(Status {
        enabled: m.get("enabled").map(|v| v == "true").unwrap_or(false),
        state: m.get("state").cloned().unwrap_or_default(),
        ssid: m.get("ssid").cloned().unwrap_or_default(),
        ipv4: m.get("ipv4").cloned().unwrap_or_default(),
        netmask: m.get("netmask").cloned().unwrap_or_default(),
        gateway: m.get("gateway").cloned().unwrap_or_default(),
        dns: m.get("dns").cloned().unwrap_or_default(),
        error: m.get("error").cloned().unwrap_or_default(),
        password_set: m.get("password_set").map(|v| v == "true").unwrap_or(false),
    })
}

/// The header kept above the serialised TOML, so a human opening the file knows what it is.
const HEADER: &str = "# mibsdk Wi-Fi uplink. Edited here or by the car's Plugins menu; the SDK applies changes within a few seconds.\n";

/// Write the config. `password` is the new value; `None` keeps the saved one (the web form leaves
/// the field blank to keep it).
pub fn save(
    enabled: bool,
    ssid: &str,
    password: Option<&str>,
    dhcp: bool,
    ip: &str,
    netmask: &str,
    gateway: &str,
    dns: &str,
) -> Result<Config> {
    let ssid = ssid.trim();
    if ssid.len() > 32 {
        bail!("the network name is at most 32 characters");
    }
    if ssid.contains('\n') || ssid.contains('\r') {
        bail!("the network name cannot contain a line break");
    }
    if let Some(p) = password {
        if p.len() > 63 {
            bail!("the password is at most 63 characters");
        }
        if p.contains('\n') || p.contains('\r') {
            bail!("the password cannot contain a line break");
        }
    }
    let ip = ip.trim();
    let netmask = netmask.trim();
    let gateway = gateway.trim();
    let dns = dns.trim();
    for (label, value) in [("IP address", ip), ("netmask", netmask), ("gateway", gateway)] {
        if !value.is_empty() && !valid_ipv4(value) {
            bail!("{label} {value:?} is not an IPv4 address");
        }
    }
    if !dns.is_empty() {
        for part in dns.split(',') {
            let part = part.trim();
            if !part.is_empty() && !valid_ipv4(part) {
                bail!("DNS server {part:?} is not an IPv4 address");
            }
        }
    }
    if !dhcp && ip.is_empty() {
        bail!("manual addressing needs an IP address");
    }
    if !dhcp && netmask.is_empty() {
        bail!("manual addressing needs a netmask");
    }
    let old = load()?.unwrap_or_default();
    let password = match password {
        Some(p) => p.to_string(),
        None if old.ssid == ssid => old.password,
        None => String::new(),
    };
    let c = Config {
        enabled,
        ssid: ssid.to_string(),
        password,
        dhcp,
        ip: ip.to_string(),
        netmask: netmask.to_string(),
        gateway: gateway.to_string(),
        dns: dns.to_string(),
    };
    let body = toml::to_string(&c).context("cannot serialise wifi.toml")?;
    let path = config_path();
    fs::create_dir_all(PERSIST).with_context(|| format!("cannot create {PERSIST}"))?;
    fsutil::atomic_write(&path, format!("{HEADER}{body}").as_bytes())
        .with_context(|| format!("cannot write {}", path.display()))?;
    // The file holds a Wi-Fi passphrase; never leave it world-readable.
    let _ = fs::set_permissions(&path, fs::Permissions::from_mode(0o600));
    Ok(c)
}

/// True when the SDK's config file exists (so the SDK has run at least once on this unit).
pub fn configured() -> bool {
    config_path().is_file()
}

/// Four decimal octets, 0..255.
pub fn valid_ipv4(s: &str) -> bool {
    let parts: Vec<&str> = s.split('.').collect();
    parts.len() == 4
        && parts.iter().all(|p| {
            !p.is_empty()
                && p.len() <= 3
                && p.bytes().all(|b| b.is_ascii_digit())
                && p.parse::<u16>().map(|n| n <= 255).unwrap_or(false)
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ipv4_validation() {
        for good in ["0.0.0.0", "10.20.0.23", "255.255.255.0", "1.2.3.4"] {
            assert!(valid_ipv4(good), "{good}");
        }
        for bad in ["", "1.2.3", "1.2.3.4.5", "256.1.1.1", "a.b.c.d", "1.2.3.", " 1.2.3.4"] {
            assert!(!valid_ipv4(bad), "{bad}");
        }
    }

    #[test]
    fn a_hand_written_file_uses_defaults_for_missing_keys() {
        let c: Config = toml::from_str("# only what matters\nssid = \"beepboop\"\nenabled = true\n").unwrap();
        assert!(c.enabled);
        assert_eq!("beepboop", c.ssid);
        assert!(c.dhcp, "dhcp defaults on");
        assert_eq!("", c.ip);
    }

    #[test]
    fn unknown_keys_and_bad_types_are_errors() {
        assert!(toml::from_str::<Config>("enabeld = true\n").is_err());
        assert!(toml::from_str::<Config>("enabled = \"yes\"\n").is_err());
    }

    #[test]
    fn a_full_config_round_trips() {
        let c = Config {
            enabled: true,
            ssid: "a\"b\\c".into(),
            password: "test-password".into(),
            dhcp: false,
            ip: "192.168.1.50".into(),
            netmask: "255.255.255.0".into(),
            gateway: "192.168.1.1".into(),
            dns: "1.1.1.1,8.8.8.8".into(),
        };
        let text = toml::to_string(&c).unwrap();
        let parsed: Config = toml::from_str(&text).unwrap();
        assert_eq!(c, parsed);
    }
}

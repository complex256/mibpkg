//! Start conditions: when a service may start (spec 2026-09-26-start-conditions-design.md).
//! Entries are aliases (`internet`, `tailnet`, `hmi-ready`) or raw checks
//! (`file:<abs path>`, `iface:<name>`, `route:<dest>[@<iface prefix>]`).
use crate::paths::Settings;
use anyhow::{bail, Context, Result};
use std::path::PathBuf;

#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Condition {
    File(PathBuf),
    Iface(String),
    Route { dest: String, iface: Option<String> },
}

pub const ALIASES: &[&str] = &["internet", "tailnet", "hmi-ready"];

/// Built-in alias meanings; `settings.toml` `[conditions]` overrides them per unit.
pub const DEFAULTS: &[(&str, &str)] = &[
    ("internet", "route:default"),
    ("tailnet", "route:100.64@tun"),
    ("hmi-ready", "file:/tmp/smFlagStartupDone"),
];

fn valid_iface(s: &str) -> bool {
    !s.is_empty() && s.len() <= 15 && s.bytes().all(|b| b.is_ascii_lowercase() || b.is_ascii_digit())
}

pub fn parse_raw(s: &str) -> Result<Condition> {
    if let Some(p) = s.strip_prefix("file:") {
        if !p.starts_with('/') || p.len() < 2 {
            bail!("condition {s:?}: file: needs an absolute path");
        }
        return Ok(Condition::File(PathBuf::from(p)));
    }
    if let Some(n) = s.strip_prefix("iface:") {
        if !valid_iface(n) {
            bail!("condition {s:?}: interface name must be 1-15 of [a-z0-9]");
        }
        return Ok(Condition::Iface(n.to_string()));
    }
    if let Some(r) = s.strip_prefix("route:") {
        let (dest, iface) = match r.split_once('@') {
            Some((d, i)) => (d, Some(i)),
            None => (r, None),
        };
        if dest.is_empty() || dest.contains(char::is_whitespace) {
            bail!("condition {s:?}: route: needs a destination");
        }
        if let Some(i) = iface {
            if !valid_iface(i) {
                bail!("condition {s:?}: interface prefix must be 1-15 of [a-z0-9]");
            }
        }
        return Ok(Condition::Route { dest: dest.to_string(), iface: iface.map(String::from) });
    }
    bail!("condition {s:?} is not file:, iface: or route:")
}

/// Accept an entry from a manifest: a known alias or a valid raw check.
pub fn validate(entry: &str) -> Result<()> {
    if ALIASES.contains(&entry) {
        return Ok(());
    }
    let known_prefix = ["file:", "iface:", "route:"].iter().any(|p| entry.starts_with(p));
    if !known_prefix {
        bail!("condition {entry:?} requires a newer mibpkg");
    }
    parse_raw(entry).map(|_| ())
}

/// Resolve an entry to a raw check, applying settings overrides for aliases.
pub fn resolve(entry: &str, settings: &Settings) -> Result<Condition> {
    if ALIASES.contains(&entry) {
        let raw = match settings.conditions.get(entry) {
            Some(v) => v.as_str(),
            None => DEFAULTS.iter().find(|(k, _)| *k == entry).map(|(_, v)| *v).unwrap(),
        };
        return parse_raw(raw).with_context(|| format!("alias {entry}"));
    }
    parse_raw(entry)
}

/// Check `[conditions]` overrides from settings.toml.
pub fn validate_overrides(settings: &Settings) -> Result<()> {
    for (k, v) in &settings.conditions {
        if !ALIASES.contains(&k.as_str()) {
            bail!("settings.toml [conditions]: unknown alias {k:?} (known: {})", ALIASES.join(", "));
        }
        parse_raw(v).with_context(|| format!("settings.toml [conditions] {k}"))?;
    }
    Ok(())
}

/// "30s" / "5m" / "2h" -> seconds, 1 s .. 24 h.
pub fn parse_timeout(s: &str) -> Result<u64> {
    let (num, unit) = s.split_at(s.len().saturating_sub(1));
    let n: u64 = num.parse().with_context(|| format!("timeout {s:?}: expected like 30s, 5m or 2h"))?;
    let secs = match unit {
        "s" => n,
        "m" => n * 60,
        "h" => n * 3600,
        _ => bail!("timeout {s:?}: unit must be s, m or h"),
    };
    if secs == 0 || secs > 24 * 3600 {
        bail!("timeout {s:?}: must be between 1s and 24h");
    }
    Ok(secs)
}

/// 45 -> "45s", 130 -> "2m10s", 3780 -> "1h3m".
pub fn fmt_secs(s: u64) -> String {
    if s < 60 {
        format!("{s}s")
    } else if s < 3600 {
        if s % 60 == 0 { format!("{}m", s / 60) } else { format!("{}m{}s", s / 60, s % 60) }
    } else if s % 3600 / 60 == 0 {
        format!("{}h", s / 3600)
    } else {
        format!("{}h{}m", s / 3600, s % 3600 / 60)
    }
}

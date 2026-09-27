//! Start conditions: when a service may start (spec 2026-09-26-start-conditions-design.md).
//! Entries are aliases (`internet`, `tailnet`, `hmi-ready`) or raw checks
//! (`file:<abs path>`, `iface:<name>`, `route:<dest>[@<iface prefix>]`).
use crate::paths::Settings;
use anyhow::{bail, Context, Result};
use std::cell::RefCell;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

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

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RouteEntry {
    pub dest: String,
    pub iface: String,
}

/// BSD-style `netstat -rn`: data rows are "<dest> <gateway> <flags> ... <interface>".
pub fn parse_netstat_rn(text: &str) -> Vec<RouteEntry> {
    text.lines()
        .filter_map(|l| {
            let f: Vec<&str> = l.split_whitespace().collect();
            if f.len() < 4 || f[0] == "Destination" || f[0].ends_with(':') || f[0] == "Routing" {
                return None;
            }
            Some(RouteEntry { dest: f[0].to_string(), iface: f[f.len() - 1].to_string() })
        })
        .collect()
}

/// `ifconfig <name>`: up = flags include UP and RUNNING, and an inet address is set.
pub fn parse_ifconfig_up(text: &str) -> bool {
    let flags = text
        .lines()
        .next()
        .and_then(|l| l.split_once('<'))
        .and_then(|(_, r)| r.split_once('>'))
        .map(|(f, _)| f);
    let has = |w: &str| flags.map_or(false, |f| f.split(',').any(|x| x == w));
    has("UP") && has("RUNNING") && text.lines().any(|l| l.trim_start().starts_with("inet "))
}

pub trait SystemView {
    fn file_exists(&self, p: &Path) -> bool;
    fn iface_up(&self, name: &str) -> Result<bool, String>;
    fn routes(&self) -> Result<Vec<RouteEntry>, String>;
}

/// Not on the unit's PATH.
pub const NETSTAT: &str = "/mnt/app/armle/usr/bin/netstat";

#[derive(Default)]
pub struct RealView;

impl SystemView for RealView {
    fn file_exists(&self, p: &Path) -> bool {
        p.exists()
    }
    fn iface_up(&self, name: &str) -> Result<bool, String> {
        let out = Command::new("ifconfig")
            .arg(name)
            .stderr(Stdio::null())
            .output()
            .map_err(|e| format!("ifconfig: {e}"))?;
        // A missing interface makes ifconfig exit non-zero: that is "not up", not an error.
        Ok(out.status.success() && parse_ifconfig_up(&String::from_utf8_lossy(&out.stdout)))
    }
    fn routes(&self) -> Result<Vec<RouteEntry>, String> {
        let out = Command::new(NETSTAT)
            .arg("-rn")
            .stderr(Stdio::null())
            .output()
            .map_err(|e| format!("netstat: {e}"))?;
        if !out.status.success() {
            return Err(format!("netstat exited with {}", out.status));
        }
        Ok(parse_netstat_rn(&String::from_utf8_lossy(&out.stdout)))
    }
}

/// Memoizes system reads for one supervisor tick, so N waiting services cost one netstat.
pub struct TickCache<'a> {
    view: &'a dyn SystemView,
    routes: RefCell<Option<Result<Vec<RouteEntry>, String>>>,
    ifaces: RefCell<HashMap<String, Result<bool, String>>>,
}

impl<'a> TickCache<'a> {
    pub fn new(view: &'a dyn SystemView) -> TickCache<'a> {
        TickCache { view, routes: RefCell::new(None), ifaces: RefCell::new(HashMap::new()) }
    }

    pub fn holds(&self, c: &Condition) -> Result<bool, String> {
        match c {
            Condition::File(p) => Ok(self.view.file_exists(p)),
            Condition::Iface(n) => self.ifaces.borrow_mut().entry(n.clone()).or_insert_with(|| self.view.iface_up(n)).clone(),
            Condition::Route { dest, iface } => {
                let mut cached = self.routes.borrow_mut();
                let routes = cached.get_or_insert_with(|| self.view.routes());
                let routes = routes.as_ref().map_err(|e| e.clone())?;
                Ok(routes.iter().any(|r| {
                    (r.dest == *dest || r.dest.starts_with(&format!("{dest}/")))
                        && iface.as_ref().map_or(true, |i| r.iface.starts_with(i.as_str()))
                }))
            }
        }
    }
}

/// The entries of `after` that do not hold now, as written, with the reason appended when a
/// check could not be evaluated or the entry cannot be resolved.
pub fn unmet(after: &[String], settings: &Settings, cache: &TickCache) -> Vec<String> {
    after
        .iter()
        .filter_map(|e| match resolve(e, settings) {
            Err(err) => Some(format!("{e} ({err:#})")),
            Ok(c) => match cache.holds(&c) {
                Ok(true) => None,
                Ok(false) => Some(e.clone()),
                Err(why) => Some(format!("{e} ({why})")),
            },
        })
        .collect()
}

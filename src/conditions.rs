//! Start conditions: when a service may start (spec 2026-09-26-start-conditions-design.md).
//! Entries are aliases (`internet`, `tailnet`, `hmi-ready`) or raw checks
//! (`file:<abs path>`, `iface:<name>`, `route:<dest>[@<iface prefix>]`).
use crate::paths::Settings;
use anyhow::{bail, Context, Result};
use std::cell::RefCell;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

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
    let unit = s.chars().last().with_context(|| format!("timeout {s:?}: expected like 30s, 5m or 2h"))?;
    let num = &s[..s.len() - unit.len_utf8()];
    if num.is_empty() || !num.bytes().all(|b| b.is_ascii_digit()) {
        bail!("timeout {s:?}: expected like 30s, 5m or 2h");
    }
    let mult: u64 = match unit {
        's' => 1,
        'm' => 60,
        'h' => 3600,
        _ => bail!("timeout {s:?}: unit must be s, m or h"),
    };
    let secs = num
        .parse::<u64>()
        .ok()
        .and_then(|n| n.checked_mul(mult))
        .with_context(|| format!("timeout {s:?}: too large"))?;
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

/// Reads the unit through `ifconfig` and `netstat`, each call bounded by `timeout` so a hung
/// command can never freeze the supervisor loop.
pub struct RealView {
    pub netstat: PathBuf,
    pub ifconfig: PathBuf,
    pub timeout: Duration,
}

impl Default for RealView {
    fn default() -> Self {
        RealView { netstat: PathBuf::from(NETSTAT), ifconfig: PathBuf::from("ifconfig"), timeout: Duration::from_secs(3) }
    }
}

/// Run `cmd arg` with a deadline: (exited successfully, stdout).
fn run_limited(cmd: &Path, arg: &str, timeout: Duration) -> Result<(bool, String), String> {
    let name = cmd.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
    let mut child = Command::new(cmd)
        .arg(arg)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|e| format!("{name}: {e}"))?;
    // Read on a thread so a full pipe can't stall the child while we wait on it.
    let mut out = child.stdout.take().unwrap();
    let reader = std::thread::spawn(move || {
        let mut s = String::new();
        let _ = std::io::Read::read_to_string(&mut out, &mut s);
        s
    });
    let end = Instant::now() + timeout;
    loop {
        match child.try_wait() {
            Ok(Some(st)) => return Ok((st.success(), reader.join().unwrap_or_default())),
            Ok(None) if Instant::now() < end => std::thread::sleep(Duration::from_millis(20)),
            Ok(None) => {
                let _ = child.kill();
                let _ = child.wait();
                return Err(format!("{name}: timed out after {}", fmt_secs(timeout.as_secs().max(1))));
            }
            Err(e) => return Err(format!("{name}: {e}")),
        }
    }
}

impl SystemView for RealView {
    fn file_exists(&self, p: &Path) -> bool {
        p.exists()
    }
    fn iface_up(&self, name: &str) -> Result<bool, String> {
        let (ok, out) = run_limited(&self.ifconfig, name, self.timeout)?;
        // A missing interface makes ifconfig exit non-zero: that is "not up", not an error.
        Ok(ok && parse_ifconfig_up(&out))
    }
    fn routes(&self) -> Result<Vec<RouteEntry>, String> {
        let (ok, out) = run_limited(&self.netstat, "-rn", self.timeout)?;
        if !ok {
            return Err("netstat exited with an error".into());
        }
        Ok(parse_netstat_rn(&out))
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

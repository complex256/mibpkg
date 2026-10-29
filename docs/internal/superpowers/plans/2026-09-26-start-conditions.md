# Start Conditions Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Services start only once their declared conditions (`internet`, `tailnet`, `hmi-ready`, `file:`, `iface:`, `route:`) hold, with an opt-in timeout, visible as `waiting: …` in status and GEM.

**Architecture:** A new `conditions` module parses condition strings, resolves the three aliases (built-in defaults, overridable in `settings.toml`), and evaluates raw checks through a `SystemView` trait (real: `stat`, `ifconfig`, `netstat -rn`; tests: a fake). The supervisor gates every start on `conditions::unmet`, tracks a wait clock and the opt-in timeout, and reports new status fields that a shared `status::describe` renders for the CLI and GEM.

**Tech Stack:** Rust 2021, std-only (as mibpkg v1); QNX 6.5 cross-build via `scripts/build.sh`.

**Spec:** `docs/superpowers/specs/2026-09-26-start-conditions-design.md`

## Global Constraints

- Std-only pure-Rust deps; no new crates. Cross-build must still pass (`scripts/build.sh`).
- `netstat` on the unit is `/mnt/app/armle/usr/bin/netstat` (not on PATH); `ifconfig` is on the PATH set by launch.sh/gem.sh.
- Unknown condition strings are refused with the exact phrase `requires a newer mibpkg`.
- Condition checks run once per supervisor tick (1 s), only for services that are waiting to start; each distinct raw check at most once per tick; `netstat -rn` at most once per tick.
- Existing boot.d files and manifests without `after` behave exactly as v1 (all new fields `#[serde(default)]`).
- Built-in alias defaults: `internet = "route:default"`, `tailnet = "route:100.64@tun"`, `hmi-ready = "file:/tmp/smFlagStartupDone"` (provisional until Task 6 measures the unit).
- Git author complex256 <me@complex256.dev>; commit trailer `Co-Authored-By: Claude Opus 5.5 (1M context) <noreply@anthropic.com>`; never reference the owner's real name.
- The mib2q-carmqtt working tree is checked out on another session's branch (`feat/mibpkg-package`, eso-comm spikes): never switch it; do carmqtt changes in a separate `git worktree` on `main`.

## Review Focus

1. **A check that can't run** (netstat missing, unparsable output) — service stays waiting and the reason is visible; nothing panics (test in Task 2).
2. **Condition met, then lost before start** — a service in backoff re-checks conditions at its next start rather than trusting an old result (test in Task 4).
3. **Settings override with a bad value or unknown alias key** — `Layout::load` fails with a clear error instead of services waiting forever on a typo (test in Task 1).
4. **`after_timeout` on a crash-looping service** — the timeout clock restarts per wait, and waiting never consumes the crash-loop budget (test in Task 4).
5. **Many services waiting on the same condition** — one `netstat` per tick, not one per service (test in Task 2).

---

## File Structure

```
src/conditions.rs      NEW: Condition, parse/validate/resolve, timeouts, output parsers, SystemView, RealView, TickCache, unmet
src/paths.rs           Settings.conditions (alias overrides) + validation in Layout::load
src/manifest.rs        ServiceEntry.after / after_timeout + validation
src/bootd.rs           BootEntry.after / after_timeout_secs
src/status.rs          SvcState::Waiting, SvcStatus fields, describe(), fmt_secs()
src/supervisor.rs      gating, wait clock, timeout note, view injection
src/cli.rs, src/gem.rs use status::describe
tests/conditions.rs, tests/fixtures/*.txt   NEW
../mib2q-carmqtt/packaging/manifest.toml.in (Task 7, via worktree)
```

---

### Task 1: Condition syntax, aliases, timeouts, settings overrides

**Files:**
- Create: `src/conditions.rs`
- Modify: `src/lib.rs` (add `pub mod conditions;`), `src/paths.rs` (Settings)
- Test: `tests/conditions.rs`

**Interfaces:**
- Produces: `conditions::{Condition, ALIASES, DEFAULTS, parse_raw(&str)->Result<Condition>, validate(&str)->Result<()>, resolve(&str, &Settings)->Result<Condition>, parse_timeout(&str)->Result<u64>, fmt_secs(u64)->String}`; `Settings.conditions: BTreeMap<String, String>` (overrides only; empty by default). `Condition` = `File(PathBuf) | Iface(String) | Route { dest: String, iface: Option<String> }`, derives `Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord`.

- [ ] **Step 1: Write the failing test `tests/conditions.rs`**

```rust
mod common;
use mibpkg::conditions::{fmt_secs, parse_raw, parse_timeout, resolve, validate, Condition};
use mibpkg::paths::{Layout, Settings};
use std::fs;
use std::path::PathBuf;

#[test]
fn raw_forms_parse() {
    assert_eq!(parse_raw("file:/tmp/x").unwrap(), Condition::File(PathBuf::from("/tmp/x")));
    assert_eq!(parse_raw("iface:tun9").unwrap(), Condition::Iface("tun9".into()));
    assert_eq!(parse_raw("route:default").unwrap(), Condition::Route { dest: "default".into(), iface: None });
    assert_eq!(parse_raw("route:100.64@tun").unwrap(), Condition::Route { dest: "100.64".into(), iface: Some("tun".into()) });
    for bad in ["file:rel/path", "iface:", "iface:BAD!", "iface:waytoolonginterfacename", "route:", "route:@tun", "nope:x", "tailnet"] {
        assert!(parse_raw(bad).is_err(), "{bad}");
    }
}

#[test]
fn validate_accepts_aliases_and_refuses_unknown_as_newer() {
    for ok in ["internet", "tailnet", "hmi-ready", "file:/tmp/a", "iface:ecm0", "route:default"] {
        validate(ok).unwrap();
    }
    let e = validate("service:tailscale.tailscaled").unwrap_err();
    assert!(format!("{e:#}").contains("requires a newer mibpkg"), "{e:#}");
    assert!(format!("{:#}", validate("gps-fix").unwrap_err()).contains("requires a newer mibpkg"));
}

#[test]
fn aliases_resolve_to_defaults_or_settings_override() {
    let s = Settings::default();
    assert_eq!(resolve("internet", &s).unwrap(), Condition::Route { dest: "default".into(), iface: None });
    assert_eq!(resolve("tailnet", &s).unwrap(), Condition::Route { dest: "100.64".into(), iface: Some("tun".into()) });
    assert_eq!(resolve("hmi-ready", &s).unwrap(), Condition::File(PathBuf::from("/tmp/smFlagStartupDone")));
    let mut s2 = Settings::default();
    s2.conditions.insert("hmi-ready".into(), "file:/tmp/startupFinishedFlag".into());
    assert_eq!(resolve("hmi-ready", &s2).unwrap(), Condition::File(PathBuf::from("/tmp/startupFinishedFlag")));
    assert_eq!(resolve("tailnet", &s2).unwrap(), Condition::Route { dest: "100.64".into(), iface: Some("tun".into()) }, "other aliases keep defaults");
    assert_eq!(resolve("iface:tun9", &s2).unwrap(), Condition::Iface("tun9".into()));
}

#[test]
fn settings_file_overrides_are_validated() {
    let f = common::fixture();
    fs::create_dir_all(f.layout.home()).unwrap();
    let p = f.layout.home().join("settings.toml");
    fs::write(&p, "[conditions]\nhmi-ready = \"file:/tmp/startupFinishedFlag\"\n").unwrap();
    let l = Layout::load(f.root()).unwrap();
    assert_eq!(l.settings.conditions["hmi-ready"], "file:/tmp/startupFinishedFlag");
    fs::write(&p, "[conditions]\ngps = \"file:/tmp/x\"\n").unwrap();
    assert!(format!("{:#}", Layout::load(f.root()).unwrap_err()).contains("gps"));
    fs::write(&p, "[conditions]\ntailnet = \"route:\"\n").unwrap();
    assert!(Layout::load(f.root()).is_err());
}

#[test]
fn timeouts_parse_and_format() {
    assert_eq!(parse_timeout("30s").unwrap(), 30);
    assert_eq!(parse_timeout("5m").unwrap(), 300);
    assert_eq!(parse_timeout("2h").unwrap(), 7200);
    for bad in ["", "5", "0s", "25h", "m", "5x", "-1m"] {
        assert!(parse_timeout(bad).is_err(), "{bad}");
    }
    assert_eq!(fmt_secs(45), "45s");
    assert_eq!(fmt_secs(130), "2m10s");
    assert_eq!(fmt_secs(3780), "1h3m");
}
```

- [ ] **Step 2: Run to verify it fails**

Run: `cargo test --test conditions`
Expected: compile error, `mibpkg::conditions` not found.

- [ ] **Step 3: Implement `src/conditions.rs` (syntax part)**

```rust
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
```

Note: `"-1m"` fails at `u64` parse; `"m"` has an empty number and fails parse.

- [ ] **Step 4: Add `conditions` to Settings and validate in `Layout::load`**

In `src/paths.rs`, add to `Settings` (after `unit`):
```rust
    /// Alias overrides for start conditions, e.g. `hmi-ready = "file:/tmp/startupFinishedFlag"`.
    pub conditions: std::collections::BTreeMap<String, String>,
```
and in `impl Default for Settings` add `conditions: Default::default(),`.
In `Layout::load`, after parsing the settings file (inside the `if f.exists()` block, after the `l.settings = ...` line):
```rust
            crate::conditions::validate_overrides(&l.settings)
                .with_context(|| format!("checking {}", f.display()))?;
```
Add `pub mod conditions;` to `src/lib.rs`.

- [ ] **Step 5: Run tests**

Run: `cargo test --test conditions && cargo test --test paths`
Expected: conditions 5 passed; paths unchanged and passing.

- [ ] **Step 6: Commit**

```bash
git add -A
git commit -m "feat: start condition syntax, aliases, settings overrides, timeouts

Co-Authored-By: Claude Opus 5.5 (1M context) <noreply@anthropic.com>"
```

---

### Task 2: Evaluating conditions (system view, output parsers, per-tick cache)

**Files:**
- Modify: `src/conditions.rs` (append)
- Create: `tests/fixtures/netstat-rn-sample.txt`, `tests/fixtures/ifconfig-tun9.txt`, `tests/fixtures/ifconfig-down.txt`
- Test: `tests/conditions.rs` (append)

**Interfaces:**
- Consumes: Task 1 `Condition`, `resolve`.
- Produces: `conditions::{RouteEntry { dest: String, iface: String }, parse_netstat_rn(&str)->Vec<RouteEntry>, parse_ifconfig_up(&str)->bool, SystemView (trait: file_exists(&Path)->bool, iface_up(&str)->Result<bool, String>, routes()->Result<Vec<RouteEntry>, String>), RealView (Default), TickCache<'a>::new(&'a dyn SystemView), TickCache::holds(&Condition)->Result<bool, String>, unmet(after:&[String], &Settings, &TickCache)->Vec<String>}`. `unmet` returns the unmet entries as written, with ` (<reason>)` appended when a check could not be evaluated.

- [ ] **Step 1: Write fixtures (representative BSD-style output; Task 6 adds real captures)**

`tests/fixtures/netstat-rn-sample.txt`:
```
Routing tables

Internet:
Destination        Gateway            Flags     Refs     Use    Mtu  Interface
default            203.0.113.61         UGS         0     1234      -  ecm0
10.255.0/24          link#2             U           2        0      -  mmx0
10.255.1/24      link#5             U           1       10      -  uap0
203.0.113.60/30      link#6             U           1        0      -  ecm0
100.64/10          100.64.0.55       UGS         0       77      -  tun9
127.0.0.1          127.0.0.1          UH          1        6  33192  lo0

Internet6:
Destination                        Gateway                        Flags     Refs     Use    Mtu  Interface
::1                                ::1                            UH          1        0  33192  lo0
```

`tests/fixtures/ifconfig-tun9.txt`:
```
tun9: flags=51<UP,POINTOPOINT,RUNNING> mtu 1280
	inet 100.64.0.55 -> 100.64.0.55 netmask 0xffffffff
```

`tests/fixtures/ifconfig-down.txt`:
```
ecm1: flags=8802<BROADCAST,SIMPLEX,MULTICAST> mtu 1500
```

- [ ] **Step 2: Write the failing tests (append to `tests/conditions.rs`)**

```rust
use mibpkg::conditions::{parse_ifconfig_up, parse_netstat_rn, unmet, RouteEntry, SystemView, TickCache};
use std::cell::Cell;
use std::path::Path;

fn fixture(name: &str) -> String {
    fs::read_to_string(Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures").join(name)).unwrap()
}

#[test]
fn netstat_and_ifconfig_parsers() {
    let r = parse_netstat_rn(&fixture("netstat-rn-sample.txt"));
    assert!(r.contains(&RouteEntry { dest: "default".into(), iface: "ecm0".into() }));
    assert!(r.contains(&RouteEntry { dest: "100.64/10".into(), iface: "tun9".into() }));
    assert!(!r.iter().any(|e| e.dest == "Destination" || e.dest.starts_with("Internet")));
    assert!(parse_ifconfig_up(&fixture("ifconfig-tun9.txt")));
    assert!(!parse_ifconfig_up(&fixture("ifconfig-down.txt")));
    assert!(!parse_ifconfig_up(""));
}

struct FakeView {
    files: Vec<&'static str>,
    up: Vec<&'static str>,
    routes: Result<Vec<RouteEntry>, String>,
    netstat_calls: Cell<u32>,
}

impl SystemView for FakeView {
    fn file_exists(&self, p: &Path) -> bool {
        self.files.iter().any(|f| Path::new(f) == p)
    }
    fn iface_up(&self, name: &str) -> Result<bool, String> {
        Ok(self.up.contains(&name))
    }
    fn routes(&self) -> Result<Vec<RouteEntry>, String> {
        self.netstat_calls.set(self.netstat_calls.get() + 1);
        self.routes.clone()
    }
}

fn view(routes: Result<Vec<RouteEntry>, String>) -> FakeView {
    FakeView { files: vec!["/tmp/smFlagStartupDone"], up: vec!["tun9"], routes, netstat_calls: Cell::new(0) }
}

fn rt(dest: &str, iface: &str) -> RouteEntry {
    RouteEntry { dest: dest.into(), iface: iface.into() }
}

#[test]
fn unmet_evaluates_aliases_and_raw_checks() {
    let s = Settings::default();
    let v = view(Ok(vec![rt("default", "ecm0"), rt("100.64/10", "tun9")]));
    let c = TickCache::new(&v);
    let all: Vec<String> = ["internet", "tailnet", "hmi-ready", "iface:tun9", "file:/tmp/smFlagStartupDone"].iter().map(|s| s.to_string()).collect();
    assert!(unmet(&all, &s, &c).is_empty());
    let v2 = view(Ok(vec![rt("default", "ecm0")])); // tailscale stuck: no tailnet route
    let c2 = TickCache::new(&v2);
    assert_eq!(unmet(&all, &s, &c2), vec!["tailnet"]);
    let v3 = view(Ok(vec![rt("100.64/10", "eth0")])); // right dest, wrong interface
    assert_eq!(unmet(&["tailnet".to_string()], &s, &TickCache::new(&v3)), vec!["tailnet"]);
    assert_eq!(unmet(&["iface:ecm1".to_string()], &s, &TickCache::new(&v)), vec!["iface:ecm1"]);
}

#[test]
fn a_check_that_cannot_run_is_unmet_with_reason() {
    let s = Settings::default();
    let v = view(Err("netstat: No such file or directory".into()));
    let got = unmet(&["internet".to_string()], &s, &TickCache::new(&v));
    assert_eq!(got, vec!["internet (netstat: No such file or directory)"]);
}

#[test]
fn one_netstat_per_tick_however_many_services() {
    let s = Settings::default();
    let v = view(Ok(vec![rt("default", "ecm0")]));
    let c = TickCache::new(&v);
    for _ in 0..5 {
        unmet(&["internet".to_string(), "tailnet".to_string()], &s, &c);
    }
    assert_eq!(v.netstat_calls.get(), 1);
}
```

- [ ] **Step 3: Run to verify it fails**

Run: `cargo test --test conditions`
Expected: compile error (items not found).

- [ ] **Step 4: Implement (append to `src/conditions.rs`)**

```rust
use std::cell::RefCell;
use std::collections::HashMap;
use std::path::Path;
use std::process::{Command, Stdio};

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
    let flags = text.lines().next().and_then(|l| l.split_once('<')).and_then(|(_, r)| r.split_once('>')).map(|(f, _)| f);
    let has = |w: &str| flags.map_or(false, |f| f.split(',').any(|x| x == w));
    has("UP") && has("RUNNING") && text.lines().any(|l| l.trim_start().starts_with("inet "))
}

pub trait SystemView {
    fn file_exists(&self, p: &Path) -> bool;
    fn iface_up(&self, name: &str) -> Result<bool, String>;
    fn routes(&self) -> Result<Vec<RouteEntry>, String>;
}

pub const NETSTAT: &str = "/mnt/app/armle/usr/bin/netstat";

#[derive(Default)]
pub struct RealView;

impl SystemView for RealView {
    fn file_exists(&self, p: &Path) -> bool {
        p.exists()
    }
    fn iface_up(&self, name: &str) -> Result<bool, String> {
        let out = Command::new("ifconfig").arg(name).stderr(Stdio::null()).output().map_err(|e| format!("ifconfig: {e}"))?;
        // A missing interface makes ifconfig exit non-zero: that is "not up", not an error.
        Ok(out.status.success() && parse_ifconfig_up(&String::from_utf8_lossy(&out.stdout)))
    }
    fn routes(&self) -> Result<Vec<RouteEntry>, String> {
        let out = Command::new(NETSTAT).arg("-rn").stderr(Stdio::null()).output().map_err(|e| format!("netstat: {e}"))?;
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
            Condition::Iface(n) => self
                .ifaces
                .borrow_mut()
                .entry(n.clone())
                .or_insert_with(|| self.view.iface_up(n))
                .clone(),
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
```

Move the new `use` lines to the top of the file with the others.

- [ ] **Step 5: Run tests**

Run: `cargo test --test conditions`
Expected: 9 passed.

- [ ] **Step 6: Commit**

```bash
git add -A
git commit -m "feat: evaluate start conditions (ifconfig, netstat, per-tick cache)

Co-Authored-By: Claude Opus 5.5 (1M context) <noreply@anthropic.com>"
```

---

### Task 3: `after` / `after_timeout` in manifests and boot.d

**Files:**
- Modify: `src/manifest.rs` (ServiceEntry + validate), `src/bootd.rs` (BootEntry + entries)
- Test: `tests/manifest.rs`, `tests/bootd_config.rs` (append)

**Interfaces:**
- Consumes: `conditions::{validate, parse_timeout}`.
- Produces: `ServiceEntry.after: Vec<String>`, `ServiceEntry.after_timeout: Option<String>`; `BootEntry.after: Vec<String>`, `BootEntry.after_timeout_secs: Option<u64>` (both `#[serde(default)]`).

- [ ] **Step 1: Write the failing tests**

Append to `tests/manifest.rs`:
```rust
#[test]
fn after_and_timeout_validation() {
    let m = Manifest::parse(&CARMQTT.replace("args = [", "after = [\"tailnet\", \"file:/tmp/x\"]\nafter_timeout = \"5m\"\nargs = [")).unwrap();
    assert_eq!(m.services[0].after, vec!["tailnet", "file:/tmp/x"]);
    assert_eq!(m.services[0].after_timeout.as_deref(), Some("5m"));
    let e = Manifest::parse(&CARMQTT.replace("args = [", "after = [\"service:tailscale.x\"]\nargs = [")).unwrap_err();
    assert!(format!("{e:#}").contains("requires a newer mibpkg"), "{e:#}");
    assert!(Manifest::parse(&CARMQTT.replace("args = [", "after = [\"tailnet\"]\nafter_timeout = \"5\"\nargs = [")).is_err());
    assert!(Manifest::parse(&CARMQTT.replace("args = [", "after_timeout = \"5m\"\nargs = [")).is_err(), "timeout without after");
    assert!(Manifest::parse(CARMQTT).unwrap().services[0].after.is_empty());
}
```

Append to `tests/bootd_config.rs`:
```rust
#[test]
fn after_fields_flow_into_bootd_and_old_files_still_load() {
    let f = common::fixture();
    let m = Manifest::parse(&M.replace("restart = \"on-failure\"", "restart = \"on-failure\"\nafter = [\"tailnet\"]\nafter_timeout = \"5m\"")).unwrap();
    let es = bootd::entries(&m);
    assert_eq!(es[0].after, vec!["tailnet"]);
    assert_eq!(es[0].after_timeout_secs, Some(300));
    bootd::write_for_package(&f.layout, "carmqtt", &es).unwrap();
    assert_eq!(bootd::load_all(&f.layout)[0].1.as_ref().unwrap(), &es[0]);
    // a v1 boot.d file without the new fields
    fs::write(f.layout.boot_d().join("old.svc.toml"), "package = \"old\"\nname = \"svc\"\nexec = \"@pkg/x\"\nphase = \"default\"\n").unwrap();
    let old = bootd::load_all(&f.layout).into_iter().find(|(n, _)| n == "old.svc.toml").unwrap().1.unwrap();
    assert!(old.after.is_empty() && old.after_timeout_secs.is_none());
}
```

- [ ] **Step 2: Run to verify it fails**

Run: `cargo test --test manifest --test bootd_config`
Expected: compile errors (no field `after`).

- [ ] **Step 3: Implement**

`src/manifest.rs` — add to `ServiceEntry`:
```rust
    /// Start conditions (all must hold). See conditions.rs.
    #[serde(default)]
    pub after: Vec<String>,
    /// Start anyway after waiting this long ("30s", "5m", "2h"); omitted = wait forever.
    #[serde(default)]
    pub after_timeout: Option<String>,
```
and in `validate()`, inside the `for s in &self.services` loop after the phase check:
```rust
            for a in &s.after {
                crate::conditions::validate(a).with_context(|| format!("service {}", s.name))?;
            }
            if let Some(t) = &s.after_timeout {
                if s.after.is_empty() {
                    bail!("service {}: after_timeout needs an `after` list", s.name);
                }
                crate::conditions::parse_timeout(t).with_context(|| format!("service {}", s.name))?;
            }
```

`src/bootd.rs` — add to `BootEntry`:
```rust
    #[serde(default)]
    pub after: Vec<String>,
    #[serde(default)]
    pub after_timeout_secs: Option<u64>,
```
and in `entries()` set:
```rust
            after: s.after.clone(),
            after_timeout_secs: s.after_timeout.as_deref().map(|t| crate::conditions::parse_timeout(t).expect("validated")),
```
(`Manifest::parse` already validated the timeout, so `expect` cannot fire for a parsed manifest.)

- [ ] **Step 4: Run tests**

Run: `cargo test`
Expected: all pass (existing BootEntry literals in `tests/bootd_config.rs` need `after: vec![], after_timeout_secs: None` added — add them).

- [ ] **Step 5: Commit**

```bash
git add -A
git commit -m "feat: after / after_timeout in manifests and boot.d

Co-Authored-By: Claude Opus 5.5 (1M context) <noreply@anthropic.com>"
```

---

### Task 4: Supervisor gating, wait clock, timeout

**Files:**
- Modify: `src/status.rs` (SvcState::Waiting, SvcStatus fields), `src/supervisor.rs`
- Test: `tests/supervisor.rs`, `tests/policy.rs` (update the SvcStatus literal)

**Interfaces:**
- Consumes: `conditions::{unmet, RealView, SystemView, TickCache, fmt_secs}`, `BootEntry.after/after_timeout_secs`.
- Produces: `SvcState::Waiting` (display `waiting`); `SvcStatus.waiting_for: Vec<String>`, `SvcStatus.waiting_secs: Option<u64>`, `SvcStatus.note: Option<String>` (all `#[serde(default)]`); `Supervisor::with_view(self, Box<dyn SystemView>) -> Self`.

- [ ] **Step 1: Write the failing tests (append to `tests/supervisor.rs`)**

```rust
fn marker(f: &common::Fixture) -> std::path::PathBuf {
    f.root().join("ready-flag")
}

#[test]
fn waits_for_a_file_condition_then_starts() {
    let f = common::fixture();
    let flag = marker(&f);
    install_pkg(&f, "1.0.0", "sleep 30", &format!("after = [\"file:{}\"]\n", flag.display()));
    let sys = RealSys { mounts: false };
    let mut sup = Supervisor::new(&f.layout, &sys, fast());
    sup.reload();
    sup.tick(Instant::now());
    let st = sup.status().services["demo.demo"].clone();
    assert_eq!(st.state, SvcState::Waiting);
    assert_eq!(st.waiting_for, vec![format!("file:{}", flag.display())]);
    fs::write(&flag, "").unwrap();
    assert!(until(&mut sup, |s| state(s) == SvcState::Running));
    assert!(sup.status().services["demo.demo"].waiting_for.is_empty());
    sup.stop_all();
}

#[test]
fn timeout_starts_anyway_with_a_note() {
    let f = common::fixture();
    install_pkg(&f, "1.0.0", "sleep 30", "after = [\"file:/nonexistent/flag\"]\nafter_timeout = \"1s\"\n");
    let sys = RealSys { mounts: false };
    let mut sup = Supervisor::new(&f.layout, &sys, fast());
    sup.reload();
    sup.tick(Instant::now());
    assert_eq!(state(&sup), SvcState::Waiting);
    assert!(until(&mut sup, |s| state(s) == SvcState::Running));
    let note = sup.status().services["demo.demo"].note.clone().unwrap();
    assert!(note.contains("started without file:/nonexistent/flag after 1s"), "{note}");
    sup.stop_all();
}

#[test]
fn crash_restart_waits_again_and_waiting_is_not_a_crash() {
    let f = common::fixture();
    let flag = marker(&f);
    fs::write(&flag, "").unwrap();
    // exits after 0.2 s; flag removed before the restart
    install_pkg(&f, "1.0.0", "sleep 0.2; exit 1", &format!("after = [\"file:{}\"]\n", flag.display()));
    let sys = RealSys { mounts: false };
    let mut sup = Supervisor::new(&f.layout, &sys, fast());
    sup.reload();
    assert!(until(&mut sup, |s| state(s) == SvcState::Running));
    fs::remove_file(&flag).unwrap();
    assert!(until(&mut sup, |s| state(s) == SvcState::Waiting), "restart after a crash waits for the condition");
    let t0 = Instant::now();
    while t0.elapsed() < Duration::from_millis(500) {
        sup.tick(Instant::now());
        std::thread::sleep(Duration::from_millis(10));
    }
    assert_eq!(state(&sup), SvcState::Waiting, "waiting never turns into failed");
    assert_eq!(sup.status().services["demo.demo"].restarts, 0);
}
```

Also update the `SvcStatus { ... }` literal in `tests/policy.rs` `status_roundtrip` to add `waiting_for: vec![], waiting_secs: None, note: None`.

- [ ] **Step 2: Run to verify it fails**

Run: `cargo test --test supervisor`
Expected: compile error (`SvcState::Waiting`, `waiting_for`, `note` missing).

- [ ] **Step 3: Implement status changes (`src/status.rs`)**

Add `Waiting` to `SvcState` and `SvcState::Waiting => "waiting"` in `Display`. Add to `SvcStatus`:
```rust
    /// Unmet start conditions while waiting.
    #[serde(default)]
    pub waiting_for: Vec<String>,
    #[serde(default)]
    pub waiting_secs: Option<u64>,
    /// e.g. "started without tailnet after 5m".
    #[serde(default)]
    pub note: Option<String>,
```

- [ ] **Step 4: Implement supervisor changes (`src/supervisor.rs`)**

1. Imports: `use crate::conditions::{self, fmt_secs, RealView, SystemView, TickCache};`
2. `Service` gains `wait_since: Option<Instant>`, `waiting_for: Vec<String>`, `note: Option<String>`; `Service::new` sets them to `None`, `Vec::new()`, `None`.
3. `Supervisor` gains `view: Box<dyn SystemView>`; `Supervisor::new` sets `view: Box::new(RealView)`; add:
```rust
    pub fn with_view(mut self, view: Box<dyn SystemView>) -> Self {
        self.view = view;
        self
    }
```
4. `tick` builds one cache per tick and passes it down:
```rust
    pub fn tick(&mut self, now: Instant) {
        let ids: Vec<String> = self.services.keys().cloned().collect();
        let cache = TickCache::new(self.view.as_ref());
        for id in ids {
            let mut s = self.services.remove(&id).unwrap();
            self.tick_one(&mut s, now, &cache);
            self.services.insert(id, s);
        }
    }
```
5. In `tick_one(&self, s: &mut Service, now: Instant, cache: &TickCache)`, replace the final start block:
```rust
        let due = s.next_start.map_or(true, |at| now >= at);
        if s.child.is_none()
            && matches!(s.state, SvcState::Starting | SvcState::Backoff | SvcState::NeedsConfig | SvcState::Waiting)
            && due
        {
            if !self.conditions_allow(s, now, cache) {
                return;
            }
            self.start(s, now);
        }
```
and add:
```rust
    /// Gate a start on the service's `after` conditions. Returns true to start now.
    /// A start with every condition met clears an earlier timeout note.
    fn conditions_allow(&self, s: &mut Service, now: Instant, cache: &TickCache) -> bool {
        let unmet = conditions::unmet(&s.entry.after, &self.layout.settings, cache);
        if unmet.is_empty() {
            s.wait_since = None;
            s.waiting_for.clear();
            s.note = None;
            return true;
        }
        let since = *s.wait_since.get_or_insert(now);
        if let Some(limit) = s.entry.after_timeout_secs {
            if now.duration_since(since) >= Duration::from_secs(limit) {
                s.note = Some(format!("started without {} after {}", unmet.join(", "), fmt_secs(limit)));
                s.wait_since = None;
                s.waiting_for.clear();
                return true;
            }
        }
        s.state = SvcState::Waiting;
        s.waiting_for = unmet;
        false
    }
```
6. In `status()`, fill the new fields:
```rust
                        waiting_for: s.waiting_for.clone(),
                        waiting_secs: s.wait_since.filter(|_| s.state == SvcState::Waiting).map(|w| Instant::now().duration_since(w).as_secs()),
                        note: s.note.clone(),
```
7. `reload()` keep-logic: a `Waiting` service is kept like any other (no change needed).

- [ ] **Step 5: Run tests**

Run: `cargo test --test supervisor --test policy && cargo test`
Expected: all pass.

- [ ] **Step 6: Commit**

```bash
git add -A
git commit -m "feat: supervisor waits for start conditions, with opt-in timeout

Co-Authored-By: Claude Opus 5.5 (1M context) <noreply@anthropic.com>"
```

---

### Task 5: Show waiting / notes in `mibpkg status` and GEM

**Files:**
- Modify: `src/status.rs` (add `describe`), `src/cli.rs` (`print_status`), `src/gem.rs` (service line)
- Test: `tests/policy.rs` (append), `tests/gem.rs` (append)

**Interfaces:**
- Consumes: `SvcStatus`, `conditions::fmt_secs`.
- Produces: `status::describe(&SvcStatus) -> String`.

- [ ] **Step 1: Write the failing tests**

Append to `tests/policy.rs`:
```rust
#[test]
fn describe_renders_each_state() {
    let base = SvcStatus { state: SvcState::Running, pid: Some(7), restarts: 2, last_exit: None, error: None, waiting_for: vec![], waiting_secs: None, note: None };
    assert_eq!(status::describe(&base), "running pid 7 restarts 2");
    let w = SvcStatus { state: SvcState::Waiting, pid: None, restarts: 0, waiting_for: vec!["tailnet".into(), "hmi-ready".into()], waiting_secs: Some(130), ..base.clone() };
    assert_eq!(status::describe(&w), "waiting: tailnet, hmi-ready (2m10s)");
    let n = SvcStatus { restarts: 0, note: Some("started without tailnet after 5m".into()), ..base.clone() };
    assert_eq!(status::describe(&n), "running pid 7 (started without tailnet after 5m)");
    let e = SvcStatus { state: SvcState::NeedsConfig, pid: None, restarts: 0, error: Some("missing /x".into()), ..base };
    assert_eq!(status::describe(&e), "needs-config (missing /x)");
}
```

Append to `tests/gem.rs` a test that builds a `Model` with a `Status` whose service is waiting and checks the package screen contains `demo.demo: waiting: tailnet (45s)`:
```rust
#[test]
fn package_screen_shows_waiting() {
    let f = common::fixture();
    let sys = RecordingSys::default();
    let ctx = Ctx::new(f.layout.clone(), &sys);
    let a = common::pkg_src(f.root(), "demo", "1.0.0", "sleep 30", "");
    let c = Collection::load(&common::collection(&f.sd(), "complex256", &[a])).unwrap();
    install::install(&ctx, &[f.sd()], &c, "demo").unwrap();
    let mut st = mibpkg::status::Status::default();
    st.services.insert(
        "demo.demo".into(),
        mibpkg::status::SvcStatus {
            state: mibpkg::status::SvcState::Waiting,
            pid: None,
            restarts: 0,
            last_exit: None,
            error: None,
            waiting_for: vec!["tailnet".into()],
            waiting_secs: Some(45),
            note: None,
        },
    );
    let scan = media::scan(&f.layout);
    let state = State::load(&f.layout).unwrap();
    let m = Model { layout: &f.layout, unit: &ctx.unit, state: &state, status: Some(&st), scan: &scan, disabled: false, supervisor_running: true };
    let screen = gem::screens(&m).into_iter().find(|s| s.name == "mibpkg_demo").unwrap();
    assert!(labels(&screen).iter().any(|l| l == "demo.demo: waiting: tailnet (45s)"), "{:?}", labels(&screen));
}
```

- [ ] **Step 2: Run to verify it fails**

Run: `cargo test --test policy --test gem`
Expected: compile error (`status::describe` missing) — and after adding a stub, the GEM test fails on the old line format.

- [ ] **Step 3: Implement**

`src/status.rs`:
```rust
/// One-line human description, shared by `mibpkg status` and GEM.
pub fn describe(s: &SvcStatus) -> String {
    let mut out = if s.state == SvcState::Waiting {
        let mut w = format!("waiting: {}", s.waiting_for.join(", "));
        if let Some(secs) = s.waiting_secs {
            w.push_str(&format!(" ({})", crate::conditions::fmt_secs(secs)));
        }
        w
    } else {
        s.state.to_string()
    };
    if let Some(p) = s.pid {
        out.push_str(&format!(" pid {p}"));
    }
    if s.restarts > 0 {
        out.push_str(&format!(" restarts {}", s.restarts));
    }
    if let Some(n) = &s.note {
        out.push_str(&format!(" ({n})"));
    } else if let Some(e) = &s.error {
        out.push_str(&format!(" ({e})"));
    }
    out
}
```
`src/cli.rs` `print_status`: replace the per-service `println!(...)` with `println!("  {id}: {}", status::describe(v));`.
`src/gem.rs`: replace the `Some(s) => format!(...)` arm with `Some(s) => format!("{id}: {}", status::describe(s)),`.

- [ ] **Step 4: Run tests**

Run: `cargo test`
Expected: all pass.

- [ ] **Step 5: Commit**

```bash
git add -A
git commit -m "feat: show waiting conditions and timeout notes in status and GEM

Co-Authored-By: Claude Opus 5.5 (1M context) <noreply@anthropic.com>"
```

---

### Task 6: On-car capture — real fixtures and alias defaults (car must be on; user at the car)

Read-only on the unit. Use `ssh -n audi` when the user is at the car (swagon is unreachable ~2-3 min after boot).

**Files:**
- Create: `tests/fixtures/netstat-rn-unit-tailnet-up.txt`, `tests/fixtures/netstat-rn-unit-tailnet-stuck.txt` (the latter only if a stuck boot happens during capture), `tests/fixtures/ifconfig-unit-tun9.txt`, `tests/fixtures/ifconfig-unit-missing.txt`
- Modify: `src/conditions.rs` (`DEFAULTS`, parser fixes if the real output differs), `docs/unit-facts.md`
- Test: `tests/conditions.rs` (append a test per real fixture)

- [ ] **Step 1: Capture routing table and interfaces**

```bash
ssh -n audi '/mnt/app/armle/usr/bin/netstat -rn' > tests/fixtures/netstat-rn-unit-tailnet-up.txt
ssh -n audi 'ifconfig tun9' > tests/fixtures/ifconfig-unit-tun9.txt
ssh -n audi 'ifconfig nosuch0; echo "exit=$?"' > tests/fixtures/ifconfig-unit-missing.txt 2>&1
```
Strip the ssh banner lines from the files if present (the unit prints an ASCII-art banner on login).

- [ ] **Step 2: Measure the HMI markers' order relative to boot**

The unit clock is unset at boot, so use ksh's `-nt` (newer than) ordering against the supervisor's pid file (written ~49 s after boot):
```bash
ssh -n audi 'for a in /tmp/smFlagStartupDone /tmp/startupFinishedFlag /tmp/MMX-startup-finished /tmp/mibpkg.pid; do for b in /tmp/smFlagStartupDone /tmp/startupFinishedFlag /tmp/MMX-startup-finished /tmp/mibpkg.pid; do [ "$a" -nt "$b" ] && echo "$a newer than $b"; done; done; ls -l /tmp/smFlagStartupDone /tmp/startupFinishedFlag /tmp/MMX-startup-finished /tmp/mibpkg.pid'
```
Choose `hmi-ready` = the marker that appears last among those that exist on every boot (check across two boots). Record the order in `docs/unit-facts.md`.

- [ ] **Step 3: Add tests for the real output, see them fail or pass**

```rust
#[test]
fn real_unit_netstat_has_default_and_tailnet_routes() {
    let r = parse_netstat_rn(&fixture("netstat-rn-unit-tailnet-up.txt"));
    let s = Settings::default();
    struct V(Vec<RouteEntry>);
    impl SystemView for V {
        fn file_exists(&self, _: &Path) -> bool { false }
        fn iface_up(&self, _: &str) -> Result<bool, String> { Ok(false) }
        fn routes(&self) -> Result<Vec<RouteEntry>, String> { Ok(self.0.clone()) }
    }
    let v = V(r);
    assert!(unmet(&["internet".to_string(), "tailnet".to_string()], &s, &TickCache::new(&v)).is_empty());
}

#[test]
fn real_unit_ifconfig() {
    assert!(parse_ifconfig_up(&fixture("ifconfig-unit-tun9.txt")));
    assert!(fixture("ifconfig-unit-missing.txt").contains("exit="), "records the missing-interface exit status");
}
```
Run: `cargo test --test conditions`. If the tailnet route appears in a different form than `100.64/10` via `tun…` (e.g. per-peer `/32` routes only, or a different interface name), change `DEFAULTS` `tailnet` to match what the real table shows (e.g. `route:100.64@tun` → the real destination spelling) and document why in `docs/unit-facts.md`. If `ifconfig nosuch0` exits 0, change `RealView::iface_up` to rely on `parse_ifconfig_up` alone (it already requires the flags line). Re-run until green.

- [ ] **Step 4: Commit**

```bash
git add -A
git commit -m "test: real unit netstat/ifconfig fixtures; set condition defaults from the unit

Co-Authored-By: Claude Opus 5.5 (1M context) <noreply@anthropic.com>"
```

---

### Task 7: Release — mibpkg 0.2.0, carmqtt 0.1.2 with `after = ["tailnet"]`, verify on the car

**Files:**
- Modify: `Cargo.toml` (version `0.2.0`), `docs/ideas.md`, `docs/unit-facts.md`
- Modify (carmqtt, in a worktree on `main`): `packaging/manifest.toml.in`, `Cargo.toml` (version `0.1.2`)

- [ ] **Step 1: Bump mibpkg and build**

Set `version = "0.2.0"` in `Cargo.toml`. Run `cargo test` (all pass) and `scripts/build.sh` (QNX build succeeds). Commit (`chore: mibpkg 0.2.0`).

- [ ] **Step 2: carmqtt manifest + version, in a worktree**

```bash
WT=<scratchpad>/carmqtt-main
git -C ../mib2q-carmqtt worktree add -q "$WT" main
```
In `$WT/packaging/manifest.toml.in`, add to the `[[service]]` block:
```toml
after = ["tailnet"]
after_timeout = "5m"
```
Set `version = "0.1.2"` in `$WT/Cargo.toml` (edit with Python or an editor; BSD `sed` lacks GNU `0,/re/`). Run `cargo test` in `$WT` (all pass), `scripts/build.sh`, then `MIBPKG=<mibpkg>/target/release/mibpkg scripts/package.sh` (the host `mibpkg` must be the 0.2.0 build: `cargo build --release` in mibpkg first, so `build` validates `after`). Copy `$WT/target/mpk/carmqtt-0.1.2.mpk` to `<mibpkg>/out/extra/`. Commit in `$WT` (`feat: carmqtt waits for the tailnet (mibpkg start conditions); 0.1.2`). Remove the worktree (`git worktree remove`), never switching the main carmqtt working tree.

- [ ] **Step 3: Bundle**

Run `scripts/bundle.sh out/extra/carmqtt-0.1.2.mpk`. Check `out/sd/mibpkg/collections/complex256/index.json` lists `mibpkg 0.2.0` and `carmqtt 0.1.2`.

- [ ] **Step 4: Put it on the card and update on the car (user present)**

With the M.I.B. card mounted on the laptop (`/Volumes/AUDI`): replace only `mibpkg/collections/` from `out/sd/mibpkg/collections/`; also copy `out/sd/mod/mibpkg/*` and `out/sd/mod/*.sh` so a re-run of the M.I.B. script installs 0.2.0. In the car:
1. GEM → mibpkg → Rescan SD card; leave and re-enter GEM.
2. GEM → mibpkg → mibpkg_mibpkg → **Update to 0.2.0** (self-update; this also verifies the self-update path). Check over `ssh -n audi '/tmp/mibpkg status'` that a new supervisor pid runs and carmqtt is running.
3. GEM → mibpkg_carmqtt → **Update to 0.1.2**.

- [ ] **Step 5: Verify behaviour across a reboot**

Restart the car. As soon as `audi` is reachable, run `ssh -n audi '/tmp/mibpkg status'` repeatedly: expect `carmqtt.carmqtt: waiting: tailnet (…)` until Tailscale's route appears, then `running`. Then `ssh -n audi 'cat /tmp/carmqtt.log'`: expect the first `mqtt connect` attempt to succeed (no "No route to host" streak). If Tailscale gets stuck, expect `waiting: tailnet` and, after 5 m, `running … (started without tailnet after 5m)`.

- [ ] **Step 6: Record and commit**

Add rows to `docs/unit-facts.md` (self-update verified; start conditions behaviour and timings). Mark the start-conditions item in `docs/ideas.md` done. Commit (`docs: start conditions verified on the unit`).

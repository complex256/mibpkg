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

#[test]
fn parse_timeout_never_panics_on_odd_input() {
    for bad in ["5\u{b5}", "+5m", "5124095576030432h", "\u{b5}", "1\u{1f600}m", " 5m"] {
        assert!(parse_timeout(bad).is_err(), "{bad:?}");
    }
}

#[test]
fn hung_system_commands_time_out() {
    use mibpkg::conditions::RealView;
    use std::os::unix::fs::PermissionsExt;
    use std::time::{Duration, Instant};
    let t = tempfile::tempdir().unwrap();
    let slow = t.path().join("slow");
    fs::write(&slow, "#!/bin/sh\nsleep 10\n").unwrap();
    fs::set_permissions(&slow, fs::Permissions::from_mode(0o755)).unwrap();
    let v = RealView { netstat: slow.clone(), ifconfig: slow, timeout: Duration::from_millis(300) };
    let t0 = Instant::now();
    let e = v.routes().unwrap_err();
    assert!(e.contains("timed out"), "{e}");
    assert!(v.iface_up("tun9").unwrap_err().contains("timed out"));
    assert!(t0.elapsed() < Duration::from_secs(3), "{:?}", t0.elapsed());
}

#[test]
fn real_view_reads_command_output() {
    use mibpkg::conditions::RealView;
    use std::os::unix::fs::PermissionsExt;
    let t = tempfile::tempdir().unwrap();
    let ns = t.path().join("netstat");
    fs::write(&ns, format!("#!/bin/sh\ncat {}\n", Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/netstat-rn-sample.txt").display())).unwrap();
    fs::set_permissions(&ns, fs::Permissions::from_mode(0o755)).unwrap();
    let v = RealView { netstat: ns, ifconfig: "/nonexistent/ifconfig".into(), timeout: std::time::Duration::from_secs(3) };
    assert!(v.routes().unwrap().iter().any(|r| r.dest == "default"));
    assert!(v.iface_up("tun9").is_err(), "a missing ifconfig binary is an error, not 'down'");
}

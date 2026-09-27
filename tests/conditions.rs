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

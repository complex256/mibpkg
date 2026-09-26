use mibpkg::manifest::{resolve, safe_rel, Manifest, Restart};

const CARMQTT: &str = r#"
[package]
name = "carmqtt"
version = "1.2.0"
description = "Car status to Home Assistant over MQTT"

[compat]
units = ["MHI2Q"]

[[file]]
src = "bin/carmqtt"
mode = "755"

[[file]]
src = "carmqtt.toml.example"

[[config]]
path = "carmqtt.toml"
example = "carmqtt.toml.example"
required = true

[[service]]
name = "carmqtt"
exec = "@pkg/bin/carmqtt"
args = ["--config", "@config/carmqtt.toml"]
"#;

#[test]
fn parses_full_manifest_with_defaults() {
    let m = Manifest::parse(CARMQTT).unwrap();
    assert_eq!(m.package.name, "carmqtt");
    assert_eq!(m.version().unwrap(), semver::Version::new(1, 2, 0));
    assert_eq!(m.compat.units, vec!["MHI2Q"]);
    assert_eq!(m.files[1].mode, "644");
    assert!(m.configs[0].required);
    assert_eq!(m.services[0].phase, "default");
    assert_eq!(m.services[0].restart, Restart::Always);
    assert!(!m.uses_scripts());
}

fn with(extra: &str) -> String {
    format!("{CARMQTT}\n{extra}")
}

#[test]
fn rejects_patch_as_needing_newer_mibpkg() {
    let e = Manifest::parse(&with("[[patch]]\nkind = \"json-merge\"\n")).unwrap_err();
    assert!(format!("{e:#}").contains("requires a newer mibpkg"), "{e:#}");
}

#[test]
fn rejects_unknown_phase_as_needing_newer_mibpkg() {
    let t = CARMQTT.replace("args = [", "phase = \"after-sim-data\"\nargs = [");
    let e = Manifest::parse(&t).unwrap_err();
    assert!(format!("{e:#}").contains("requires a newer mibpkg"), "{e:#}");
}

#[test]
fn rejects_unknown_keys_bad_names_versions_paths() {
    assert!(Manifest::parse(&with("[extra]\na = 1\n")).is_err());
    assert!(Manifest::parse(&CARMQTT.replace("name = \"carmqtt\"\nversion", "name = \"Car MQTT\"\nversion")).is_err());
    assert!(Manifest::parse(&CARMQTT.replace("1.2.0", "1.2")).is_err());
    assert!(Manifest::parse(&CARMQTT.replace("src = \"bin/carmqtt\"", "src = \"../carmqtt\"")).is_err());
    assert!(Manifest::parse(&CARMQTT.replace("mode = \"755\"", "mode = \"9\"")).is_err());
}

#[test]
fn service_exec_must_be_a_listed_package_file() {
    assert!(Manifest::parse(&CARMQTT.replace("@pkg/bin/carmqtt", "/bin/sh")).is_err());
    assert!(Manifest::parse(&CARMQTT.replace("@pkg/bin/carmqtt", "@pkg/bin/other")).is_err());
}

#[test]
fn scripts_are_detected() {
    let m = Manifest::parse(&with("[scripts]\npost_install = \"scripts/post.sh\"\n")).unwrap();
    assert!(m.uses_scripts());
}

#[test]
fn safe_rel_and_resolve() {
    assert!(safe_rel("a/b").is_ok());
    for bad in ["", "/abs", "../x", "a/../b", "./a"] {
        assert!(safe_rel(bad).is_err(), "{bad}");
    }
    assert_eq!(resolve("@pkg/bin/x --c @config/y", "/P", "/C"), "/P/bin/x --c /C/y");
}

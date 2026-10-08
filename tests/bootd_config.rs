mod common;
use mibpkg::bootd::{self, BootEntry};
use mibpkg::config;
use mibpkg::manifest::{Manifest, Restart};
use std::fs;
use std::os::unix::fs::PermissionsExt;

const M: &str = "[package]\nname = \"carmqtt\"\nversion = \"1.0.0\"\n\n[[file]]\nsrc = \"bin/carmqtt\"\n\n[[config]]\npath = \"carmqtt.toml\"\nrequired = true\n\n[[config]]\npath = \"extra.toml\"\n\n[[service]]\nname = \"carmqtt\"\nexec = \"@pkg/bin/carmqtt\"\nargs = [\"--config\", \"@config/carmqtt.toml\"]\nrestart = \"on-failure\"\n";

#[test]
fn entries_write_load_remove() {
    let f = common::fixture();
    let m = Manifest::parse(M).unwrap();
    let es = bootd::entries(&m);
    assert_eq!(
        es,
        vec![BootEntry {
            package: "carmqtt".into(),
            name: "carmqtt".into(),
            exec: "@pkg/bin/carmqtt".into(),
            args: vec!["--config".into(), "@config/carmqtt.toml".into()],
            phase: "default".into(),
            restart: Restart::OnFailure,
            requires: vec!["@config/carmqtt.toml".into()],
            after: vec![],
            after_timeout_secs: None,
        }]
    );
    assert_eq!(es[0].id(), "carmqtt.carmqtt");
    bootd::write_for_package(&f.layout, "carmqtt", &es).unwrap();
    // a neighbour whose name shares a prefix must survive removal
    let other = BootEntry { package: "car".into(), name: "x".into(), ..es[0].clone() };
    bootd::write_for_package(&f.layout, "car", &[other]).unwrap();
    let all = bootd::load_all(&f.layout);
    assert_eq!(all.iter().map(|(n, _)| n.as_str()).collect::<Vec<_>>(), vec!["car.x.toml", "carmqtt.carmqtt.toml"]);
    assert_eq!(all[1].1.as_ref().unwrap(), &es[0]);
    bootd::remove_for_package(&f.layout, "carmqtt").unwrap();
    assert_eq!(bootd::load_all(&f.layout).len(), 1);
}

#[test]
fn unknown_phase_in_bootd_is_an_error_entry() {
    let f = common::fixture();
    fs::create_dir_all(f.layout.boot_d()).unwrap();
    fs::write(
        f.layout.boot_d().join("a.b.toml"),
        "package = \"a\"\nname = \"b\"\nexec = \"@pkg/b\"\nphase = \"after-sim-data\"\n",
    )
    .unwrap();
    let all = bootd::load_all(&f.layout);
    assert!(format!("{:#}", all[0].1.as_ref().unwrap_err()).contains("requires a newer mibpkg"));
}

#[test]
fn config_import_prev_and_missing() {
    let f = common::fixture();
    let m = Manifest::parse(M).unwrap();
    assert_eq!(config::missing_required(&f.layout, &m), vec!["carmqtt.toml"]);
    let card = config::card_path(&f.sd(), "carmqtt", "carmqtt.toml");
    fs::create_dir_all(card.parent().unwrap()).unwrap();
    fs::write(&card, "v1").unwrap();
    assert_eq!(config::import(&f.layout, &[f.sd()], &m, None).unwrap().imported, vec!["carmqtt.toml"]);
    let dst = f.layout.config_dir("carmqtt").join("carmqtt.toml");
    assert_eq!(fs::read_to_string(&dst).unwrap(), "v1");
    assert_eq!(fs::metadata(&dst).unwrap().permissions().mode() & 0o777, 0o600);
    assert!(config::missing_required(&f.layout, &m).is_empty());
    assert_eq!(config::import(&f.layout, &[f.sd()], &m, None).unwrap(), config::Report::default(), "unchanged file is not re-imported");
    fs::write(&card, "v2").unwrap();
    config::import(&f.layout, &[f.sd()], &m, None).unwrap();
    assert_eq!(fs::read_to_string(&dst).unwrap(), "v2");
    assert_eq!(fs::read_to_string(dst.with_file_name("carmqtt.toml.prev")).unwrap(), "v1");
}

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

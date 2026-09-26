mod common;
use mibpkg::cli::{run, utc_stamp};
use mibpkg::state::State;
use std::fs;

fn args(root: &std::path::Path, rest: &[&str]) -> Vec<String> {
    let mut v = vec!["--root".to_string(), root.display().to_string(), "--no-supervisor".to_string()];
    v.extend(rest.iter().map(|s| s.to_string()));
    v
}

#[test]
fn stamp_is_utc_civil_time() {
    assert_eq!(utc_stamp(0), "1970-01-01 00:00 UTC");
    assert_eq!(utc_stamp(1_700_000_000), "2023-11-14 22:13 UTC");
}

#[test]
fn build_and_index_from_the_host() {
    let t = tempfile::tempdir().unwrap();
    let src = common::pkg_src(t.path(), "alpha", "1.0.0", "sleep 1", "");
    let coll = t.path().join("c");
    fs::create_dir_all(coll.join("packages")).unwrap();
    let a = |v: &[&str]| v.iter().map(|s| s.to_string()).collect::<Vec<_>>();
    assert_eq!(run(&a(&["build", src.to_str().unwrap(), coll.join("packages").to_str().unwrap()])).unwrap(), 0);
    assert!(coll.join("packages/alpha-1.0.0.mpk").is_file());
    assert_eq!(run(&a(&["index", coll.to_str().unwrap(), "--id", "complex256", "--name", "complex256 packages"])).unwrap(), 0);
    assert!(fs::read_to_string(coll.join("index.json")).unwrap().contains("\"alpha\""));
}

#[test]
fn gem_actions_record_results_and_never_fail() {
    let f = common::fixture();
    let a = common::pkg_src(f.root(), "alpha", "1.0.0", "sleep 30", "");
    common::collection(&f.sd(), "complex256", &[a]);
    assert_eq!(run(&args(f.root(), &["gem", "install", "alpha", "complex256"])).unwrap(), 0);
    let st = State::load(&f.layout).unwrap();
    assert!(st.results["alpha"].starts_with("OK: installed alpha 1.0.0"), "{:?}", st.results);
    assert!(f.layout.engdefs().join("mibpkg_alpha.esd").is_file(), "screens regenerated");
    assert_eq!(run(&args(f.root(), &["gem", "install", "nope"])).unwrap(), 0);
    let st = State::load(&f.layout).unwrap();
    assert!(st.results["nope"].starts_with("FAILED:"), "{:?}", st.results);
    fs::write(f.layout.run("lock"), std::process::id().to_string()).unwrap();
    run(&args(f.root(), &["gem", "remove", "alpha"])).unwrap();
    let st = State::load(&f.layout).unwrap();
    assert!(st.results["alpha"].starts_with("FAILED: busy"), "double tap: {:?}", st.results);
    assert!(st.packages.contains_key("alpha"), "busy action changed nothing");
    fs::remove_file(f.layout.run("lock")).unwrap();
}

#[test]
fn rescan_imports_config_and_requests_reload() {
    let f = common::fixture();
    let a = common::pkg_src(f.root(), "alpha", "1.0.0", "sleep 30", "[[config]]\npath = \"a.toml\"\nrequired = true\n");
    common::collection(&f.sd(), "complex256", &[a]);
    run(&args(f.root(), &["install", "alpha"])).unwrap();
    fs::remove_file(f.layout.run("reload")).unwrap();
    let card = mibpkg::config::card_path(&f.sd(), "alpha", "a.toml");
    fs::create_dir_all(card.parent().unwrap()).unwrap();
    fs::write(&card, "x").unwrap();
    run(&args(f.root(), &["rescan"])).unwrap();
    assert!(f.layout.config_dir("alpha").join("a.toml").is_file());
    assert!(f.layout.run("reload").exists());
}

#[test]
fn bootstrap_and_uninstall_self() {
    let f = common::fixture();
    assert_eq!(run(&args(f.root(), &["bootstrap"])).unwrap(), 0);
    assert!(fs::read_to_string(f.layout.startup_sh()).unwrap().contains(mibpkg::hook::MARKER));
    assert!(f.layout.engdefs().join("mibpkg.esd").is_file());
    assert!(f.layout.boot_d().is_dir());
    assert_eq!(run(&args(f.root(), &["bootstrap"])).unwrap(), 0, "idempotent");
    assert_eq!(run(&args(f.root(), &["uninstall-self"])).unwrap(), 0);
    assert_eq!(fs::read_to_string(f.layout.startup_sh()).unwrap(), common::STARTUP);
    assert!(!f.layout.engdefs().join("mibpkg.esd").exists());
}

#[test]
fn unknown_command_is_an_error() {
    let f = common::fixture();
    assert!(run(&args(f.root(), &["frobnicate"])).is_err());
}

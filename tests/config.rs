//! Config files written from two places, the card (mibpkg/config/<pkg>/) and the web interface
//! (`gem config-set`): an upgrade or rescan must never drop what the user set on the car.
mod common;
use mibpkg::cli::run;
use mibpkg::state::State;
use std::fs;
use std::path::PathBuf;

const ID: &str = "0123456789abcdef0123456789abcdef";
const CONF: &str = "[[config]]\npath = \"carmqtt.toml\"\nrequired = true\n";
const MINIMAL: &str = "[mqtt]\nhost = \"ha.local\"\n";
const EDITED: &str = "[mqtt]\nhost = \"ha.local\"\n\n[sources.ubx]\nenabled = true\npriority = 20\n";

fn mibpkg(f: &common::Fixture, rest: &[&str]) {
    let mut v = vec!["--root".to_string(), f.root().display().to_string(), "--no-supervisor".to_string()];
    v.extend(rest.iter().map(|s| s.to_string()));
    run(&v).unwrap();
}

/// A gem verb's recorded result (what the web interface shows).
fn gem(f: &common::Fixture, rest: &[&str]) -> String {
    let mut v = vec!["gem"];
    v.extend_from_slice(rest);
    mibpkg(f, &v);
    let st = State::load(&f.layout).unwrap();
    let key = if rest.len() > 1 && rest[0] != "rescan" { rest[1] } else { "_" };
    st.results.get(key).cloned().unwrap_or_default()
}

/// Offer carmqtt in these versions on the card.
fn offer(f: &common::Fixture, versions: &[&str]) {
    let srcs: Vec<_> = versions.iter().map(|v| common::pkg_src(f.root(), "carmqtt", v, "sleep 30", CONF)).collect();
    common::collection(&f.sd(), "complex256", &srcs);
}

fn card(f: &common::Fixture) -> PathBuf {
    mibpkg::config::card_path(&f.sd(), "carmqtt", "carmqtt.toml")
}

fn put_on_card(f: &common::Fixture, text: &str) {
    fs::create_dir_all(card(f).parent().unwrap()).unwrap();
    fs::write(card(f), text).unwrap();
}

fn live(f: &common::Fixture) -> String {
    fs::read_to_string(f.layout.config_dir("carmqtt").join("carmqtt.toml")).unwrap()
}

fn pending(f: &common::Fixture) -> PathBuf {
    mibpkg::config::pending_path(&f.layout, "carmqtt", "carmqtt.toml")
}

fn web_edit(f: &common::Fixture, text: &str) {
    fs::write(f.layout.run(&format!("text.{ID}")), text).unwrap();
    let r = gem(f, &["config-set", "carmqtt", "0", ID]);
    assert!(r.starts_with("OK: saved"), "{r}");
}

/// Installed from the card with the card's config, then edited in the web interface.
fn installed_and_edited() -> common::Fixture {
    let f = common::fixture();
    offer(&f, &["0.2.1"]);
    put_on_card(&f, MINIMAL);
    mibpkg(&f, &["install", "carmqtt"]);
    assert_eq!(live(&f), MINIMAL, "a new install takes the card's config");
    web_edit(&f, EDITED);
    f
}

#[test]
fn upgrade_and_rescan_keep_config_edited_on_the_car() {
    // The on-car report: index, rescan, install 0.2.1 -> 0.2.2 with the old file still on the card.
    let f = installed_and_edited();
    offer(&f, &["0.2.1", "0.2.2"]);
    mibpkg(&f, &["rescan"]);
    assert_eq!(live(&f), EDITED, "rescan reverted the web edit");
    mibpkg(&f, &["install", "carmqtt"]);
    assert_eq!(install_version(&f), "0.2.2");
    assert_eq!(live(&f), EDITED, "the upgrade reverted the web edit");
    assert!(!pending(&f).exists(), "an unchanged card file is no conflict");
    // The same through GEM / the web interface.
    offer(&f, &["0.2.1", "0.2.2", "0.2.3"]);
    assert!(gem(&f, &["rescan"]).starts_with("OK: rescanned"));
    assert!(gem(&f, &["install", "carmqtt"]).starts_with("OK: updated carmqtt 0.2.2 -> 0.2.3"));
    assert_eq!(live(&f), EDITED);
}

fn install_version(f: &common::Fixture) -> String {
    mibpkg::install::current_version(&f.layout, "carmqtt").unwrap()
}

#[test]
fn a_file_changed_on_the_card_still_wins_over_an_untouched_car_copy() {
    let f = common::fixture();
    offer(&f, &["0.2.1"]);
    put_on_card(&f, MINIMAL);
    mibpkg(&f, &["install", "carmqtt"]);
    put_on_card(&f, EDITED);
    assert!(gem(&f, &["rescan"]).contains("carmqtt: imported carmqtt.toml"));
    assert_eq!(live(&f), EDITED);
    assert_eq!(fs::read_to_string(f.layout.config_dir("carmqtt").join("carmqtt.toml.prev")).unwrap(), MINIMAL);
}

#[test]
fn changed_on_both_sides_is_a_conflict_that_keeps_the_car_copy() {
    let f = installed_and_edited();
    put_on_card(&f, "[mqtt]\nhost = \"new.local\"\n");
    let r = gem(&f, &["rescan"]);
    assert!(r.contains("config conflict: carmqtt.toml"), "{r}");
    assert_eq!(live(&f), EDITED);
    assert_eq!(fs::read_to_string(pending(&f)).unwrap(), "[mqtt]\nhost = \"new.local\"\n");
    // Reported again on an upgrade, still without touching the car's copy.
    offer(&f, &["0.2.1", "0.2.2"]);
    let r = gem(&f, &["install", "carmqtt"]);
    assert!(r.contains("updated carmqtt 0.2.1 -> 0.2.2; config conflict: carmqtt.toml"), "{r}");
    assert_eq!(live(&f), EDITED);
}

#[test]
fn keep_settles_the_conflict_for_good() {
    let f = installed_and_edited();
    put_on_card(&f, "[mqtt]\nhost = \"new.local\"\n");
    gem(&f, &["rescan"]);
    let _ = fs::remove_file(f.layout.run("restart.carmqtt.carmqtt")); // from the web edit
    let r = gem(&f, &["config-resolve", "carmqtt", "keep"]);
    assert_eq!(r.split(" (").next().unwrap(), "OK: kept the current carmqtt.toml for carmqtt");
    assert_eq!(live(&f), EDITED);
    assert!(!pending(&f).exists());
    assert!(!f.layout.run("restart.carmqtt.carmqtt").exists(), "nothing changed, nothing restarts");
    assert!(gem(&f, &["rescan"]).starts_with("OK: rescanned; no new config files"), "the card copy was dismissed");
    assert_eq!(live(&f), EDITED);
    assert!(gem(&f, &["config-resolve", "carmqtt", "keep"]).contains("no config conflict"));
}

#[test]
fn card_settles_the_conflict_with_the_card_copy() {
    let f = installed_and_edited();
    put_on_card(&f, "[mqtt]\nhost = \"new.local\"\n");
    gem(&f, &["rescan"]);
    let _ = fs::remove_file(f.layout.run("restart.carmqtt.carmqtt"));
    // By number, as the web interface sends it.
    let r = gem(&f, &["config-resolve", "carmqtt", "card", "0"]);
    assert!(r.starts_with("OK: using the card's carmqtt.toml for carmqtt; restarting"), "{r}");
    assert_eq!(live(&f), "[mqtt]\nhost = \"new.local\"\n");
    assert_eq!(fs::read_to_string(f.layout.config_dir("carmqtt").join("carmqtt.toml.prev")).unwrap(), EDITED);
    assert!(!pending(&f).exists());
    assert!(f.layout.run("restart.carmqtt.carmqtt").exists());
    assert!(gem(&f, &["config-resolve", "carmqtt", "card", "9"]).contains("has no config \"9\""));
}

#[test]
fn the_config_flag_settles_conflicts_without_asking() {
    let f = installed_and_edited();
    put_on_card(&f, "a = 1\n");
    mibpkg(&f, &["rescan", "--config=keep"]);
    assert_eq!(live(&f), EDITED);
    assert!(!pending(&f).exists());
    put_on_card(&f, "a = 2\n");
    offer(&f, &["0.2.1", "0.2.2"]);
    mibpkg(&f, &["install", "carmqtt", "--config=card"]);
    assert_eq!(live(&f), "a = 2\n");
    assert!(!pending(&f).exists());
    let mut v = vec!["--root".to_string(), f.root().display().to_string(), "--no-supervisor".to_string()];
    v.extend(["rescan", "--config=mine"].map(String::from));
    assert!(run(&v).is_err());
}

#[test]
fn an_install_from_an_older_mibpkg_never_loses_the_car_copy() {
    // No record of the card bytes yet: a differing car copy may hold web edits.
    let f = installed_and_edited();
    fs::remove_dir_all(mibpkg::config::base_dir(&f.layout, "carmqtt")).unwrap();
    assert!(gem(&f, &["rescan"]).contains("config conflict"));
    assert_eq!(live(&f), EDITED);
    // A car copy equal to the card's is simply adopted.
    let f = common::fixture();
    offer(&f, &["0.2.1"]);
    put_on_card(&f, MINIMAL);
    mibpkg(&f, &["install", "carmqtt"]);
    fs::remove_dir_all(mibpkg::config::base_dir(&f.layout, "carmqtt")).unwrap();
    assert!(gem(&f, &["rescan"]).starts_with("OK: rescanned; no new config files"));
    put_on_card(&f, EDITED);
    gem(&f, &["rescan"]);
    assert_eq!(live(&f), EDITED, "after adopting, a card change imports again");
}

#[test]
fn purge_forgets_the_card_record() {
    let f = installed_and_edited();
    mibpkg(&f, &["remove", "carmqtt", "--purge"]);
    assert!(!mibpkg::config::base_dir(&f.layout, "carmqtt").exists());
    mibpkg(&f, &["install", "carmqtt"]);
    assert_eq!(live(&f), MINIMAL, "a fresh install seeds from the card again");
}

#[test]
fn packages_without_config_are_untouched() {
    let f = common::fixture();
    common::collection(&f.sd(), "complex256", &[common::pkg_src(f.root(), "plain", "1.0.0", "sleep 30", "")]);
    mibpkg(&f, &["install", "plain"]);
    assert!(gem(&f, &["rescan"]).starts_with("OK: rescanned; no new config files"));
    assert!(!f.layout.config_dir("plain").exists());
    assert!(!mibpkg::config::base_dir(&f.layout, "plain").exists());
}

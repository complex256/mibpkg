mod common;
use mibpkg::ctx::Ctx;
use mibpkg::gem::{self, Item, Screen};
use mibpkg::state::State;
use mibpkg::sys::RecordingSys;
use std::fs;

#[test]
fn render_matches_stock_esd_shape() {
    let s = Screen {
        name: "mibpkg".into(),
        parent: "Main".into(),
        items: vec![
            Item::Text("hello \"world\" \u{2192}".into()),
            Item::Button { label: "Rescan SD card".into(), action: vec!["rescan".into()] },
        ],
    };
    assert_eq!(
        gem::render(&s, "/mnt/app/root/mibpkg/gem"),
        "screen mibpkg Main\n\nkeyValue\n\tvalue String sys 0x00000000 0\n\t\tlabel \"hello 'world' ?\"\n\t\tpoll 0\n\nscript\n\tvalue sys 1 0x100 \"/mnt/app/root/mibpkg/gem/rescan.sh\"\n\tlabel \"Rescan SD card\"\n"
    );
    assert_eq!(gem::script_name(&["install".into(), "car-mqtt".into(), "c".into()]), "install.car-mqtt.c.sh");
    assert_ne!(
        gem::script_name(&["install".into(), "a-b".into(), "c".into()]),
        gem::script_name(&["install".into(), "a".into(), "b-c".into()]),
        "package a-b from c and a from b-c must not share a script"
    );
    assert!(gem::script_body(&["rescan".into()]).contains("gem.sh rescan"));
}

#[test]
fn regenerate_writes_esd_and_scripts_and_cleans_old() {
    let f = common::fixture();
    let sys = RecordingSys::default();
    let ctx = Ctx::new(f.layout.clone(), &sys);
    fs::write(f.layout.engdefs().join("mibpkg_old.esd"), "x").unwrap();
    fs::write(f.layout.engdefs().join("Audio_prod.esd"), "stock").unwrap();
    let a = common::pkg_src(f.root(), "alpha", "1.0.0", "sleep 30", "");
    common::collection(&f.sd(), "complex256", &[a]);
    gem::regenerate(&ctx).unwrap();
    let eng = f.layout.engdefs();
    assert!(eng.join("mibpkg.esd").is_file());
    assert!(!eng.join("mibpkg_alpha.esd").exists(), "no package screens");
    assert!(!eng.join("mibpkg_old.esd").exists());
    assert_eq!(fs::read_to_string(eng.join("Audio_prod.esd")).unwrap(), "stock");
    let script = f.layout.gem_dir().join("web-restart.sh");
    assert!(fs::read_to_string(&script).unwrap().contains("gem.sh web-restart"));
    assert_eq!(*sys.calls.borrow(), vec!["rw /mnt/app", "ro /mnt/app"]);
    gem::remove_screens(&f.layout).unwrap();
    assert!(!eng.join("mibpkg.esd").exists());
}

#[test]
fn live_rows_poll_persistence() {
    let s = Screen {
        name: "mibpkg".into(),
        parent: "Main".into(),
        items: vec![Item::Live { label: "supervisor".into(), key: 1 }],
    };
    assert_eq!(
        gem::render(&s, "/g"),
        format!("screen mibpkg Main\n\nkeyValue\n\tvalue String per {} 0x00000001\n\t\tlabel \"supervisor\"\n\t\tpoll 1000\n", mibpkg::persist::NS)
    );
}

#[test]
fn regenerate_leaves_unchanged_screens_and_persistence_alone() {
    use std::os::unix::fs::MetadataExt;
    let f = common::fixture();
    let sys = RecordingSys::default();
    let ctx = Ctx::new(f.layout.clone(), &sys);
    let mut st = State::default();
    st.results.insert("_".into(), "OK: rescanned".into());
    st.save(&f.layout).unwrap();
    let a = common::pkg_src(f.root(), "alpha", "1.0.0", "sleep 30", "");
    common::collection(&f.sd(), "complex256", &[a]);
    gem::regenerate(&ctx).unwrap();
    let main = f.layout.engdefs().join("mibpkg.esd");
    let script = f.layout.gem_dir().join("web-restart.sh");
    let (ino_main, ino_script) = (std::fs::metadata(&main).unwrap().ino(), std::fs::metadata(&script).unwrap().ino());
    gem::regenerate(&ctx).unwrap();
    assert_eq!(std::fs::metadata(&main).unwrap().ino(), ino_main, "unchanged screen must not be rewritten (an open GEM screen stops polling)");
    assert_eq!(std::fs::metadata(&script).unwrap().ino(), ino_script);
    assert!(std::fs::read_to_string(&main).unwrap().contains(&format!("per {} 0x00000001", mibpkg::persist::NS)), "live supervisor row");
    assert!(sys.persisted.borrow().is_empty(), "results are published by the action that records them, not by every regeneration");
}

#[test]
fn regenerate_still_removes_screens_that_went_away() {
    let f = common::fixture();
    let sys = RecordingSys::default();
    let ctx = Ctx::new(f.layout.clone(), &sys);
    std::fs::write(f.layout.engdefs().join("mibpkg_probe.esd"), "x").unwrap();
    std::fs::create_dir_all(f.layout.gem_dir()).unwrap();
    std::fs::write(f.layout.gem_dir().join("install-old-format.sh"), "x").unwrap();
    gem::regenerate(&ctx).unwrap();
    assert!(!f.layout.engdefs().join("mibpkg_probe.esd").exists());
    assert!(!f.layout.gem_dir().join("install-old-format.sh").exists());
    assert!(f.layout.engdefs().join("mibpkg.esd").exists());
}

fn page(f: &common::Fixture) -> String {
    let sys = RecordingSys::default();
    gem::regenerate(&Ctx::new(f.layout.clone(), &sys)).unwrap();
    fs::read_to_string(f.layout.engdefs().join("mibpkg.esd")).unwrap()
}

#[test]
fn the_only_screen_is_the_mibpkg_page() {
    let f = common::fixture();
    let a = common::pkg_src(f.root(), "alpha", "1.0.0", "sleep 30", "");
    common::collection(&f.sd(), "complex256", &[a]);
    let sys = RecordingSys::default();
    gem::regenerate(&Ctx::new(f.layout.clone(), &sys)).unwrap();
    let esd: Vec<String> = fs::read_dir(f.layout.engdefs()).unwrap().map(|e| e.unwrap().file_name().to_string_lossy().to_string()).collect();
    assert_eq!(esd, ["mibpkg.esd"], "no package screens, no separate web screen");
    let text = fs::read_to_string(f.layout.engdefs().join("mibpkg.esd")).unwrap();
    assert!(text.starts_with("screen mibpkg Main\n"));
    assert!(!text.contains("alpha"), "packages are web-only");
}

#[test]
fn the_page_shows_address_listen_and_pin() {
    let f = common::fixture();
    fs::write(f.layout.run("web.url"), "port 80\nhotspot http://10.255.1.1\ntailscale http://100.64.0.55\niface uap0 10.255.1.1\n").unwrap();
    let p = page(&f);
    assert!(p.contains("hotspot: http://10.255.1.1") && p.contains("tailscale: http://100.64.0.55"), "{p}");
    assert!(p.contains("listen: hotspot, tailscale"));
    assert!(p.contains("PIN: not set"));
    assert!(p.contains("open the address above in a phone browser"));
    for key in [mibpkg::persist::KEY_SUPERVISOR, mibpkg::persist::service_key("mibpkg.web")] {
        assert!(p.contains(&format!("0x{key:08x}")), "live row {key:#x}");
    }
}

#[test]
fn the_page_explains_a_missing_address_or_an_unrecognised_hotspot() {
    let f = common::fixture();
    assert!(page(&f).contains("address: not known yet"));
    fs::write(f.layout.run("web.url"), "port 80\niface uap0 10.255.1.1\niface ecm0 203.0.113.3\n").unwrap();
    let p = page(&f);
    assert!(p.contains("no hotspot or Tailscale address recognised"), "{p}");
    assert!(p.contains("interfaces up: uap0 10.255.1.1, ecm0 203.0.113.3"));
}

#[test]
fn recovery_buttons_are_always_there() {
    let f = common::fixture();
    fs::create_dir_all(f.layout.home()).unwrap();
    for web_toml in ["enabled = false\n", "{{{ broken\n"] {
        fs::write(mibpkg::web::settings::path(&f.layout), web_toml).unwrap();
        let p = page(&f);
        for label in ["Restart web server", "Reset web settings", "Update mibpkg"] {
            assert!(p.contains(label), "{label} with web.toml {web_toml:?}");
        }
        for script in ["web-restart.sh", "web-reset.sh", "self-update.sh"] {
            assert!(f.layout.gem_dir().join(script).is_file(), "{script}");
        }
    }
    let p = page(&f);
    assert!(p.contains("using the defaults"), "the web.toml warning is shown: {p}");
}

#[test]
fn the_page_shows_disable_and_results() {
    let f = common::fixture();
    fs::create_dir_all(f.sd().join("mibpkg")).unwrap();
    fs::write(f.sd().join("mibpkg/DISABLE"), "").unwrap();
    let mut st = State::default();
    st.results.insert("_".into(), "OK: web server restart requested".into());
    st.results.insert("mibpkg".into(), "OK: updated mibpkg 0.4.4 -> 0.5.0".into());
    st.save(&f.layout).unwrap();
    let p = page(&f);
    assert!(p.contains("DISABLE"));
    assert!(p.contains(&format!("0x{:08x}", mibpkg::persist::KEY_RESULT_GLOBAL)));
    assert!(p.contains(&format!("0x{:08x}", mibpkg::persist::result_key("mibpkg"))));
}

#[test]
fn a_set_pin_can_be_removed_from_gem() {
    let f = common::fixture();
    assert!(!page(&f).contains("Remove PIN"), "no PIN, no button");
    let mut s = mibpkg::web::settings::WebSettings::default();
    s.pin = Some(mibpkg::web::settings::hash_pin("2468"));
    mibpkg::web::settings::save(&f.layout, &s).unwrap();
    let p = page(&f);
    assert!(p.contains("PIN: set") && p.contains("Remove PIN"), "{p}");
    assert!(f.layout.gem_dir().join("web-clear-pin.sh").is_file());
}

#[test]
fn upgrading_removes_the_package_and_web_screens() {
    let f = common::fixture();
    // What 0.4.x generated.
    for esd in ["mibpkg_carmqtt.esd", "mibpkg__web.esd"] {
        fs::write(f.layout.engdefs().join(esd), "screen x mibpkg\n").unwrap();
    }
    fs::create_dir_all(f.layout.gem_dir()).unwrap();
    for script in ["restart.carmqtt.sh", "rescan.sh", "check.sh"] {
        fs::write(f.layout.gem_dir().join(script), "x").unwrap();
    }
    page(&f);
    let mut esd: Vec<String> = fs::read_dir(f.layout.engdefs()).unwrap().map(|e| e.unwrap().file_name().to_string_lossy().to_string()).collect();
    esd.sort();
    assert_eq!(esd, ["mibpkg.esd"]);
    let mut scripts: Vec<String> = fs::read_dir(f.layout.gem_dir()).unwrap().map(|e| e.unwrap().file_name().to_string_lossy().to_string()).collect();
    scripts.sort();
    assert_eq!(scripts, ["self-update.sh", "web-reset.sh", "web-restart.sh"]);
}

#[test]
fn update_checks_online_first_and_a_previous_mibpkg_can_be_rolled_back_to() {
    let f = common::fixture();
    let p = page(&f);
    assert!(p.contains("Update mibpkg") && f.layout.gem_dir().join("self-update.sh").is_file(), "{p}");
    assert!(!p.contains("Roll back mibpkg"), "nothing to roll back to yet");
    let mut st = State::default();
    st.packages.insert(
        "mibpkg".into(),
        mibpkg::state::Installed { collection: "complex256-mibpkg".into(), version: "0.5.0".into(), previous: Some("0.4.4".into()), services: vec![], uses_scripts: false },
    );
    st.save(&f.layout).unwrap();
    let p = page(&f);
    assert!(p.contains("Roll back mibpkg to 0.4.4"), "{p}");
    assert!(f.layout.gem_dir().join("rollback.mibpkg.sh").is_file());
}

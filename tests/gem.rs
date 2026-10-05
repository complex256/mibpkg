mod common;
use mibpkg::ctx::Ctx;
use mibpkg::gem::{self, Item, Model, Screen};
use mibpkg::index::Collection;
use mibpkg::install;
use mibpkg::media;
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
    assert_eq!(gem::screen_name("car-mqtt"), "mibpkg_car_mqtt");
    assert!(gem::script_body(&["rescan".into()]).contains("gem.sh rescan"));
}

fn labels(s: &Screen) -> Vec<String> {
    s.items
        .iter()
        .map(|i| match i {
            Item::Text(t) => t.clone(),
            Item::Button { label, .. } => format!("[{label}]"),
            Item::Live { label, .. } => format!("{label} <live>"),
        })
        .collect()
}

#[test]
fn screens_cover_install_update_and_warnings() {
    let f = common::fixture();
    let sys = RecordingSys::default();
    let ctx = Ctx::new(f.layout.clone(), &sys);
    let a1 = common::pkg_src(f.root(), "alpha", "1.0.0", "sleep 30", "");
    let c = Collection::load(&common::collection(&f.sd(), "complex256", &[a1.clone()])).unwrap();
    install::install(&ctx, &[f.sd()], &c, "alpha").unwrap();
    let a2 = common::pkg_src(f.root(), "alpha", "1.1.0", "sleep 30", "");
    let b = common::pkg_src(f.root(), "beta", "0.1.0", "sleep 30", "[scripts]\npost_install = \"bin/beta\"\n");
    let q = common::pkg_src(f.root(), "gamma", "0.1.0", "sleep 30", "[compat]\nunits = [\"MHI2\"]\n");
    common::collection(&f.sd(), "complex256", &[a1, a2, b, q]);
    let scan = media::scan(&f.layout);
    let state = State::load(&f.layout).unwrap();
    let m = Model { layout: &f.layout, unit: &ctx.unit, state: &state, status: None, scan: &scan, online: &[], disabled: false, supervisor_running: false };
    let screens = gem::screens(&m);
    let main = &screens[0];
    assert_eq!((main.name.as_str(), main.parent.as_str()), ("mibpkg", "Main"));
    let ml = labels(main);
    assert!(ml.iter().any(|l| l == "[Update all (1)]"), "{ml:?}");
    assert!(ml.iter().any(|l| l == "[Rescan SD card]"));
    assert!(ml.iter().any(|l| l == "supervisor <live>"), "{ml:?}");
    let alpha = screens.iter().find(|s| s.name == "mibpkg_alpha").unwrap();
    let al = labels(alpha);
    assert!(al.iter().any(|l| l == "installed: 1.0.0"), "{al:?}");
    assert!(al.iter().any(|l| l == "[Update to 1.1.0]"));
    assert!(al.iter().any(|l| l == "[Remove]"));
    let beta = screens.iter().find(|s| s.name == "mibpkg_beta").unwrap();
    let bl = labels(beta);
    assert!(bl.iter().any(|l| l.contains("(!) runs scripts")), "{bl:?}");
    assert!(bl.iter().any(|l| l == "[Install 0.1.0 from complex256]"));
    let gamma = screens.iter().find(|s| s.name == "mibpkg_gamma").unwrap();
    let gl = labels(gamma);
    assert!(gl.iter().any(|l| l.contains("not for this unit")));
    assert!(!gl.iter().any(|l| l.starts_with("[Install")));
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
    assert!(eng.join("mibpkg_alpha.esd").is_file());
    assert!(!eng.join("mibpkg_old.esd").exists());
    assert_eq!(fs::read_to_string(eng.join("Audio_prod.esd")).unwrap(), "stock");
    let script = f.layout.gem_dir().join("install.alpha.complex256.sh");
    assert!(fs::read_to_string(&script).unwrap().contains("gem.sh install alpha complex256"));
    assert_eq!(*sys.calls.borrow(), vec!["rw /mnt/app", "ro /mnt/app"]);
    gem::remove_screens(&f.layout).unwrap();
    assert!(!eng.join("mibpkg.esd").exists());
}

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
    let m = Model { layout: &f.layout, unit: &ctx.unit, state: &state, status: Some(&st), scan: &scan, online: &[], disabled: false, supervisor_running: true };
    let screen = gem::screens(&m).into_iter().find(|s| s.name == "mibpkg_demo").unwrap();
    assert!(labels(&screen).iter().any(|l| l == "demo.demo <live>"), "service rows are live: {:?}", labels(&screen));
}

#[test]
fn install_button_offers_the_newest_compatible_version() {
    let f = common::fixture();
    let sys = RecordingSys::default();
    let ctx = Ctx::new(f.layout.clone(), &sys);
    let old = common::pkg_src(f.root(), "demo", "0.1.0", "sleep 30", "");
    let new = common::pkg_src(f.root(), "demo", "0.2.0", "sleep 30", "[compat]\nunits = [\"MHI2\"]\n");
    common::collection(&f.sd(), "complex256", &[old, new]);
    let scan = media::scan(&f.layout);
    let state = State::load(&f.layout).unwrap();
    let m = Model { layout: &f.layout, unit: &ctx.unit, state: &state, status: None, scan: &scan, online: &[], disabled: false, supervisor_running: false };
    let screen = gem::screens(&m).into_iter().find(|s| s.name == "mibpkg_demo").unwrap();
    let l = labels(&screen);
    assert!(l.iter().any(|x| x == "[Install 0.1.0 from complex256]"), "{l:?}");
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
    let script = f.layout.gem_dir().join("install.alpha.complex256.sh");
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

#[test]
fn online_versions_and_check_button_on_screens() {
    let f = common::fixture();
    let sys = RecordingSys::default();
    let ctx = Ctx::new(f.layout.clone(), &sys);
    let idx: mibpkg::index::Index = serde_json::from_str(
        r#"{"format":1,"collection":{"id":"complex256","name":"complex256 packages"},"packages":[{"name":"demo","version":"2.0.0","file":"demo-2.0.0.mpk","sha256":"0","size":1}]}"#,
    ).unwrap();
    let online = vec![Collection::from_online(idx, "https://h/").unwrap()];
    let scan = media::scan(&f.layout);
    let state = State::load(&f.layout).unwrap();
    let m = Model { layout: &f.layout, unit: &ctx.unit, state: &state, status: None, scan: &scan, online: &online, disabled: false, supervisor_running: false };
    let screens = gem::screens(&m);
    let main = labels(&screens[0]);
    assert!(main.iter().any(|l| l == "[Check for updates]"), "{main:?}");
    assert!(main.iter().any(|l| l == "online <live>"), "{main:?}");
    let demo = labels(screens.iter().find(|s| s.name == "mibpkg_demo").unwrap());
    assert!(demo.iter().any(|l| l == "online [complex256]: 2.0.0"), "{demo:?}");
    assert!(demo.iter().any(|l| l == "[Install 2.0.0 from complex256]"), "{demo:?}");
}

#[test]
fn web_interface_screen_has_status_urls_and_recovery_buttons() {
    let f = common::fixture();
    fs::write(f.layout.run("web.url"), "port 80\nhotspot http://10.255.1.1\n").unwrap();
    let sys = RecordingSys::default();
    let ctx = Ctx::new(f.layout.clone(), &sys);
    gem::regenerate(&ctx).unwrap();
    let esd = fs::read_to_string(f.layout.engdefs().join("mibpkg__web.esd")).unwrap();
    assert!(esd.starts_with("screen mibpkg__web mibpkg\n"), "{esd}");
    assert!(esd.contains("http://10.255.1.1"));
    assert!(esd.contains("listen: hotspot, tailscale"));
    assert!(esd.contains("PIN: not set"));
    assert!(esd.contains("Restart web server") && esd.contains("Reset web settings"));
    assert!(esd.contains(&format!("0x{:08x}", mibpkg::persist::service_key("mibpkg.web"))), "live status row");
    assert!(f.layout.gem_dir().join("web-restart.sh").is_file());
    assert!(f.layout.gem_dir().join("web-reset.sh").is_file());
}

#[test]
fn web_screen_lists_interfaces_when_the_hotspot_is_not_recognised() {
    let f = common::fixture();
    // The server found no hotspot/Tailscale address, but these interfaces are up.
    fs::write(f.layout.run("web.url"), "port 80\niface uap0 10.255.1.1\niface ecm0 10.0.0.2\n").unwrap();
    let sys = RecordingSys::default();
    gem::regenerate(&Ctx::new(f.layout.clone(), &sys)).unwrap();
    let esd = fs::read_to_string(f.layout.engdefs().join("mibpkg__web.esd")).unwrap();
    assert!(esd.contains("interfaces up: uap0 10.255.1.1, ecm0 10.0.0.2"), "{esd}");
    assert!(esd.contains("no hotspot or Tailscale address recognised"), "{esd}");
    assert!(!esd.contains("iface: uap0"), "iface lines are not shown as URLs");
}

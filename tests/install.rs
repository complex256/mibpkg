mod common;
use mibpkg::ctx::Ctx;
use mibpkg::index::Collection;
use mibpkg::install;
use mibpkg::state::State;
use mibpkg::sys::RecordingSys;
use std::fs;

fn setup(f: &common::Fixture, versions: &[&str], extra: &str) -> Collection {
    let srcs: Vec<_> = versions.iter().map(|v| common::pkg_src(f.root(), "demo", v, "sleep 30", extra)).collect();
    Collection::load(&common::collection(&f.sd(), "complex256", &srcs)).unwrap()
}

#[test]
fn fresh_install_commits_and_restores_ro() {
    let f = common::fixture();
    let c = setup(&f, &["1.0.0"], "");
    let sys = RecordingSys::default();
    let ctx = Ctx::new(f.layout.clone(), &sys);
    let msg = install::install(&ctx, &[f.sd()], &c, "demo").unwrap();
    assert_eq!(msg, "installed demo 1.0.0");
    assert_eq!(install::current_version(&f.layout, "demo").as_deref(), Some("1.0.0"));
    assert!(f.layout.pkg_dir("demo").join("1.0.0/bin/demo").is_file());
    assert!(f.layout.boot_d().join("demo.demo.toml").is_file());
    let st = State::load(&f.layout).unwrap();
    assert_eq!(st.packages["demo"].collection, "complex256");
    assert_eq!(st.packages["demo"].services, vec!["demo.demo"]);
    assert!(f.layout.run("reload").exists());
    assert_eq!(*sys.calls.borrow(), vec!["rw /mnt/app", "ro /mnt/app"]);
    assert_eq!(install::install(&ctx, &[f.sd()], &c, "demo").unwrap(), "demo 1.0.0 is already installed");
}

#[test]
fn update_keeps_one_previous_and_rollback_switches_back() {
    let f = common::fixture();
    let sys = RecordingSys::default();
    let ctx = Ctx::new(f.layout.clone(), &sys);
    let c1 = setup(&f, &["1.0.0"], "");
    install::install(&ctx, &[f.sd()], &c1, "demo").unwrap();
    let c2 = setup(&f, &["1.0.0", "1.1.0", "1.2.0"], "");
    assert_eq!(install::updates_available(&State::load(&f.layout).unwrap(), &[c2.clone()], &ctx.unit), vec![("demo".to_string(), "1.2.0".to_string())]);
    assert_eq!(install::install(&ctx, &[f.sd()], &c2, "demo").unwrap(), "updated demo 1.0.0 -> 1.2.0");
    let st = State::load(&f.layout).unwrap();
    assert_eq!(st.packages["demo"].previous.as_deref(), Some("1.0.0"));
    assert!(f.layout.pkg_dir("demo").join("1.0.0").is_dir());
    assert_eq!(install::rollback(&ctx, "demo").unwrap(), "rolled back demo 1.2.0 -> 1.0.0");
    assert_eq!(install::current_version(&f.layout, "demo").as_deref(), Some("1.0.0"));
    // The card still offers 1.2.0 (now the kept "previous" dir): installing it again works.
    let again = install::install(&ctx, &[f.sd()], &Collection::load(&c2.dir).unwrap(), "demo").unwrap();
    assert_eq!(again, "updated demo 1.0.0 -> 1.2.0");
    assert_eq!(install::current_version(&f.layout, "demo").as_deref(), Some("1.2.0"));
}

#[test]
fn corrupt_or_truncated_archive_changes_nothing() {
    let f = common::fixture();
    let sys = RecordingSys::default();
    let ctx = Ctx::new(f.layout.clone(), &sys);
    let c1 = setup(&f, &["1.0.0"], "");
    install::install(&ctx, &[f.sd()], &c1, "demo").unwrap();
    let c2 = setup(&f, &["1.0.0", "1.1.0"], "");
    let a = c2.dir.join("packages/demo-1.1.0.mpk");
    let bytes = fs::read(&a).unwrap();
    fs::write(&a, &bytes[..bytes.len() / 2]).unwrap(); // card pulled mid-copy
    let calls_before = sys.calls.borrow().len();
    let e = install::install(&ctx, &[f.sd()], &c2, "demo").unwrap_err();
    assert!(format!("{e:#}").contains("corrupt"), "{e:#}");
    assert_eq!(install::current_version(&f.layout, "demo").as_deref(), Some("1.0.0"));
    assert!(!f.layout.pkg_dir("demo").join("1.1.0").exists());
    assert_eq!(sys.calls.borrow().len(), calls_before, "never mounted rw");
}

#[test]
fn failing_post_install_rolls_back_the_new_version() {
    let f = common::fixture();
    let sys = RecordingSys::default();
    let ctx = Ctx::new(f.layout.clone(), &sys);
    let src = common::pkg_src(f.root(), "demo", "1.0.0", "sleep 30", "[scripts]\npost_install = \"post.sh\"\n");
    fs::write(src.join("post.sh"), "exit 7\n").unwrap();
    let c = Collection::load(&common::collection(&f.sd(), "complex256", &[src])).unwrap();
    let e = install::install(&ctx, &[f.sd()], &c, "demo").unwrap_err();
    assert!(format!("{e:#}").contains("post_install"));
    assert!(install::current_version(&f.layout, "demo").is_none());
    assert!(!f.layout.pkg_dir("demo").join("1.0.0").exists());
    assert!(State::load(&f.layout).unwrap().packages.is_empty());
    assert_eq!(*sys.calls.borrow(), vec!["rw /mnt/app", "ro /mnt/app"]);
}

#[test]
fn power_loss_leftovers_are_cleaned() {
    let f = common::fixture();
    let sys = RecordingSys::default();
    let ctx = Ctx::new(f.layout.clone(), &sys);
    let c1 = setup(&f, &["1.0.0"], "");
    install::install(&ctx, &[f.sd()], &c1, "demo").unwrap();
    fs::create_dir_all(f.layout.pkg_dir("demo").join("1.1.0.new/bin")).unwrap();
    install::cleanup_stale(&f.layout).unwrap();
    assert!(!f.layout.pkg_dir("demo").join("1.1.0.new").exists());
    assert_eq!(install::current_version(&f.layout, "demo").as_deref(), Some("1.0.0"));
}

#[test]
fn compat_space_and_mount_failures_refuse() {
    let f = common::fixture();
    let sys = RecordingSys::default();
    let ctx = Ctx::new(f.layout.clone(), &sys);
    let c = setup(&f, &["1.0.0"], "[compat]\nunits = [\"MHI2\"]\n");
    assert!(format!("{:#}", install::install(&ctx, &[f.sd()], &c, "demo").unwrap_err()).contains("not compatible"));
    let c = setup(&f, &["1.0.0"], "");
    sys.free.set(Some(10));
    assert!(format!("{:#}", install::install(&ctx, &[f.sd()], &c, "demo").unwrap_err()).contains("not enough space"));
    sys.free.set(None);
    sys.fail_remount_rw.set(true);
    assert!(install::install(&ctx, &[f.sd()], &c, "demo").is_err());
    assert!(install::current_version(&f.layout, "demo").is_none());
}

#[test]
fn same_name_in_two_collections() {
    let f = common::fixture();
    let sys = RecordingSys::default();
    let ctx = Ctx::new(f.layout.clone(), &sys);
    let a = Collection::load(&common::collection(&f.sd(), "complex256", &[common::pkg_src(f.root(), "demo", "1.0.0", "sleep 30", "")])).unwrap();
    let b = Collection::load(&common::collection(&f.sd(), "extras", &[common::pkg_src(f.root(), "demo", "2.0.0", "sleep 30", "")])).unwrap();
    let both = vec![a.clone(), b.clone()];
    let st = State::default();
    assert!(format!("{:#}", install::pick_collection(&st, &both, "demo", None, &ctx.unit).unwrap_err()).contains("several collections"));
    assert_eq!(install::pick_collection(&st, &both, "demo", Some("extras"), &ctx.unit).unwrap().id(), "extras");
    install::install(&ctx, &[f.sd()], &a, "demo").unwrap();
    let st = State::load(&f.layout).unwrap();
    assert_eq!(install::pick_collection(&st, &both, "demo", None, &ctx.unit).unwrap().id(), "complex256");
    let e = install::install(&ctx, &[f.sd()], &b, "demo").unwrap_err();
    assert!(format!("{e:#}").contains("remove it before"), "{e:#}");
}

#[test]
fn remove_and_purge() {
    let f = common::fixture();
    let sys = RecordingSys::default();
    let ctx = Ctx::new(f.layout.clone(), &sys);
    let c = setup(&f, &["1.0.0"], "[[config]]\npath = \"c.toml\"\n");
    install::install(&ctx, &[f.sd()], &c, "demo").unwrap();
    fs::create_dir_all(f.layout.config_dir("demo")).unwrap();
    fs::write(f.layout.config_dir("demo").join("c.toml"), "x").unwrap();
    assert_eq!(install::remove(&ctx, "demo", false).unwrap(), "removed demo");
    assert!(!f.layout.pkg_dir("demo").exists());
    assert!(!f.layout.boot_d().join("demo.demo.toml").exists());
    assert!(f.layout.config_dir("demo").join("c.toml").exists(), "config kept");
    install::install(&ctx, &[f.sd()], &c, "demo").unwrap();
    assert_eq!(install::remove(&ctx, "demo", true).unwrap(), "removed demo and its config");
    assert!(!f.layout.config_dir("demo").exists());
    assert!(install::remove(&ctx, "demo", false).is_err());
    assert!(install::remove(&ctx, "mibpkg", false).is_err());
}

#[test]
fn needs_config_is_reported() {
    let f = common::fixture();
    let sys = RecordingSys::default();
    let ctx = Ctx::new(f.layout.clone(), &sys);
    let c = setup(&f, &["1.0.0"], "[[config]]\npath = \"c.toml\"\nrequired = true\n");
    let msg = install::install(&ctx, &[f.sd()], &c, "demo").unwrap();
    assert!(msg.contains("needs config: c.toml"), "{msg}");
}

#[test]
fn previous_is_only_kept_when_its_dir_exists() {
    // mibpkg registered by bootstrap has no pkgs/ dir for its version: no rollback target.
    let f = common::fixture();
    let sys = RecordingSys::default();
    let ctx = Ctx::new(f.layout.clone(), &sys);
    let mut st = State::default();
    st.packages.insert(
        "demo".into(),
        mibpkg::state::Installed { collection: "complex256".into(), version: "0.9.0".into(), previous: None, services: vec![], uses_scripts: false },
    );
    st.save(&f.layout).unwrap();
    let c = setup(&f, &["1.0.0"], "");
    install::install(&ctx, &[f.sd()], &c, "demo").unwrap();
    assert!(State::load(&f.layout).unwrap().packages["demo"].previous.is_none());
}

#[test]
fn install_reports_progress_steps() {
    use std::cell::RefCell;
    use std::rc::Rc;
    let f = common::fixture();
    let sys = RecordingSys::default();
    let mut ctx = Ctx::new(f.layout.clone(), &sys);
    let seen = Rc::new(RefCell::new(Vec::<String>::new()));
    let s2 = seen.clone();
    ctx.progress = Box::new(move |m: &str| s2.borrow_mut().push(m.to_string()));
    let c = setup(&f, &["1.0.0"], "");
    install::install(&ctx, &[f.sd()], &c, "demo").unwrap();
    let got = seen.borrow().join(" | ");
    for step in ["checking demo 1.0.0", "unpacking", "switching demo to 1.0.0", "starting services"] {
        assert!(got.contains(step), "missing {step:?} in {got}");
    }
}

#[test]
fn power_loss_after_current_flip_is_repaired_not_deleted() {
    let f = common::fixture();
    let sys = RecordingSys::default();
    let ctx = Ctx::new(f.layout.clone(), &sys);
    let c1 = setup(&f, &["1.0.0"], "");
    install::install(&ctx, &[f.sd()], &c1, "demo").unwrap();
    let c2 = setup(&f, &["1.0.0", "1.1.0"], "");
    install::install(&ctx, &[f.sd()], &c2, "demo").unwrap();
    // Power lost after `current` flipped to 1.1.0 but before the state file was saved.
    let mut st = State::load(&f.layout).unwrap();
    let e = st.packages.get_mut("demo").unwrap();
    e.version = "1.0.0".into();
    e.previous = None;
    st.save(&f.layout).unwrap();
    let msg = install::install(&ctx, &[f.sd()], &c2, "demo").unwrap();
    assert!(msg.contains("repaired"), "{msg}");
    assert!(f.layout.pkg_dir("demo").join("1.1.0/bin/demo").is_file(), "the active version must survive");
    assert_eq!(install::current_version(&f.layout, "demo").as_deref(), Some("1.1.0"));
    assert_eq!(State::load(&f.layout).unwrap().packages["demo"].version, "1.1.0");
}

#[test]
fn every_action_cleans_interrupted_installs() {
    let f = common::fixture();
    let sys = RecordingSys::default();
    let ctx = Ctx::new(f.layout.clone(), &sys);
    let c = setup(&f, &["1.0.0"], "");
    install::install(&ctx, &[f.sd()], &c, "demo").unwrap();
    let stale = f.layout.pkg_dir("other").join("2.0.0.new");
    fs::create_dir_all(&stale).unwrap();
    install::remove(&ctx, "demo", false).unwrap();
    assert!(!stale.exists());
}

#[test]
fn newest_compatible_version_is_chosen() {
    let f = common::fixture();
    let sys = RecordingSys::default();
    let ctx = Ctx::new(f.layout.clone(), &sys);
    let old = common::pkg_src(f.root(), "demo", "1.0.0", "sleep 30", "");
    let new = common::pkg_src(f.root(), "demo", "2.0.0", "sleep 30", "[compat]\nunits = [\"MHI2\"]\n");
    let c = Collection::load(&common::collection(&f.sd(), "complex256", &[old, new])).unwrap();
    assert_eq!(install::install(&ctx, &[f.sd()], &c, "demo").unwrap(), "installed demo 1.0.0");
    assert!(install::updates_available(&State::load(&f.layout).unwrap(), &[c.clone()], &ctx.unit).is_empty(), "2.0.0 is not for this unit");
}

#[test]
fn unknown_free_space_is_reported() {
    use std::cell::RefCell;
    use std::rc::Rc;
    let f = common::fixture();
    let sys = RecordingSys::default(); // free: None = df unreadable
    let mut ctx = Ctx::new(f.layout.clone(), &sys);
    let seen = Rc::new(RefCell::new(Vec::<String>::new()));
    let s2 = seen.clone();
    ctx.progress = Box::new(move |m: &str| s2.borrow_mut().push(m.to_string()));
    let c = setup(&f, &["1.0.0"], "");
    install::install(&ctx, &[f.sd()], &c, "demo").unwrap();
    assert!(seen.borrow().iter().any(|m| m.contains("free space unknown")), "{:?}", seen.borrow());
}

#[test]
fn a_package_without_services_installs_without_a_boot_d_dir() {
    let f = common::fixture();
    let sys = RecordingSys::default();
    let ctx = Ctx::new(f.layout.clone(), &sys);
    let s = f.root().join("src-lib");
    fs::create_dir_all(s.join("share")).unwrap();
    fs::write(s.join("manifest.toml"), "[package]\nname = \"lib\"\nversion = \"1.0.0\"\n\n[[file]]\nsrc = \"share/data.txt\"\n").unwrap();
    fs::write(s.join("share/data.txt"), "x").unwrap();
    let c = Collection::load(&common::collection(&f.sd(), "complex256", &[s])).unwrap();
    assert!(!f.layout.boot_d().exists());
    assert_eq!(install::install(&ctx, &[f.sd()], &c, "lib").unwrap(), "installed lib 1.0.0");
}

#[test]
fn repair_is_not_blocked_by_the_free_space_check() {
    let f = common::fixture();
    let sys = RecordingSys::default();
    let ctx = Ctx::new(f.layout.clone(), &sys);
    let c1 = setup(&f, &["1.0.0"], "");
    install::install(&ctx, &[f.sd()], &c1, "demo").unwrap();
    let c2 = setup(&f, &["1.0.0", "1.1.0"], "");
    install::install(&ctx, &[f.sd()], &c2, "demo").unwrap();
    let mut st = State::load(&f.layout).unwrap();
    st.packages.get_mut("demo").unwrap().version = "1.0.0".into();
    st.save(&f.layout).unwrap();
    sys.free.set(Some(10)); // disk nearly full: old and new versions are both still there
    let msg = install::install(&ctx, &[f.sd()], &c2, "demo").unwrap();
    assert!(msg.contains("repaired"), "{msg}");
}

#[test]
fn self_update_installs_the_ca_bundle() {
    let f = common::fixture();
    let sys = RecordingSys::default();
    let ctx = Ctx::new(f.layout.clone(), &sys);
    let s = f.root().join("src-mibpkg");
    fs::create_dir_all(s.join("bin")).unwrap();
    fs::write(s.join("manifest.toml"), "[package]\nname = \"mibpkg\"\nversion = \"9.0.0\"\n\n[[file]]\nsrc = \"bin/mibpkg\"\nmode = \"755\"\n\n[[file]]\nsrc = \"bin/launch.sh\"\nmode = \"755\"\n\n[[file]]\nsrc = \"bin/gem.sh\"\nmode = \"755\"\n\n[[file]]\nsrc = \"bin/ca.pem\"\n").unwrap();
    for n in ["mibpkg", "launch.sh", "gem.sh", "ca.pem"] {
        fs::write(s.join("bin").join(n), n).unwrap();
    }
    let c = Collection::load(&common::collection(&f.sd(), "complex256", &[s])).unwrap();
    install::install(&ctx, &[f.sd()], &c, "mibpkg").unwrap();
    assert_eq!(fs::read_to_string(f.layout.bin_dir().join("ca.pem")).unwrap(), "ca.pem");
}

#[test]
fn a_self_update_hands_the_supervisor_restart_to_launch_sh() {
    // On the unit (2026-10-07) the `gem` action that installed a new mibpkg finished by starting a
    // supervisor itself (the "make sure one runs" step), from its own old binary, in the gap before
    // launch.sh started the new one; launch.sh's supervisor then found one running and exited.
    let f = common::fixture();
    assert!(!mibpkg::control::supervisor_start_deferred(&f.layout), "nothing pending");
    let src = f.root().join("src-mibpkg");
    fs::create_dir_all(src.join("bin")).unwrap();
    fs::write(src.join("manifest.toml"), "[package]\nname = \"mibpkg\"\nversion = \"9.9.9\"\n\n[[file]]\nsrc = \"bin/mibpkg\"\nmode = \"755\"\n\n[[file]]\nsrc = \"bin/launch.sh\"\nmode = \"755\"\n\n[[file]]\nsrc = \"bin/gem.sh\"\nmode = \"755\"\n").unwrap();
    for n in ["mibpkg", "launch.sh", "gem.sh"] {
        fs::write(src.join("bin").join(n), n).unwrap();
    }
    let c = Collection::load(&common::collection(&f.sd(), "complex256-mibpkg", &[src])).unwrap();
    let sys = RecordingSys::default();
    let ctx = Ctx::new(f.layout.clone(), &sys);
    let msg = install::install(&ctx, &[f.sd()], &c, "mibpkg").unwrap();
    assert!(msg.contains("restarts itself"), "{msg}");
    assert!(mibpkg::control::supervisor_start_deferred(&f.layout), "launch.sh starts the next supervisor");
    // The new supervisor clears it when it starts.
    mibpkg::control::selfupdate_done(&f.layout);
    assert!(!mibpkg::control::supervisor_start_deferred(&f.layout));
}

fn card_colls(f: &common::Fixture) -> Vec<mibpkg::index::Collection> {
    mibpkg::media::loaded(&mibpkg::media::scan(&f.layout))
}

/// demo 1.0.0 installed from collection "old"; then the card only has `ids` offering 1.1.0.
fn moved_setup(ids: &[&str]) -> (common::Fixture, RecordingSys) {
    let f = common::fixture();
    let sys = RecordingSys::default();
    let v1 = common::pkg_src(f.root(), "demo", "1.0.0", "sleep 30", "");
    let old = common::collection(&f.sd(), "old", &[v1]);
    {
        let ctx = Ctx::new(f.layout.clone(), &sys);
        let colls = card_colls(&f);
        let c = install::pick_collection(&State::default(), &colls, "demo", Some("old"), &ctx.unit).unwrap();
        install::install(&ctx, &mibpkg::media::roots(&f.layout), c, "demo").unwrap();
    }
    std::fs::remove_dir_all(old).unwrap();
    let v2 = common::pkg_src(f.root(), "demo", "1.1.0", "sleep 30", "");
    for id in ids {
        common::collection(&f.sd(), id, &[v2.clone()]);
    }
    (f, sys)
}

#[test]
fn a_package_whose_collection_is_gone_updates_from_the_one_that_has_it() {
    let (f, sys) = moved_setup(&["new"]);
    let ctx = Ctx::new(f.layout.clone(), &sys);
    let state = State::load(&f.layout).unwrap();
    let colls = card_colls(&f);
    assert_eq!(install::update_sources("old", "demo", &colls), vec!["new".to_string()]);
    assert_eq!(install::updates_available(&state, &colls, &ctx.unit), vec![("demo".into(), "1.1.0".into())]);
    let c = install::pick_collection(&state, &colls, "demo", None, &ctx.unit).unwrap();
    assert_eq!(c.id(), "new");
    install::install(&ctx, &mibpkg::media::roots(&f.layout), c, "demo").unwrap();
    assert_eq!(State::load(&f.layout).unwrap().packages["demo"].collection, "new");
}

#[test]
fn several_new_homes_need_a_choice() {
    let (f, sys) = moved_setup(&["new-a", "new-b"]);
    let ctx = Ctx::new(f.layout.clone(), &sys);
    let state = State::load(&f.layout).unwrap();
    let colls = card_colls(&f);
    assert!(install::updates_available(&state, &colls, &ctx.unit).is_empty());
    let e = install::pick_collection(&state, &colls, "demo", None, &ctx.unit).unwrap_err().to_string();
    assert!(e.contains("new-a") && e.contains("new-b"), "{e}");
    let c = install::pick_collection(&state, &colls, "demo", Some("new-b"), &ctx.unit).unwrap();
    install::install(&ctx, &mibpkg::media::roots(&f.layout), c, "demo").unwrap();
    assert_eq!(State::load(&f.layout).unwrap().packages["demo"].collection, "new-b");
}

#[test]
fn the_cross_collection_guard_holds_while_the_recorded_collection_is_present() {
    let (f, sys) = moved_setup(&["new"]);
    // Put "old" back (still offering 1.0.0): it is present again, so "new" is refused.
    let v1 = common::pkg_src(f.root(), "demo", "1.0.0", "sleep 30", "");
    common::collection(&f.sd(), "old", &[v1]);
    let ctx = Ctx::new(f.layout.clone(), &sys);
    let state = State::load(&f.layout).unwrap();
    let colls = card_colls(&f);
    assert_eq!(install::update_sources("old", "demo", &colls), vec!["old".to_string()]);
    let c = install::pick_collection(&state, &colls, "demo", Some("new"), &ctx.unit).unwrap();
    let e = install::install(&ctx, &mibpkg::media::roots(&f.layout), c, "demo").unwrap_err().to_string();
    assert!(e.contains("remove it before installing it from new"), "{e}");
}

#[test]
fn an_update_from_a_new_collection_says_so() {
    let (f, sys) = moved_setup(&["new"]);
    let ctx = Ctx::new(f.layout.clone(), &sys);
    let colls = card_colls(&f);
    let c = install::pick_collection(&State::load(&f.layout).unwrap(), &colls, "demo", None, &ctx.unit).unwrap();
    let msg = install::install(&ctx, &mibpkg::media::roots(&f.layout), c, "demo").unwrap();
    assert!(msg.contains("now from collection new (was old)"), "{msg}");
}

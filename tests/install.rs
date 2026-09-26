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
    assert_eq!(install::updates_available(&State::load(&f.layout).unwrap(), &[c2.clone()]), vec![("demo".to_string(), "1.2.0".to_string())]);
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
    assert!(format!("{:#}", install::pick_collection(&st, &both, "demo", None).unwrap_err()).contains("several collections"));
    assert_eq!(install::pick_collection(&st, &both, "demo", Some("extras")).unwrap().id(), "extras");
    install::install(&ctx, &[f.sd()], &a, "demo").unwrap();
    let st = State::load(&f.layout).unwrap();
    assert_eq!(install::pick_collection(&st, &both, "demo", None).unwrap().id(), "complex256");
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

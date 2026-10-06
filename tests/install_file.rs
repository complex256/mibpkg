//! Installing and upgrading from an uploaded .mpk file (web interface uploads).
mod common;
use mibpkg::ctx::Ctx;
use mibpkg::index::Collection;
use mibpkg::install;
use mibpkg::state::State;
use mibpkg::sys::RecordingSys;
use std::fs;
use std::path::PathBuf;

/// A standalone .mpk (as a user would upload it) in the temp dir, outside any collection.
fn mpk(f: &common::Fixture, name: &str, version: &str, extra: &str) -> PathBuf {
    let src = common::pkg_src(f.root(), name, version, "sleep 30", extra);
    let out = f.root().join(format!("upload-{name}-{version}.mpk"));
    mibpkg::archive::pack(&src, &out).unwrap();
    out
}

/// A card collection `id` holding demo at `versions`.
fn card(f: &common::Fixture, id: &str, versions: &[&str]) -> Collection {
    let srcs: Vec<_> = versions.iter().map(|v| common::pkg_src(f.root(), "demo", v, "sleep 30", "")).collect();
    Collection::load(&common::collection(&f.sd(), id, &srcs)).unwrap()
}

fn err(r: anyhow::Result<String>) -> String {
    format!("{:#}", r.unwrap_err())
}

#[test]
fn fresh_install_records_the_chosen_collection() {
    let f = common::fixture();
    let sys = RecordingSys::default();
    let ctx = Ctx::new(f.layout.clone(), &sys);
    let colls = vec![card(&f, "complex256", &["0.9.0"])];
    let file = mpk(&f, "demo", "1.0.0", "");
    let msg = install::install_file(&ctx, &[f.sd()], &colls, &file, "demo", Some("complex256")).unwrap();
    assert_eq!(msg, "installed demo 1.0.0");
    let st = State::load(&f.layout).unwrap();
    assert_eq!(st.packages["demo"].collection, "complex256");
    assert!(f.layout.pkg_dir("demo").join("1.0.0/bin/demo").is_file());
}

#[test]
fn none_records_uploaded_which_never_offers_updates() {
    let f = common::fixture();
    let sys = RecordingSys::default();
    let ctx = Ctx::new(f.layout.clone(), &sys);
    let colls = vec![card(&f, "complex256", &["2.0.0"])];
    let file = mpk(&f, "demo", "1.0.0", "");
    install::install_file(&ctx, &[f.sd()], &colls, &file, "demo", Some("-")).unwrap();
    let st = State::load(&f.layout).unwrap();
    assert_eq!(st.packages["demo"].collection, "uploaded");
    assert!(install::updates_available(&st, &colls, &ctx.unit).is_empty());
    let e = install::pick_collection(&st, &colls, "demo", None, &ctx.unit).unwrap_err();
    assert!(format!("{e:#}").contains("uploaded file"), "{e:#}");
}

#[test]
fn upgrade_keeps_the_recorded_collection() {
    let f = common::fixture();
    let sys = RecordingSys::default();
    let ctx = Ctx::new(f.layout.clone(), &sys);
    let c = card(&f, "complex256", &["1.0.0"]);
    install::install(&ctx, &[f.sd()], &c, "demo").unwrap();
    let other = card(&f, "community", &["1.0.0"]);
    let file = mpk(&f, "demo", "1.1.0", "");
    let msg = install::install_file(&ctx, &[f.sd()], &[c, other], &file, "demo", Some("community")).unwrap();
    assert_eq!(msg, "updated demo 1.0.0 -> 1.1.0");
    let st = State::load(&f.layout).unwrap();
    assert_eq!(st.packages["demo"].collection, "complex256");
    assert_eq!(st.packages["demo"].previous.as_deref(), Some("1.0.0"));
}

#[test]
fn same_and_older_versions_change_nothing() {
    let f = common::fixture();
    let sys = RecordingSys::default();
    let ctx = Ctx::new(f.layout.clone(), &sys);
    let c = card(&f, "complex256", &["1.1.0"]);
    install::install(&ctx, &[f.sd()], &c, "demo").unwrap();
    let same = mpk(&f, "demo", "1.1.0", "");
    assert_eq!(install::install_file(&ctx, &[f.sd()], &[], &same, "demo", None).unwrap(), "demo 1.1.0 is already installed");
    let older = mpk(&f, "demo", "1.0.0", "");
    assert!(err(install::install_file(&ctx, &[f.sd()], &[], &older, "demo", None)).contains("Roll back"));
    assert_eq!(install::current_version(&f.layout, "demo").as_deref(), Some("1.1.0"));
}

#[test]
fn wrong_name_incompatible_corrupt_and_unknown_collection_are_refused() {
    let f = common::fixture();
    let sys = RecordingSys::default();
    let ctx = Ctx::new(f.layout.clone(), &sys);
    let file = mpk(&f, "demo", "1.0.0", "");
    assert!(err(install::install_file(&ctx, &[f.sd()], &[], &file, "other", Some("-"))).contains("holds demo"));

    let mhi2 = mpk(&f, "old", "1.0.0", "[compat]\nunits = [\"MHI2\"]\n");
    assert!(err(install::install_file(&ctx, &[f.sd()], &[], &mhi2, "old", Some("-"))).contains("not compatible"));

    let bytes = fs::read(&file).unwrap();
    let broken = f.root().join("broken.mpk");
    fs::write(&broken, &bytes[..bytes.len() / 2]).unwrap();
    assert!(err(install::install_file(&ctx, &[f.sd()], &[], &broken, "demo", Some("-"))).contains("not a valid package"));

    assert!(err(install::install_file(&ctx, &[f.sd()], &[], &file, "demo", Some("nowhere"))).contains("nowhere"));
    assert!(err(install::install_file(&ctx, &[f.sd()], &[], &file, "demo", None)).contains("collection"));
    assert!(sys.calls.borrow().is_empty(), "nothing was mounted read-write");
    assert!(State::load(&f.layout).unwrap().packages.is_empty());
}

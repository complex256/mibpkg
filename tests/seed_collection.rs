mod common;
use mibpkg::ctx::Ctx;
use mibpkg::sys::RecordingSys;
use std::fs;

#[test]
fn seed_installs_once_then_marks_done() {
    let f = common::fixture();
    let sys = RecordingSys::default();
    let ctx = Ctx::new(f.layout.clone(), &sys);
    let src = common::pkg_src(f.root(), "demo", "1.0.0", "exit 0", "");
    common::collection(&f.sd(), "seed", &[src]);
    mibpkg::supervisor::seed_once(&ctx, &[f.sd()]).unwrap();
    assert!(f.layout.state_dir().join("seeded").exists());
    assert!(f.layout.state_dir().join("installed.json").exists());
    let before = fs::read(f.layout.state_dir().join("installed.json")).unwrap();
    mibpkg::supervisor::seed_once(&ctx, &[f.sd()]).unwrap();
    assert_eq!(fs::read(f.layout.state_dir().join("installed.json")).unwrap(), before);
}

#[test]
fn seed_requests_rw_for_its_marker_even_with_no_collections() {
    // On the unit /mnt/app is read-only after the installs; the marker write must take its own
    // rw mount or it silently fails and seed re-runs every boot.
    let f = common::fixture();
    let sys = RecordingSys::default();
    let ctx = Ctx::new(f.layout.clone(), &sys);
    mibpkg::supervisor::seed_once(&ctx, &[]).unwrap(); // no media => nothing to install
    assert!(f.layout.state_dir().join("seeded").exists(), "marker not written");
    assert!(sys.calls.borrow().iter().any(|c| c == "rw /mnt/app"), "no rw mount taken for the marker: {:?}", sys.calls.borrow());
}

#[test]
fn gem_once_enables_on_first_boot_then_not_again() {
    use mibpkg::gem_enable::{GEM_DEV_KEY, COMMIT_KEY};
    let f = common::fixture();
    let sys = RecordingSys::default();
    let ctx = Ctx::new(f.layout.clone(), &sys);
    mibpkg::supervisor::gem_once(&ctx).unwrap();
    assert_eq!(sys.persisted_bytes.borrow().clone(), vec![(0, GEM_DEV_KEY, 1u8), (0, COMMIT_KEY, 0u8)]);
    assert!(f.layout.state_dir().join("gem-enabled").exists());
    // second boot: marker present, no further writes (respects a later disable-gem)
    mibpkg::supervisor::gem_once(&ctx).unwrap();
    assert_eq!(sys.persisted_bytes.borrow().len(), 2, "gem_once re-enabled on a later boot");
}

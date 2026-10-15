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

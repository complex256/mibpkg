mod common;
use mibpkg::ctx::Ctx;
use mibpkg::lock::Lock;
use mibpkg::sys::{RealSys, RecordingSys, Sys};
use std::fs;
use std::time::Duration;

#[test]
fn nested_rw_remounts_once_and_restores_ro() {
    let f = common::fixture();
    let sys = RecordingSys::default();
    let ctx = Ctx::new(f.layout.clone(), &sys);
    {
        let _a = ctx.mounts.rw("/mnt/app").unwrap();
        {
            let _b = ctx.mounts.rw("/mnt/app").unwrap();
            let _c = ctx.mounts.rw("/mnt/system").unwrap();
        }
        assert_eq!(*sys.calls.borrow(), vec!["rw /mnt/app", "rw /mnt/system", "ro /mnt/system"]);
    }
    assert_eq!(*sys.calls.borrow(), vec!["rw /mnt/app", "rw /mnt/system", "ro /mnt/system", "ro /mnt/app"]);
    assert_eq!(ctx.unit.unit.as_deref(), Some("MHI2Q"));
}

#[test]
fn failed_rw_mount_is_an_error_and_not_counted() {
    let f = common::fixture();
    let sys = RecordingSys::default();
    sys.fail_remount_rw.set(true);
    let ctx = Ctx::new(f.layout.clone(), &sys);
    assert!(ctx.mounts.rw("/mnt/app").is_err());
    sys.fail_remount_rw.set(false);
    drop(ctx.mounts.rw("/mnt/app").unwrap());
    assert_eq!(*sys.calls.borrow(), vec!["rw /mnt/app", "rw /mnt/app", "ro /mnt/app"]);
}

#[test]
fn lock_is_exclusive_and_steals_stale_locks() {
    let f = common::fixture();
    let sys = RealSys { mounts: false };
    let l = Lock::acquire(&f.layout, &sys).unwrap();
    let e = Lock::acquire(&f.layout, &sys).unwrap_err();
    assert!(format!("{e}").starts_with("busy"), "{e}");
    drop(l);
    // A lock left by a dead process is taken over.
    fs::write(f.layout.run("lock"), "999999").unwrap();
    let _l2 = Lock::acquire(&f.layout, &sys).unwrap();
}

#[test]
fn run_script_success_failure_timeout() {
    let t = tempfile::tempdir().unwrap();
    let s = t.path().join("s.sh");
    let sys = RealSys { mounts: false };
    fs::write(&s, "[ \"$1\" = go ] && [ \"$X\" = y ] && exit 0; exit 3\n").unwrap();
    sys.run_script(&s, &["go"], &[("X".into(), "y".into())], Duration::from_secs(5)).unwrap();
    assert!(sys.run_script(&s, &["no"], &[], Duration::from_secs(5)).is_err());
    fs::write(&s, "sleep 5\n").unwrap();
    let e = sys.run_script(&s, &[], &[], Duration::from_millis(300)).unwrap_err();
    assert!(format!("{e}").contains("timed out"));
}

#[test]
fn process_alive_and_terminate() {
    let sys = RealSys { mounts: false };
    let mut child = std::process::Command::new("sleep").arg("30").spawn().unwrap();
    assert!(sys.process_alive(child.id()));
    sys.terminate(child.id());
    child.wait().unwrap();
    assert!(!sys.process_alive(child.id()));
    assert!(sys.free_bytes(std::path::Path::new("/")).unwrap_or(1) > 0);
}

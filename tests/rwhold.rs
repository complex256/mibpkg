mod common;
use mibpkg::ctx::Mounts;
use mibpkg::rwhold::{self, RWLOCK_WAIT};
use mibpkg::sys::RecordingSys;
use std::fs;
use std::path::Path;
use std::process::{Child, Command};
use std::time::{Duration, Instant};

/// A live process whose pid stands in for another mibpkg; killed and reaped on drop.
struct Live(Child);

impl Live {
    fn spawn() -> Live {
        Live(Command::new("sleep").arg("30").spawn().unwrap())
    }
    fn pid(&self) -> u32 {
        self.0.id()
    }
}

impl Drop for Live {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

/// The pid of a process that has exited and been reaped.
fn dead_pid() -> u32 {
    let mut c = Command::new("true").spawn().unwrap();
    c.wait().unwrap();
    c.id()
}

fn holders(tmp: &Path, tag: &str) -> Vec<String> {
    let prefix = format!("mibpkg.rw.{tag}.");
    let mut v: Vec<String> = fs::read_dir(tmp)
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
        .filter(|n| n.starts_with(&prefix))
        .collect();
    v.sort();
    v
}

#[test]
fn other_live_holder_keeps_app_rw() {
    let f = common::fixture();
    let sys = RecordingSys::default();
    let other = Live::spawn();
    let a = Mounts::new(&sys, &f.layout);
    let b = Mounts::with_holder(&sys, &f.layout, other.pid());
    let ga = a.rw("/mnt/app").unwrap();
    let gb = b.rw("/mnt/app").unwrap();
    drop(ga);
    assert_eq!(*sys.calls.borrow(), vec!["rw /mnt/app", "rw /mnt/app"]);
    assert_eq!(holders(&f.layout.p("/tmp"), "app"), vec![format!("mibpkg.rw.app.{}", other.pid())]);
    drop(gb);
    assert_eq!(sys.calls.borrow().last().unwrap(), "ro /mnt/app");
    assert!(holders(&f.layout.p("/tmp"), "app").is_empty());
}

#[test]
fn dead_holder_is_ignored_and_swept() {
    let f = common::fixture();
    let sys = RecordingSys::default();
    let stale = f.layout.run(&format!("rw.app.{}", dead_pid()));
    fs::write(&stale, "").unwrap();
    let m = Mounts::new(&sys, &f.layout);
    drop(m.rw("/mnt/app").unwrap());
    assert_eq!(sys.calls.borrow().last().unwrap(), "ro /mnt/app");
    assert!(!stale.exists());
    assert!(holders(&f.layout.p("/tmp"), "app").is_empty());
}

#[test]
fn nested_guards_in_one_process_touch_the_registry_once() {
    let f = common::fixture();
    let sys = RecordingSys::default();
    let m = Mounts::new(&sys, &f.layout);
    let own = f.layout.run(&format!("rw.app.{}", std::process::id()));
    let outer = m.rw("/mnt/app").unwrap();
    let inner = m.rw("/mnt/app").unwrap();
    assert!(own.exists());
    drop(inner);
    assert!(own.exists());
    assert_eq!(*sys.calls.borrow(), vec!["rw /mnt/app"]);
    drop(outer);
    assert!(!own.exists());
    assert_eq!(*sys.calls.borrow(), vec!["rw /mnt/app", "ro /mnt/app"]);
}

#[test]
fn reassert_remounts_rw_without_touching_holders() {
    let f = common::fixture();
    let sys = RecordingSys::default();
    let other = Live::spawn();
    let a = Mounts::new(&sys, &f.layout);
    let b = Mounts::with_holder(&sys, &f.layout, other.pid());
    let _ga = a.rw("/mnt/app").unwrap();
    let _gb = b.rw("/mnt/app").unwrap();
    let before = holders(&f.layout.p("/tmp"), "app");
    assert_eq!(before.len(), 2);
    a.reassert("/mnt/app").unwrap();
    assert_eq!(holders(&f.layout.p("/tmp"), "app"), before);
    assert_eq!(*sys.calls.borrow(), vec!["rw /mnt/app", "rw /mnt/app", "rw /mnt/app"]);
}

#[test]
fn reassert_without_a_guard_is_an_error() {
    let f = common::fixture();
    let sys = RecordingSys::default();
    let m = Mounts::new(&sys, &f.layout);
    assert!(m.reassert("/mnt/app").is_err());
    {
        let _g = m.rw("/mnt/app").unwrap();
    }
    assert!(m.reassert("/mnt/app").is_err());
    assert_eq!(*sys.calls.borrow(), vec!["rw /mnt/app", "ro /mnt/app"]);
}

#[test]
fn remount_lock_held_by_live_pid_times_out_with_busy() {
    let f = common::fixture();
    let sys = RecordingSys::default();
    let other = Live::spawn();
    fs::write(f.layout.run("rwlock"), other.pid().to_string()).unwrap();
    let m = Mounts::new(&sys, &f.layout);
    let t = Instant::now();
    let e = m.rw("/mnt/app").err().expect("rw under a held remount lock must fail");
    let took = t.elapsed();
    assert!(format!("{e:#}").contains("busy"), "{e:#}");
    assert!(took >= RWLOCK_WAIT, "{took:?}");
    assert!(took < RWLOCK_WAIT + Duration::from_secs(1), "{took:?}");
    assert!(sys.calls.borrow().is_empty());
    assert!(holders(&f.layout.p("/tmp"), "app").is_empty());
    // The failed acquire is not counted: no guard, so reassert still refuses.
    assert!(m.reassert("/mnt/app").is_err());
}

#[test]
fn stale_remount_lock_is_taken_over() {
    let f = common::fixture();
    let sys = RecordingSys::default();
    let lock = f.layout.run("rwlock");
    fs::write(&lock, dead_pid().to_string()).unwrap();
    rwhold::acquire(&f.layout, &sys, "/mnt/app", std::process::id()).unwrap();
    assert!(!lock.exists(), "the remount lock is released after use");
    rwhold::release(&f.layout, &sys, "/mnt/app", std::process::id()).unwrap();
    assert_eq!(*sys.calls.borrow(), vec!["rw /mnt/app", "ro /mnt/app"]);
}

#[test]
fn unknown_mount_is_rejected() {
    assert_eq!(rwhold::tag("/mnt/app").unwrap(), "app");
    assert_eq!(rwhold::tag("/mnt/system").unwrap(), "system");
    assert!(rwhold::tag("/mnt/persist").is_err());
}

#[test]
fn failed_rw_remount_leaves_no_holder_file() {
    let f = common::fixture();
    let sys = RecordingSys::default();
    sys.fail_remount_rw.set(true);
    let m = Mounts::new(&sys, &f.layout);
    assert!(m.rw("/mnt/app").is_err());
    assert!(holders(&f.layout.p("/tmp"), "app").is_empty());
}

#[test]
fn rw_and_rwlock_are_reserved_package_names() {
    for n in ["rw", "rwlock"] {
        assert!(mibpkg::manifest::RESERVED.contains(&n), "{n}");
    }
}

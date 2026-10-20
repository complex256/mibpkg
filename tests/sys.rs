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

#[test]
fn lock_file_without_its_pid_yet_is_busy_not_stale() {
    let f = common::fixture();
    let sys = RealSys { mounts: false };
    let p = f.layout.run("lock");
    fs::write(&p, "").unwrap(); // another process has created the lock but not written its pid
    let p2 = p.clone();
    let w = std::thread::spawn(move || {
        std::thread::sleep(Duration::from_millis(30));
        fs::write(&p2, std::process::id().to_string()).unwrap();
    });
    let e = Lock::acquire(&f.layout, &sys).unwrap_err();
    assert!(format!("{e}").starts_with("busy"), "{e}");
    w.join().unwrap();
}

#[test]
fn liveness_comes_from_proc_without_starting_a_process() {
    use mibpkg::sys::alive_in_proc;
    let t = tempfile::tempdir().unwrap();
    // No /proc/self: this system has no usable /proc (macOS); the caller falls back to kill -0.
    assert_eq!(alive_in_proc(t.path(), 123), None);
    std::fs::create_dir_all(t.path().join("self")).unwrap();
    std::fs::create_dir_all(t.path().join("123")).unwrap();
    assert_eq!(alive_in_proc(t.path(), 123), Some(true));
    assert_eq!(alive_in_proc(t.path(), 456), Some(false));
}

/// Liveness from the host, with a hook run whenever a pid is found dead: it simulates another
/// process acting on the lock between our liveness check and our takeover.
struct RacingSys<F: Fn(u32)> {
    on_dead: F,
}

impl<F: Fn(u32)> Sys for RacingSys<F> {
    fn remount(&self, _mount: &str, _rw: bool) -> anyhow::Result<()> {
        Ok(())
    }
    fn shell_syntax_ok(&self, _script: &std::path::Path) -> bool {
        true
    }
    fn run_script(&self, _s: &std::path::Path, _a: &[&str], _e: &[(String, String)], _t: Duration) -> anyhow::Result<()> {
        Ok(())
    }
    fn process_alive(&self, pid: u32) -> bool {
        let alive = (RealSys { mounts: false }).process_alive(pid);
        if !alive {
            (self.on_dead)(pid);
        }
        alive
    }
    fn terminate(&self, _pid: u32) {}
    fn free_bytes(&self, _path: &std::path::Path) -> Option<u64> {
        None
    }
    fn persist_write(&self, _ns: u32, _key: u32, _value: &str) -> anyhow::Result<()> {
        Ok(())
    }
    fn persist_write_byte(&self, _ns: u32, _key: u32, _value: u8) -> anyhow::Result<()> {
        Ok(())
    }
}

fn reaped_pid() -> u32 {
    let mut c = std::process::Command::new("true").spawn().unwrap();
    c.wait().unwrap();
    c.id()
}

#[test]
fn stale_takeover_spares_a_lock_another_taker_got_first() {
    let f = common::fixture();
    let p = f.layout.run("lock");
    let mut other = std::process::Command::new("sleep").arg("30").spawn().unwrap();
    let other_pid = other.id();
    fs::write(&p, reaped_pid().to_string()).unwrap();
    // While we decide the dead holder's lock is stale, another taker removes it and takes it.
    let p2 = p.clone();
    let sys = RacingSys { on_dead: move |_| fs::write(&p2, other_pid.to_string()).unwrap() };
    let e = Lock::acquire(&f.layout, &sys).unwrap_err();
    assert!(format!("{e}").starts_with("busy"), "{e}");
    assert_eq!(fs::read_to_string(&p).unwrap(), other_pid.to_string(), "the other taker's lock survives");
    let _ = other.kill();
    let _ = other.wait();
}

#[test]
fn stale_takeover_still_works_when_nobody_races() {
    let f = common::fixture();
    let p = f.layout.run("lock");
    fs::write(&p, reaped_pid().to_string()).unwrap();
    let sys = RacingSys { on_dead: |_| {} };
    let l = Lock::acquire(&f.layout, &sys).unwrap();
    assert_eq!(fs::read_to_string(&p).unwrap(), std::process::id().to_string());
    drop(l);
    assert!(!p.exists());
}

#[test]
fn dropping_a_lock_spares_one_another_process_took_over() {
    let f = common::fixture();
    let sys = RealSys { mounts: false };
    let p = f.layout.run("lock");
    let l = Lock::acquire(&f.layout, &sys).unwrap();
    fs::write(&p, "4242").unwrap(); // our lock was (wrongly) stolen
    drop(l);
    assert_eq!(fs::read_to_string(&p).unwrap(), "4242");
}

#[test]
fn waiting_for_a_lock_whose_holder_vanishes_takes_it() {
    let f = common::fixture();
    let sys = RealSys { mounts: false };
    let p = f.layout.run("lock");
    let mut other = std::process::Command::new("sleep").arg("30").spawn().unwrap();
    fs::write(&p, other.id().to_string()).unwrap();
    let p2 = p.clone();
    let w = std::thread::spawn(move || {
        std::thread::sleep(Duration::from_millis(200));
        fs::remove_file(&p2).unwrap(); // the holder releases normally
    });
    let _l = Lock::acquire_at(&p, &sys, Duration::from_secs(2)).unwrap();
    assert_eq!(fs::read_to_string(&p).unwrap(), std::process::id().to_string());
    w.join().unwrap();
    let _ = other.kill();
    let _ = other.wait();
}

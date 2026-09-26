mod common;
use mibpkg::ctx::Ctx;
use mibpkg::index::Collection;
use mibpkg::install;
use mibpkg::status::SvcState;
use mibpkg::supervisor::{Supervisor, Timing};
use mibpkg::sys::{RealSys, RecordingSys, Sys};
use std::fs;
use std::time::{Duration, Instant};

fn fast() -> Timing {
    Timing {
        backoff_start: Duration::from_millis(20),
        backoff_max: Duration::from_millis(80),
        fail_window: Duration::from_secs(30),
        fail_count: 3,
        poll: Duration::from_millis(10),
        stop_grace: Duration::from_secs(2),
        config_recheck: Duration::from_millis(30),
        stable_after: Duration::from_secs(60),
    }
}

fn install_pkg(f: &common::Fixture, version: &str, body: &str, extra: &str) {
    let src = common::pkg_src(f.root(), "demo", version, body, extra);
    let c = Collection::load(&common::collection(&f.sd(), "complex256", &[src])).unwrap();
    let sys = RecordingSys::default();
    let ctx = Ctx::new(f.layout.clone(), &sys);
    install::install(&ctx, &[f.sd()], &c, "demo").unwrap();
}

/// Tick until `pred` holds or 5 s pass.
fn until(sup: &mut Supervisor, pred: impl Fn(&Supervisor) -> bool) -> bool {
    let end = Instant::now() + Duration::from_secs(5);
    while Instant::now() < end {
        sup.tick(Instant::now());
        if pred(sup) {
            return true;
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    false
}

fn state(sup: &Supervisor) -> SvcState {
    sup.status().services["demo.demo"].state
}

#[test]
fn runs_from_tmp_and_logs() {
    let f = common::fixture();
    install_pkg(&f, "1.0.0", "echo hello-from-demo; sleep 30", "");
    let sys = RealSys { mounts: false };
    let mut sup = Supervisor::new(&f.layout, &sys, fast());
    sup.reload();
    assert!(until(&mut sup, |s| state(s) == SvcState::Running));
    assert!(f.layout.run("demo.demo").is_file(), "binary copied to /tmp");
    assert!(until(&mut sup, |_| fs::read_to_string(f.layout.run("demo.demo.log")).unwrap_or_default().contains("hello-from-demo")));
    let pid = sup.status().services["demo.demo"].pid.unwrap();
    sup.stop_all();
    assert!(!sys.process_alive(pid));
}

#[test]
fn crash_loop_ends_in_failed_until_restart() {
    let f = common::fixture();
    install_pkg(&f, "1.0.0", "exit 1", "");
    let sys = RealSys { mounts: false };
    let mut sup = Supervisor::new(&f.layout, &sys, fast());
    sup.reload();
    assert!(until(&mut sup, |s| state(s) == SvcState::Failed));
    assert_eq!(sup.status().services["demo.demo"].restarts, 2);
    sup.restart("demo.demo");
    assert_ne!(state(&sup), SvcState::Failed);
}

#[test]
fn restart_never_policy_stops() {
    let f = common::fixture();
    install_pkg(&f, "1.0.0", "exit 0", "restart = \"on-failure\"\n");
    let sys = RealSys { mounts: false };
    let mut sup = Supervisor::new(&f.layout, &sys, fast());
    sup.reload();
    assert!(until(&mut sup, |s| state(s) == SvcState::Stopped));
}

#[test]
fn needs_config_then_starts_when_config_arrives() {
    let f = common::fixture();
    install_pkg(&f, "1.0.0", "sleep 30", "[[config]]\npath = \"c.toml\"\nrequired = true\n");
    let sys = RealSys { mounts: false };
    let mut sup = Supervisor::new(&f.layout, &sys, fast());
    sup.reload();
    assert!(until(&mut sup, |s| state(s) == SvcState::NeedsConfig));
    fs::create_dir_all(f.layout.config_dir("demo")).unwrap();
    fs::write(f.layout.config_dir("demo").join("c.toml"), "x").unwrap();
    assert!(until(&mut sup, |s| state(s) == SvcState::Running), "starts without a reload");
    sup.stop_all();
}

#[test]
fn reload_restarts_on_version_change_and_stops_removed() {
    let f = common::fixture();
    install_pkg(&f, "1.0.0", "sleep 30", "");
    let sys = RealSys { mounts: false };
    let mut sup = Supervisor::new(&f.layout, &sys, fast());
    sup.reload();
    assert!(until(&mut sup, |s| state(s) == SvcState::Running));
    let pid1 = sup.status().services["demo.demo"].pid.unwrap();
    install_pkg(&f, "1.1.0", "sleep 30", ""); // boot.d entry text is identical
    sup.reload();
    assert!(until(&mut sup, |s| s.status().services["demo.demo"].pid.map_or(false, |p| p != pid1)));
    assert!(!sys.process_alive(pid1), "old version stopped");
    let pid2 = sup.status().services["demo.demo"].pid.unwrap();
    let rs = RecordingSys::default();
    let ctx = Ctx::new(f.layout.clone(), &rs);
    install::remove(&ctx, "demo", false).unwrap();
    sup.reload();
    assert!(sup.status().services.is_empty());
    assert!(!sys.process_alive(pid2));
}

#[test]
fn invalid_bootd_is_reported_not_fatal() {
    let f = common::fixture();
    fs::create_dir_all(f.layout.boot_d()).unwrap();
    fs::write(f.layout.boot_d().join("x.y.toml"), "garbage").unwrap();
    let sys = RealSys { mounts: false };
    let mut sup = Supervisor::new(&f.layout, &sys, fast());
    sup.reload();
    assert!(sup.status().invalid.contains_key("x.y.toml"));
}

#[test]
fn run_loop_honours_stop_request_and_disable() {
    let f = common::fixture();
    install_pkg(&f, "1.0.0", "sleep 30", "");
    let layout = f.layout.clone();
    let h = std::thread::spawn(move || {
        let sys = RealSys { mounts: false };
        mibpkg::supervisor::run(&layout, &sys, fast()).unwrap();
    });
    let sys = RealSys { mounts: false };
    let end = Instant::now() + Duration::from_secs(5);
    let mut pid = None;
    while Instant::now() < end && pid.is_none() {
        pid = mibpkg::status::read(&f.layout).and_then(|s| s.services.get("demo.demo").and_then(|x| x.pid));
        std::thread::sleep(Duration::from_millis(20));
    }
    let pid = pid.expect("service started by run loop");
    mibpkg::control::request_stop(&f.layout).unwrap();
    h.join().unwrap();
    assert!(!sys.process_alive(pid));
    assert!(!f.layout.run("pid").exists());

    fs::write(f.sd().join("mibpkg/DISABLE"), "").unwrap();
    mibpkg::supervisor::run(&f.layout, &sys, fast()).unwrap();
    assert!(mibpkg::status::read(&f.layout).unwrap().disabled);
}

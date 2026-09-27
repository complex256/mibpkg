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

#[test]
fn update_revives_a_failed_service() {
    let f = common::fixture();
    install_pkg(&f, "1.0.0", "exit 1", "");
    let sys = RealSys { mounts: false };
    let mut sup = Supervisor::new(&f.layout, &sys, fast());
    sup.reload();
    assert!(until(&mut sup, |s| state(s) == SvcState::Failed));
    install_pkg(&f, "1.1.0", "sleep 30", "");
    sup.reload();
    assert!(until(&mut sup, |s| state(s) == SvcState::Running), "fixed version must run after update");
    sup.stop_all();
}

#[test]
fn stopped_service_stays_stopped_on_unrelated_reload() {
    let f = common::fixture();
    install_pkg(&f, "1.0.0", "exit 0", "restart = \"on-failure\"\n");
    let sys = RealSys { mounts: false };
    let mut sup = Supervisor::new(&f.layout, &sys, fast());
    sup.reload();
    assert!(until(&mut sup, |s| state(s) == SvcState::Stopped));
    sup.reload();
    sup.tick(Instant::now());
    assert_eq!(state(&sup), SvcState::Stopped);
}

#[test]
fn removed_service_leaves_no_tmp_files() {
    let f = common::fixture();
    install_pkg(&f, "1.0.0", "echo hi; sleep 30", "");
    let sys = RealSys { mounts: false };
    let mut sup = Supervisor::new(&f.layout, &sys, fast());
    sup.reload();
    assert!(until(&mut sup, |_| fs::read_to_string(f.layout.run("demo.demo.log")).unwrap_or_default().contains("hi")));
    fs::write(f.layout.run("demo.demo.log.1"), "old").unwrap();
    let rs = RecordingSys::default();
    let ctx = Ctx::new(f.layout.clone(), &rs);
    install::remove(&ctx, "demo", false).unwrap();
    sup.reload();
    for n in ["demo.demo", "demo.demo.log", "demo.demo.log.1"] {
        assert!(!f.layout.run(n).exists(), "{n} left in /tmp");
    }
}

fn marker(f: &common::Fixture) -> std::path::PathBuf {
    f.root().join("ready-flag")
}

#[test]
fn waits_for_a_file_condition_then_starts() {
    let f = common::fixture();
    let flag = marker(&f);
    install_pkg(&f, "1.0.0", "sleep 30", &format!("after = [\"file:{}\"]\n", flag.display()));
    let sys = RealSys { mounts: false };
    let mut sup = Supervisor::new(&f.layout, &sys, fast());
    sup.reload();
    sup.tick(Instant::now());
    let st = sup.status().services["demo.demo"].clone();
    assert_eq!(st.state, SvcState::Waiting);
    assert_eq!(st.waiting_for, vec![format!("file:{}", flag.display())]);
    fs::write(&flag, "").unwrap();
    assert!(until(&mut sup, |s| state(s) == SvcState::Running));
    assert!(sup.status().services["demo.demo"].waiting_for.is_empty());
    sup.stop_all();
}

#[test]
fn timeout_starts_anyway_with_a_note() {
    let f = common::fixture();
    install_pkg(&f, "1.0.0", "sleep 30", "after = [\"file:/nonexistent/flag\"]\nafter_timeout = \"1s\"\n");
    let sys = RealSys { mounts: false };
    let mut sup = Supervisor::new(&f.layout, &sys, fast());
    sup.reload();
    sup.tick(Instant::now());
    assert_eq!(state(&sup), SvcState::Waiting);
    assert!(until(&mut sup, |s| state(s) == SvcState::Running));
    let note = sup.status().services["demo.demo"].note.clone().unwrap();
    assert!(note.contains("started without file:/nonexistent/flag after 1s"), "{note}");
    sup.stop_all();
}

#[test]
fn crash_restart_waits_again_and_waiting_is_not_a_crash() {
    let f = common::fixture();
    let flag = marker(&f);
    fs::write(&flag, "").unwrap();
    // exits after 0.2 s; flag removed before the restart
    install_pkg(&f, "1.0.0", "sleep 0.2; exit 1", &format!("after = [\"file:{}\"]\n", flag.display()));
    let sys = RealSys { mounts: false };
    let mut sup = Supervisor::new(&f.layout, &sys, fast());
    sup.reload();
    assert!(until(&mut sup, |s| state(s) == SvcState::Running));
    fs::remove_file(&flag).unwrap();
    assert!(until(&mut sup, |s| state(s) == SvcState::Waiting), "restart after a crash waits for the condition");
    let t0 = Instant::now();
    while t0.elapsed() < Duration::from_millis(500) {
        sup.tick(Instant::now());
        std::thread::sleep(Duration::from_millis(10));
    }
    assert_eq!(state(&sup), SvcState::Waiting, "waiting never turns into failed");
    assert_eq!(sup.status().services["demo.demo"].restarts, 0);
}

#[test]
fn timeout_then_missing_config_shows_the_error_and_starts_once_config_arrives() {
    let f = common::fixture();
    install_pkg(&f, "1.0.0", "sleep 30", "after = [\"file:/nonexistent/flag\"]\nafter_timeout = \"1s\"\n\n[[config]]\npath = \"c.toml\"\nrequired = true\n");
    let sys = RealSys { mounts: false };
    let mut sup = Supervisor::new(&f.layout, &sys, fast());
    sup.reload();
    assert!(until(&mut sup, |s| state(s) == SvcState::NeedsConfig));
    let line = mibpkg::status::describe(&sup.status().services["demo.demo"]);
    assert!(line.contains("missing"), "the missing-config error must stay visible: {line}");
    fs::create_dir_all(f.layout.config_dir("demo")).unwrap();
    fs::write(f.layout.config_dir("demo").join("c.toml"), "x").unwrap();
    let t0 = Instant::now();
    assert!(until(&mut sup, |s| state(s) == SvcState::Running));
    assert!(t0.elapsed() < Duration::from_millis(900), "no second full timeout: {:?}", t0.elapsed());
    let note = sup.status().services["demo.demo"].note.clone().unwrap_or_default();
    assert!(note.contains("started without"), "note is set once it really starts: {note}");
    sup.stop_all();
}

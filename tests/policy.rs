mod common;
use mibpkg::status::{self, Status, SvcState, SvcStatus};
use mibpkg::supervisor::{next_backoff, CappedLog, CrashWindow, Timing};
use std::fs;
use std::time::{Duration, Instant};

#[test]
fn backoff_doubles_to_cap() {
    let t = Timing::default();
    let mut b = None;
    let mut seen = vec![];
    for _ in 0..12 {
        let n = next_backoff(&t, b);
        seen.push(n.as_secs());
        b = Some(n);
    }
    assert_eq!(&seen[..5], &[1, 2, 4, 8, 16]);
    assert_eq!(*seen.last().unwrap(), 300);
}

#[test]
fn crash_window_gives_up_after_n_in_window() {
    let t = Timing::default();
    let mut w = CrashWindow::default();
    let t0 = Instant::now();
    for i in 0..4 {
        assert!(!w.record(t0 + Duration::from_secs(i * 60), &t));
    }
    assert!(w.record(t0 + Duration::from_secs(240), &t), "5th exit within 10 min");
    let mut w = CrashWindow::default();
    for i in 0..10 {
        assert!(!w.record(t0 + Duration::from_secs(i * 200), &t), "spread out exits never give up");
    }
}

#[test]
fn capped_log_rotates_once() {
    let t = tempfile::tempdir().unwrap();
    let p = t.path().join("svc.log");
    let mut l = CappedLog::open(p.clone(), 10);
    l.write(b"12345");
    l.write(b"6789");
    l.write(b"abcdef");
    assert_eq!(fs::read(&p).unwrap(), b"abcdef");
    assert_eq!(fs::read(t.path().join("svc.log.1")).unwrap(), b"123456789");
}

#[test]
fn status_roundtrip() {
    let f = common::fixture();
    assert!(status::read(&f.layout).is_none());
    let mut s = Status { updated: 1, pid: 2, ..Default::default() };
    s.services.insert(
        "demo.demo".into(),
        SvcStatus { state: SvcState::NeedsConfig, pid: None, restarts: 0, last_exit: None, error: None, waiting_for: vec![], waiting_secs: None, note: None },
    );
    status::write(&f.layout, &s).unwrap();
    let r = status::read(&f.layout).unwrap();
    assert_eq!(r.services["demo.demo"].state, SvcState::NeedsConfig);
    assert_eq!(SvcState::NeedsConfig.to_string(), "needs-config");
}

#[test]
fn background_supervisor_survives_hangup() {
    let t = tempfile::tempdir().unwrap();
    let exe = t.path().join("fake");
    let out = t.path().join("alive");
    fs::write(&exe, format!("#!/bin/sh\nkill -HUP $$\necho yes > {}\n", out.display())).unwrap();
    fs::set_permissions(&exe, std::os::unix::fs::PermissionsExt::from_mode(0o755)).unwrap();
    let mut c = mibpkg::control::supervisor_command(&exe, t.path());
    c.status().unwrap();
    assert_eq!(fs::read_to_string(&out).unwrap_or_default().trim(), "yes", "SIGHUP must be ignored");
}

#[test]
fn describe_renders_each_state() {
    let base = SvcStatus { state: SvcState::Running, pid: Some(7), restarts: 2, last_exit: None, error: None, waiting_for: vec![], waiting_secs: None, note: None };
    assert_eq!(status::describe(&base), "running pid 7 restarts 2");
    let w = SvcStatus { state: SvcState::Waiting, pid: None, restarts: 0, waiting_for: vec!["tailnet".into(), "hmi-ready".into()], waiting_secs: Some(130), ..base.clone() };
    assert_eq!(status::describe(&w), "waiting: tailnet, hmi-ready (2m10s)");
    let n = SvcStatus { restarts: 0, note: Some("started without tailnet after 5m".into()), ..base.clone() };
    assert_eq!(status::describe(&n), "running pid 7 (started without tailnet after 5m)");
    let e = SvcStatus { state: SvcState::NeedsConfig, pid: None, restarts: 0, error: Some("missing /x".into()), ..base };
    assert_eq!(status::describe(&e), "needs-config (missing /x)");
}

#[test]
fn describe_shows_note_and_error_and_hides_note_while_waiting() {
    let base = SvcStatus { state: SvcState::Backoff, pid: None, restarts: 1, last_exit: None, error: Some("spawn failed".into()), waiting_for: vec![], waiting_secs: None, note: Some("started without tailnet after 5m".into()) };
    assert_eq!(status::describe(&base), "backoff restarts 1 (started without tailnet after 5m) (spawn failed)");
    let w = SvcStatus { state: SvcState::Waiting, waiting_for: vec!["tailnet".into()], waiting_secs: Some(30), error: None, ..base };
    assert_eq!(status::describe(&w), "waiting: tailnet (30s) restarts 1");
}

#[test]
fn boot_markers_are_logged_once_with_their_time() {
    use mibpkg::supervisor::MarkerWatch;
    let f = common::fixture();
    let a = f.layout.p("/tmp/smFlagStartupDone");
    let b = f.layout.p("/tmp/ppp_connected");
    fs::write(&a, "").unwrap();
    let mut w = MarkerWatch::new(&f.layout, &["/tmp/smFlagStartupDone", "/tmp/ppp_connected"]);
    assert_eq!(w.check(&f.layout, 19), vec!["boot marker /tmp/smFlagStartupDone: already present at 19 s"]);
    assert!(w.check(&f.layout, 20).is_empty(), "nothing new");
    fs::write(&b, "").unwrap();
    assert_eq!(w.check(&f.layout, 57), vec!["boot marker /tmp/ppp_connected: appeared at 57 s"]);
    assert!(w.check(&f.layout, 58).is_empty(), "each marker logged once");
    assert!(w.done());
}

#[test]
fn online_checks_wait_for_internet_run_once_and_repeat() {
    use mibpkg::supervisor::OnlineSchedule;
    let mut s = OnlineSchedule::new(Duration::from_millis(200));
    let mut spawn = || std::process::Command::new("true").spawn();
    let t0 = Instant::now();
    s.tick(t0, false, &mut spawn);
    assert_eq!(s.spawns(), 0, "no internet, no check");
    s.tick(t0, true, &mut spawn);
    assert_eq!(s.spawns(), 1);
    std::thread::sleep(Duration::from_millis(50));
    s.tick(Instant::now(), true, &mut spawn);
    assert_eq!(s.spawns(), 1, "not again before the interval");
    std::thread::sleep(Duration::from_millis(250));
    s.tick(Instant::now(), true, &mut spawn);
    assert_eq!(s.spawns(), 2, "again after the interval");
}

#[test]
fn a_running_check_is_never_doubled() {
    use mibpkg::supervisor::OnlineSchedule;
    let mut s = OnlineSchedule::new(Duration::from_millis(1));
    let mut spawn = || std::process::Command::new("sleep").arg("2").spawn();
    s.tick(Instant::now(), true, &mut spawn);
    std::thread::sleep(Duration::from_millis(20));
    s.tick(Instant::now(), true, &mut spawn);
    assert_eq!(s.spawns(), 1, "previous check still running");
}

#[test]
fn a_failed_check_is_retried_soon_and_due_reports_when() {
    use mibpkg::supervisor::OnlineSchedule;
    let mut s = OnlineSchedule::new(Duration::from_secs(3600));
    let t0 = Instant::now();
    assert!(s.due(t0));
    let mut fail = || std::process::Command::new("false").spawn();
    s.tick(t0, true, &mut fail);
    assert!(!s.due(t0), "a check is running");
    std::thread::sleep(Duration::from_millis(200)); // let `false` exit
    s.tick(t0 + Duration::from_secs(1), true, &mut fail); // reaps the failure
    assert!(!s.due(t0 + Duration::from_secs(60)), "short backoff, not immediately");
    assert!(s.due(t0 + Duration::from_secs(3 * 60)), "retried within minutes, not 6 h");
}

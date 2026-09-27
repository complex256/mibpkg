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

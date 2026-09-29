mod common;
use mibpkg::persist::{persist_text, result_key, service_key, Publisher, KEY_RESULT_GLOBAL, NS};
use mibpkg::status::{SvcState, SvcStatus};
use mibpkg::sys::RecordingSys;

fn st(state: SvcState, pid: Option<u32>, waiting: &[&str]) -> SvcStatus {
    SvcStatus {
        state,
        pid,
        restarts: 0,
        last_exit: None,
        error: None,
        waiting_for: waiting.iter().map(|s| s.to_string()).collect(),
        waiting_secs: Some(42),
        note: None,
    }
}

#[test]
fn keys_are_stable_and_separate() {
    assert_eq!(service_key("carmqtt.carmqtt"), service_key("carmqtt.carmqtt"));
    assert_ne!(service_key("carmqtt.carmqtt"), service_key("carmqtt.other"));
    assert_ne!(service_key("carmqtt"), result_key("carmqtt"));
    assert_eq!(result_key("_"), KEY_RESULT_GLOBAL);
    assert!(service_key("x") > 100 && result_key("x") > 100, "clear of the fixed keys");
}

#[test]
fn waiting_text_has_no_ticking_duration() {
    assert_eq!(persist_text(&st(SvcState::Waiting, None, &["tailnet"])), "waiting: tailnet");
    assert_eq!(persist_text(&st(SvcState::Running, Some(7), &[])), "running pid 7");
}

#[test]
fn publisher_writes_only_changes() {
    let sys = RecordingSys::default();
    let mut p = Publisher::default();
    p.set(&sys, 1, "running since 19 s");
    p.set(&sys, 1, "running since 19 s");
    p.set(&sys, 5, "waiting: tailnet");
    p.set(&sys, 5, "running pid 9");
    assert_eq!(
        *sys.persisted.borrow(),
        vec![(NS, 1, "running since 19 s".to_string()), (NS, 5, "waiting: tailnet".to_string()), (NS, 5, "running pid 9".to_string())]
    );
}

#[test]
fn a_failing_persistence_service_is_not_retried_every_tick() {
    let sys = RecordingSys::default();
    sys.fail_persist.set(true);
    let mut p = Publisher::default();
    p.set(&sys, 1, "running");
    p.set(&sys, 5, "waiting: tailnet");
    p.set(&sys, 1, "running");
    assert_eq!(sys.persisted.borrow().len(), 1, "one failed attempt, then back off instead of blocking every tick");
}

mod common;
use mibpkg::persist::{persist_text, result_key, result_value, service_key, short, Publisher, KEY_RESULT_GLOBAL, MAX_VALUE, NS};
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
    // GEM shows the state only: no pid, so a restart that ends up running again writes nothing.
    assert_eq!(persist_text(&st(SvcState::Running, Some(7), &[])), "running");
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

// GEM's value column does not wrap: every live value fits in MAX_VALUE characters (2026-10-07).
#[test]
fn values_are_short_enough_for_gem() {
    assert_eq!(short("running"), "running");
    let long = short("FAILED: carmqtt 0.2.1 is not compatible with this unit");
    assert_eq!(long.chars().count(), MAX_VALUE);
    assert!(long.ends_with('…'), "{long}");
    let mut s = st(SvcState::NeedsConfig, None, &[]);
    s.error = Some("missing /mnt/app/root/mibpkg/state/config/carmqtt/broker.toml".into());
    s.restarts = 3;
    assert_eq!(persist_text(&s), "needs config");
    assert!(persist_text(&st(SvcState::Waiting, None, &["tailnet", "hmi-ready", "internet"])).chars().count() <= MAX_VALUE);
}

#[test]
fn results_show_the_outcome_not_the_whole_line() {
    assert_eq!(result_value("OK: updated mibpkg 0.4.4 -> 0.5.0; mibpkg restarts itself (2026-10-07 15:32 UTC)"), "OK");
    assert_eq!(result_value("BUSY: another mibpkg action is running (pid 123)"), "busy");
    let failed = result_value("FAILED: no newer mibpkg on the card or online");
    assert!(failed.starts_with("FAILED: no newer") && failed.chars().count() <= MAX_VALUE, "{failed}");
}

#[test]
fn a_value_already_written_is_not_written_again_by_another_process() {
    // The supervisor and each `gem` action are separate processes: an in-memory "last value" does
    // not stop repeats, so the last written value of each row is kept in /tmp (RAM).
    let f = common::fixture();
    let sys = RecordingSys::default();
    Publisher::with_memo(&f.layout).set(&sys, result_key("_"), "OK");
    Publisher::with_memo(&f.layout).set(&sys, result_key("_"), "OK");
    Publisher::with_memo(&f.layout).set(&sys, result_key("_"), "FAILED: busy");
    assert_eq!(sys.persisted.borrow().len(), 2, "{:?}", sys.persisted.borrow());
    // A write that failed is not remembered, so it is tried again.
    let g = common::fixture();
    let failing = RecordingSys::default();
    failing.fail_persist.set(true);
    Publisher::with_memo(&g.layout).set(&failing, 1, "running");
    failing.fail_persist.set(false);
    Publisher::with_memo(&g.layout).set(&failing, 1, "running");
    assert_eq!(failing.persisted.borrow().len(), 2);
}

#[test]
fn an_action_result_is_published_as_its_short_outcome_once() {
    let f = common::fixture();
    let sys = RecordingSys::default();
    mibpkg::persist::publish_result(&sys, &f.layout, "_", "OK: rescanned; no new config files");
    mibpkg::persist::publish_result(&sys, &f.layout, "_", "OK: web server restart requested (2026-10-07 15:32 UTC)");
    assert_eq!(*sys.persisted.borrow(), vec![(NS, KEY_RESULT_GLOBAL, "OK".to_string())], "same outcome, no second write");
}

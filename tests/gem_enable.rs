use mibpkg::gem_enable::{enable, disable, GEM_DEV_KEY, COMMIT_KEY};
use mibpkg::sys::RecordingSys;

#[test]
fn enable_writes_dev_flag_then_commit() {
    let sys = RecordingSys::default();
    enable(&sys).unwrap();
    let w = sys.persisted.borrow().clone();
    assert_eq!(w, vec![(0, GEM_DEV_KEY, "1".into()), (0, COMMIT_KEY, "0".into())]);
}

#[test]
fn disable_writes_zero_then_commit() {
    let sys = RecordingSys::default();
    disable(&sys).unwrap();
    let w = sys.persisted.borrow().clone();
    assert_eq!(w, vec![(0, GEM_DEV_KEY, "0".into()), (0, COMMIT_KEY, "0".into())]);
}

#[test]
fn pc_failure_surfaces_as_err_so_caller_can_log_and_continue() {
    let sys = RecordingSys::default();
    sys.fail_persist.set(true);
    assert!(enable(&sys).is_err());
}

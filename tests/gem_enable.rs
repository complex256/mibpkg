use mibpkg::gem_enable::{enable, disable, GEM_DEV_KEY, COMMIT_KEY};
use mibpkg::sys::RecordingSys;

// The GEM dev-mode flag (ns0 key 3221356557) is a BYTE: it must be written with pc's byte
// type, not as a string (a string write hangs pc on a type mismatch — found on-car 2026-10-16).

#[test]
fn enable_writes_dev_byte_then_commit() {
    let sys = RecordingSys::default();
    enable(&sys).unwrap();
    let w = sys.persisted_bytes.borrow().clone();
    assert_eq!(w, vec![(0, GEM_DEV_KEY, 1u8), (0, COMMIT_KEY, 0u8)]);
    // and never as a string
    assert!(sys.persisted.borrow().is_empty(), "GEM flag must not be written as a string");
}

#[test]
fn disable_writes_zero_byte_then_commit() {
    let sys = RecordingSys::default();
    disable(&sys).unwrap();
    let w = sys.persisted_bytes.borrow().clone();
    assert_eq!(w, vec![(0, GEM_DEV_KEY, 0u8), (0, COMMIT_KEY, 0u8)]);
}

#[test]
fn pc_failure_surfaces_as_err_so_caller_can_log_and_continue() {
    let sys = RecordingSys::default();
    sys.fail_persist.set(true);
    assert!(enable(&sys).is_err());
}

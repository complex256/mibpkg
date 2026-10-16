use std::io::Write;

#[test]
fn install_sh_is_valid_shell() {
    let src = mibpkg::swdl::INSTALL_SH;
    let d = tempfile::tempdir().unwrap();
    let p = d.path().join("install.sh");
    std::fs::File::create(&p).unwrap().write_all(src.as_bytes()).unwrap();
    if std::process::Command::new("ksh").arg("-n").arg(&p).status().map(|s| s.success()).unwrap_or(false) {
        return; // ksh accepted it
    }
    let ok = std::process::Command::new("sh").arg("-n").arg(&p).status().unwrap().success();
    assert!(ok, "install.sh failed sh -n");
}

#[test]
fn result_is_touched_only_after_bootstrap() {
    let src = mibpkg::swdl::INSTALL_SH;
    let clear = src.find("rm -f /tmp/SWDLScript.Result").expect("stale marker cleared");
    let boot = src.find("bootstrap").expect("bootstrap call present");
    let last_touch = src.rfind("touch /tmp/SWDLScript.Result").expect("Result touch present");
    assert!(clear < boot, "stale marker must be cleared before bootstrap");
    assert!(boot < last_touch, "success marker must be touched only after bootstrap");
    assert!(src.contains("enable-gem"));
    assert!(src.contains("gem-patch apply"));
    assert!(src.contains("_Swdlautorun.txt"));
}

#[test]
fn forward_captures_exit_and_clears_stale_marker() {
    let src = mibpkg::swdl::INSTALL_SH;
    // The MMX forward must not `exec` (that replaces the RCC shell, so the RCC-side
    // SWDLScript.Result marker would never be touched and SWDL would report failure).
    assert!(!src.contains("exec on -f mmx"), "forward uses exec; RCC marker would never be touched");
    // A stale success marker from an earlier run in the same (RAM) boot must be cleared first.
    assert!(src.contains("rm -f /tmp/SWDLScript.Result"), "stale Result marker not cleared at start");
}

#[test]
fn install_sh_suppresses_the_most_popup() {
    let src = mibpkg::swdl::INSTALL_SH;
    assert!(src.contains("skipMostPopup.txt"), "install.sh does not suppress the MOST popup");
    // placed on the MMX side where efs-persist is reachable as /net/rcc/mnt/efs-persist.
    assert!(src.contains("/net/rcc/mnt/efs-persist"));
}

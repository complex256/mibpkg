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
    let b = src.find("bootstrap").expect("bootstrap call present");
    let r = src.find("SWDLScript.Result").expect("Result touch present");
    assert!(b < r, "Result is touched before bootstrap");
    assert!(src.contains("enable-gem"));
    assert!(src.contains("gem-patch apply"));
    assert!(src.contains("_Swdlautorun.txt"));
}

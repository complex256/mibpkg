mod common;
use mibpkg::ctx::Ctx;
use mibpkg::hook::{add_hook, install_hook, remove_lines, uninstall_hook, MARKER};
use mibpkg::sys::RecordingSys;
use std::fs;
use std::os::unix::fs::PermissionsExt;

#[test]
fn add_hook_places_line_after_inetd_and_is_idempotent() {
    let out = add_hook(common::STARTUP).unwrap();
    let lines: Vec<&str> = out.lines().collect();
    let i = lines.iter().position(|l| l.contains("inetd")).unwrap();
    assert!(lines[i + 1].trim_start().starts_with("## mibpkg"));
    assert!(lines[i + 2].contains(MARKER));
    assert_eq!(add_hook(&out).unwrap(), out);
    assert!(add_hook("echo no inetd here\n").is_err());
}

#[test]
fn remove_lines_takes_the_comment_too() {
    let with = add_hook(common::STARTUP).unwrap();
    let (back, changed) = remove_lines(&with, MARKER);
    assert!(changed);
    assert_eq!(back, common::STARTUP);
    let (same, changed) = remove_lines(common::STARTUP, MARKER);
    assert!(!changed);
    assert_eq!(same, common::STARTUP);
}

#[test]
fn install_and_uninstall_on_disk() {
    let f = common::fixture();
    let sys = RecordingSys::default();
    let ctx = Ctx::new(f.layout.clone(), &sys);
    assert!(install_hook(&ctx).unwrap());
    let p = f.layout.startup_sh();
    assert!(fs::read_to_string(&p).unwrap().contains(MARKER));
    assert_eq!(fs::metadata(&p).unwrap().permissions().mode() & 0o777, 0o755, "mode kept");
    assert_eq!(fs::read_to_string(p.with_file_name("startup.sh.pre-mibpkg")).unwrap(), common::STARTUP);
    assert!(!p.with_file_name("startup.sh.mibpkg-new").exists());
    assert!(!install_hook(&ctx).unwrap(), "second install is a no-op");
    assert_eq!(*sys.calls.borrow(), vec!["rw /mnt/system", "rw /mnt/app", "ro /mnt/app", "ro /mnt/system"]);
    assert!(uninstall_hook(&ctx).unwrap());
    assert_eq!(fs::read_to_string(&p).unwrap(), common::STARTUP);
}

#[test]
fn syntax_failure_leaves_startup_untouched() {
    let f = common::fixture();
    let sys = RecordingSys::default();
    sys.syntax_bad.set(true);
    let ctx = Ctx::new(f.layout.clone(), &sys);
    let e = install_hook(&ctx).unwrap_err();
    assert!(format!("{e:#}").contains("ksh -n"));
    let p = f.layout.startup_sh();
    assert_eq!(fs::read_to_string(&p).unwrap(), common::STARTUP);
    assert!(!p.with_file_name("startup.sh.mibpkg-new").exists());
}

#[test]
fn hook_starts_launch_without_a_fixed_sleep() {
    // Start conditions replace the old `sleep 30`; the supervisor starts as soon as startup.sh gets here.
    assert!(!mibpkg::hook::HOOK[1].contains("sleep"), "{}", mibpkg::hook::HOOK[1]);
    assert!(mibpkg::hook::HOOK[1].contains(MARKER));
}

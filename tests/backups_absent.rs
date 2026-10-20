use mibpkg::backups::{backup_once, backup_path, forget, has_backup, is_tombstone, restore, restore_all};
use mibpkg::paths::Layout;
use std::fs;
use std::os::unix::fs::PermissionsExt;

const F: &str = "/mnt/app/eso/hmi/lsd/probe.txt";

fn layout() -> (tempfile::TempDir, Layout) {
    let t = tempfile::tempdir().unwrap();
    let l = Layout::load(t.path()).unwrap();
    fs::create_dir_all(l.p(F).parent().unwrap()).unwrap();
    (t, l)
}

#[test]
fn missing_file_backs_up_as_tombstone_and_restore_deletes_it() {
    let (_t, l) = layout();
    backup_once(&l, F).unwrap();
    assert!(is_tombstone(&l, F));
    assert!(has_backup(&l, F));
    assert!(!backup_path(&l, F).exists());
    fs::write(l.p(F), b"created by install").unwrap();
    restore(&l, F).unwrap();
    assert!(!l.p(F).exists());
    restore(&l, F).unwrap(); // already gone is success
}

#[test]
fn existing_backup_is_never_overwritten() {
    let (_t, l) = layout();
    fs::write(l.p(F), b"first").unwrap();
    backup_once(&l, F).unwrap();
    fs::write(l.p(F), b"second").unwrap();
    backup_once(&l, F).unwrap();
    assert!(!is_tombstone(&l, F));
    restore(&l, F).unwrap();
    assert_eq!(fs::read(l.p(F)).unwrap(), b"first");
}

#[test]
fn forget_removes_both_forms() {
    let (_t, l) = layout();
    backup_once(&l, F).unwrap();
    forget(&l, F).unwrap();
    assert!(!has_backup(&l, F));
    fs::write(l.p(F), b"x").unwrap();
    backup_once(&l, F).unwrap();
    assert!(has_backup(&l, F));
    forget(&l, F).unwrap();
    assert!(!has_backup(&l, F));
    forget(&l, F).unwrap(); // nothing to forget is fine
}

#[test]
fn restore_all_handles_tombstones() {
    let (_t, l) = layout();
    let kept = "/mnt/app/eso/hmi/lsd/kept.txt";
    fs::write(l.p(kept), b"orig").unwrap();
    backup_once(&l, kept).unwrap();
    backup_once(&l, F).unwrap();
    fs::write(l.p(kept), b"patched").unwrap();
    fs::write(l.p(F), b"new").unwrap();
    restore_all(&l).unwrap();
    assert_eq!(fs::read(l.p(kept)).unwrap(), b"orig");
    assert!(!l.p(F).exists());
}

#[test]
fn mode_is_preserved() {
    let (_t, l) = layout();
    fs::write(l.p(F), b"x").unwrap();
    fs::set_permissions(l.p(F), fs::Permissions::from_mode(0o777)).unwrap();
    backup_once(&l, F).unwrap();
    fs::write(l.p(F), b"y").unwrap();
    fs::set_permissions(l.p(F), fs::Permissions::from_mode(0o600)).unwrap();
    restore(&l, F).unwrap();
    assert_eq!(fs::metadata(l.p(F)).unwrap().permissions().mode() & 0o7777, 0o777);
}

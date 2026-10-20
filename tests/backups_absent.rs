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
    assert!(!l.p(F).exists());
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

#[test]
fn restore_all_skips_and_cleans_leftover_tmp_files() {
    let (_t, l) = layout();
    backup_once(&l, F).unwrap();
    let dir = backup_path(&l, F).parent().unwrap().to_path_buf();
    let a = dir.join("mnt%app%eso%hmi%lsd%x.tmp");
    let b = dir.join("mnt%app%eso%hmi%lsd%y.absent.tmp");
    fs::write(&a, b"half").unwrap();
    fs::write(&b, b"").unwrap();
    fs::write(l.p(F), b"new").unwrap();
    restore_all(&l).unwrap();
    assert!(!l.p(F).exists());
    assert!(!a.exists() && !b.exists());
}

#[test]
fn restore_all_is_best_effort_and_reports_errors() {
    let (_t, l) = layout();
    let ok = "/mnt/app/eso/hmi/lsd/ok.txt";
    fs::write(l.p(ok), b"orig").unwrap();
    backup_once(&l, ok).unwrap();
    // A backup whose target directory is gone cannot be restored.
    let bad = "/mnt/app/eso/hmi/gone/bad.txt";
    fs::create_dir_all(l.p(bad).parent().unwrap()).unwrap();
    fs::write(l.p(bad), b"o").unwrap();
    backup_once(&l, bad).unwrap();
    let d = l.p(bad).parent().unwrap().to_path_buf();
    fs::remove_dir_all(&d).unwrap();
    fs::write(&d, b"a file, not a dir").unwrap();
    fs::write(l.p(ok), b"patched").unwrap();
    assert!(restore_all(&l).is_err());
    assert_eq!(fs::read(l.p(ok)).unwrap(), b"orig", "other entries still restored");
}

#[test]
fn restore_clears_the_record_so_a_stale_tombstone_cannot_delete_later_files() {
    let (_t, l) = layout();
    backup_once(&l, F).unwrap();
    fs::write(l.p(F), b"ours").unwrap();
    restore(&l, F).unwrap();
    assert!(!has_backup(&l, F));
    restore_after_noop(&l);
    // someone else creates the file; a new backup records the real file
    fs::write(l.p(F), b"theirs").unwrap();
    backup_once(&l, F).unwrap();
    assert!(!is_tombstone(&l, F));
    fs::write(l.p(F), b"changed").unwrap();
    restore(&l, F).unwrap();
    assert_eq!(fs::read(l.p(F)).unwrap(), b"theirs");
}

fn restore_after_noop(l: &Layout) {
    // with no record, restore is an error (nothing to restore) and touches nothing
    fs::write(l.p(F), b"keep").unwrap();
    assert!(restore(l, F).is_err());
    assert_eq!(fs::read(l.p(F)).unwrap(), b"keep");
}

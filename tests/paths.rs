use mibpkg::fsutil;
use mibpkg::paths::Layout;
use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::Path;

#[test]
fn layout_maps_unit_paths_under_root() {
    let t = tempfile::tempdir().unwrap();
    let l = Layout::load(t.path()).unwrap();
    assert_eq!(l.home(), t.path().join("mnt/app/root/mibpkg"));
    assert_eq!(l.boot_d(), t.path().join("mnt/app/root/mibpkg/boot.d"));
    assert_eq!(l.pkg_dir("carmqtt"), t.path().join("mnt/app/root/mibpkg/pkgs/carmqtt"));
    assert_eq!(l.config_dir("carmqtt"), t.path().join("mnt/app/root/mibpkg/state/config/carmqtt"));
    assert_eq!(l.run("status"), t.path().join("tmp/mibpkg.status"));
    assert_eq!(l.tmp_bin(), t.path().join("tmp/mibpkg"));
    assert_eq!(l.startup_sh(), t.path().join("mnt/system/etc/boot/startup.sh"));
    assert_eq!(l.engdefs(), t.path().join("mnt/app/eso/hmi/engdefs"));
    assert_eq!(l.unit_path(&l.gem_dir()), "/mnt/app/root/mibpkg/gem");
}

#[test]
fn layout_on_real_root_is_identity() {
    let l = Layout::load("/nonexistent-root-for-test").unwrap();
    let real = Layout { root: "/".into(), settings: l.settings.clone() };
    assert_eq!(real.p("/mnt/app"), Path::new("/mnt/app"));
    assert_eq!(real.unit_path(Path::new("/mnt/app/x")), "/mnt/app/x");
}

#[test]
fn settings_file_overrides_defaults() {
    let t = tempfile::tempdir().unwrap();
    let home = t.path().join("mnt/app/root/mibpkg");
    fs::create_dir_all(&home).unwrap();
    fs::write(home.join("settings.toml"), "unit = \"MHI2\"\ngem_parent = \"Other\"\n").unwrap();
    let l = Layout::load(t.path()).unwrap();
    assert_eq!(l.settings.unit.as_deref(), Some("MHI2"));
    assert_eq!(l.settings.gem_parent, "Other");
    assert!(l.settings.media_roots.contains(&"/fs/sda0".to_string()));
}

#[test]
fn settings_rejects_unknown_keys() {
    let t = tempfile::tempdir().unwrap();
    let home = t.path().join("mnt/app/root/mibpkg");
    fs::create_dir_all(&home).unwrap();
    fs::write(home.join("settings.toml"), "typo = 1\n").unwrap();
    assert!(Layout::load(t.path()).is_err());
}

#[test]
fn atomic_write_and_copy_atomic() {
    let t = tempfile::tempdir().unwrap();
    let f = t.path().join("a/b/c.txt");
    fsutil::atomic_write(&f, b"one").unwrap();
    fsutil::atomic_write(&f, b"two").unwrap();
    assert_eq!(fs::read(&f).unwrap(), b"two");
    assert!(!fsutil::tmp_sibling(&f).exists());
    let d = t.path().join("x/y.bin");
    fsutil::copy_atomic(&f, &d, 0o755).unwrap();
    assert_eq!(fs::read(&d).unwrap(), b"two");
    assert_eq!(fs::metadata(&d).unwrap().permissions().mode() & 0o777, 0o755);
}

#[test]
fn sync_unsupported_is_not_an_error() {
    // QNX 6.5 /tmp is /dev/shmem: fsync there fails with ENOSYS (Unsupported).
    use std::io::{Error, ErrorKind};
    assert!(fsutil::sync_result(Err(Error::from(ErrorKind::Unsupported))).is_ok());
    assert!(fsutil::sync_result(Err(Error::from(ErrorKind::PermissionDenied))).is_err());
    assert!(fsutil::sync_result(Ok(())).is_ok());
}

#[test]
fn rename_fallback_copies_and_removes_source() {
    // QNX 6.5 /tmp (/dev/shmem) cannot rename: EXDEV. The fallback copies instead.
    let t = tempfile::tempdir().unwrap();
    let a = t.path().join("a.tmp");
    let b = t.path().join("b");
    fs::write(&a, "new").unwrap();
    fs::set_permissions(&a, fs::Permissions::from_mode(0o755)).unwrap();
    fs::write(&b, "old").unwrap();
    fsutil::replace_by_copy(&a, &b).unwrap();
    assert_eq!(fs::read_to_string(&b).unwrap(), "new");
    assert!(!a.exists());
    assert_eq!(fs::metadata(&b).unwrap().permissions().mode() & 0o777, 0o755);
    assert!(fsutil::is_cross_device(&std::io::Error::from(std::io::ErrorKind::CrossesDevices)));
    assert!(!fsutil::is_cross_device(&std::io::Error::from(std::io::ErrorKind::NotFound)));
}

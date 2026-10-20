mod common;
use mibpkg::ctx::Ctx;
use mibpkg::mibsdk::stamp::{self, StampKind};
use mibpkg::mibsdk::{self, FileOps};
use mibpkg::sys::RecordingSys;
use mibpkg::unit::hmi_version;
use std::cell::{Cell, RefCell};
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

const FIXTURES: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/mibsdk");
const SDK_JARS: [&str; 3] = ["mibsdk-runtime.jar", "mibsdk-adapter-hqc.jar", "mibsdk-rt-retrotranslator.jar"];

/// A copy of the stamp fixtures (jars + stamps in CheckStamp's format 1) that a test may edit.
fn stamps() -> tempfile::TempDir {
    let d = tempfile::tempdir().unwrap();
    for e in fs::read_dir(FIXTURES).unwrap() {
        let p = e.unwrap().path();
        fs::copy(&p, d.path().join(p.file_name().unwrap())).unwrap();
    }
    d
}

fn verify_hello(d: &Path) -> anyhow::Result<stamp::Verified> {
    let jar = d.join("hello.jar");
    stamp::verify(&d.join("hello.jar.stamp"), StampKind::Plugin, Some("hello"), &[&jar])
}

fn verify_unit(d: &Path) -> anyhow::Result<stamp::Verified> {
    let jars: Vec<PathBuf> = SDK_JARS.iter().map(|j| d.join(j)).collect();
    let refs: Vec<&Path> = jars.iter().map(|p| p.as_path()).collect();
    stamp::verify(&d.join("mibsdk-unit.stamp"), StampKind::Unit, None, &refs)
}

fn reason(r: anyhow::Result<stamp::Verified>) -> String {
    format!("{:#}", r.expect_err("the stamp was accepted"))
}

// ---- HMI version ----

#[test]
fn hmi_version_reads_the_hmi_line() {
    let f = common::fixture();
    fs::create_dir_all(f.layout.p("/ifs")).unwrap();
    fs::copy(Path::new(FIXTURES).join("version_info.txt"), f.layout.p("/ifs/version_info.txt")).unwrap();
    assert_eq!(hmi_version(&f.layout).as_deref(), Some("HMI_AU_MIB2_HQC_NAR_G22_C10_180803"));
}

#[test]
fn hmi_version_none_without_file() {
    let f = common::fixture();
    assert_eq!(hmi_version(&f.layout), None);
    fs::create_dir_all(f.layout.p("/ifs")).unwrap();
    fs::write(f.layout.p("/ifs/version_info.txt"), "MU = 0636\nHMIX = nope\n").unwrap();
    assert_eq!(hmi_version(&f.layout), None, "no HMI line");
}

// ---- check stamp (port of mibsdk-dev's STAMP_PY) ----

// was: test_warns_loudly_for_an_unrestricted_plugin
#[test]
fn stamp_accepts_plugin_stamp_and_reports_unrestricted() {
    let d = stamps();
    assert_eq!(verify_hello(d.path()).unwrap().unrestricted, Some(false));
    let p = d.path().join("hello.jar.stamp");
    let text = fs::read_to_string(&p).unwrap().replace("plugin.unrestricted=false", "plugin.unrestricted=true");
    fs::write(&p, text).unwrap();
    let v = verify_hello(d.path()).unwrap();
    assert_eq!(v.unrestricted, Some(true));
    assert_eq!(
        stamp::unrestricted_warning("hello"),
        "WARNING: hello declares platform.unrestricted: it may use reflection, exec or exit; the SDK cannot contain it"
    );
}

#[test]
fn stamp_accepts_unit_stamp_for_three_jars() {
    let d = stamps();
    assert_eq!(verify_unit(d.path()).unwrap().unrestricted, None);
}

// was: test_refuses_a_dist_without_a_stamp
// was: test_refuses_a_jar_without_a_stamp
#[test]
fn stamp_missing_file_is_missing() {
    let d = stamps();
    fs::remove_file(d.path().join("mibsdk-unit.stamp")).unwrap();
    assert_eq!(reason(verify_unit(d.path())), "missing");
    fs::remove_file(d.path().join("hello.jar.stamp")).unwrap();
    let why = reason(verify_hello(d.path()));
    assert_eq!(why, "missing");
    assert_eq!(
        stamp::plugin_refusal("hello", &why),
        "hello.jar has no valid check stamp (missing); only jars built by the mibsdk.plugin convention can be installed: ./gradlew :<project>:mibPluginJar (M7)"
    );
    assert_eq!(
        stamp::sdk_refusal(&why),
        "the mibsdk package's jars are not a checked build (missing); build it with ./gradlew :dist:mibpkgPackage; nothing changed (M7)"
    );
    // A jar that cannot be read is "missing" too.
    let d = stamps();
    fs::remove_file(d.path().join("hello.jar")).unwrap();
    assert_eq!(reason(verify_hello(d.path())), "missing");
}

#[test]
fn stamp_rejects_wrong_kind() {
    let d = stamps();
    let jar = d.path().join("hello.jar");
    let r = stamp::verify(&d.path().join("hello.jar.stamp"), StampKind::Unit, None, &[&jar]);
    assert_eq!(reason(r), "kind");
    let jars: Vec<PathBuf> = SDK_JARS.iter().map(|j| d.path().join(j)).collect();
    let refs: Vec<&Path> = jars.iter().map(|p| p.as_path()).collect();
    let r = stamp::verify(&d.path().join("mibsdk-unit.stamp"), StampKind::Plugin, Some("hello"), &refs);
    assert_eq!(reason(r), "kind");
}

// was: test_refuses_a_stamp_with_a_duplicate_key_or_no_checks
#[test]
fn stamp_rejects_duplicate_key_or_empty_checks() {
    let d = stamps();
    let p = d.path().join("hello.jar.stamp");
    let good = fs::read_to_string(&p).unwrap();
    let bad = [
        format!("{good}plugin.id=hello\n"),
        good.replace("checks=classVersion48,linkCheck,permissionScan", "checks="),
        good.replace("checks=classVersion48,linkCheck,permissionScan\n", ""),
        good.replace("plugin.unrestricted=false", "plugin.unrestricted=yes"),
        good.replace("plugin.unrestricted=false\n", ""),
        format!("{good}\n"),
        format!("{good}no equals sign\n"),
    ];
    for b in bad {
        fs::write(&p, &b).unwrap();
        assert_eq!(reason(verify_hello(d.path())), "format", "{b}");
    }
}

#[test]
fn stamp_rejects_first_line_not_format_1() {
    let d = stamps();
    let p = d.path().join("mibsdk-unit.stamp");
    let good = fs::read_to_string(&p).unwrap();
    for b in [good.replace("format=1", "format=2"), good.replacen("format=1\nkind=unit\n", "kind=unit\nformat=1\n", 1), String::new()] {
        fs::write(&p, &b).unwrap();
        assert_eq!(reason(verify_unit(d.path())), "format", "{b}");
    }
}

// was: test_refuses_a_stamp_for_another_id
#[test]
fn stamp_rejects_other_plugin_id() {
    let d = stamps();
    let p = d.path().join("hello.jar.stamp");
    let good = fs::read_to_string(&p).unwrap();
    fs::write(&p, good.replace("plugin.id=hello", "plugin.id=other")).unwrap();
    assert_eq!(reason(verify_hello(d.path())), "id other != hello");
    fs::write(&p, good.replace("plugin.id=hello\n", "")).unwrap();
    assert_eq!(reason(verify_hello(d.path())), "id None != hello");
}

// was: test_refuses_a_jar_that_does_not_match_its_stamp
#[test]
fn stamp_rejects_hash_mismatch_of_one_of_three_jars() {
    let d = stamps();
    fs::write(d.path().join("mibsdk-adapter-hqc.jar"), "rebuilt without the checks").unwrap();
    let why = reason(verify_unit(d.path()));
    assert_eq!(why, "hash mismatch for mibsdk-adapter-hqc.jar");
    assert_eq!(
        stamp::sdk_refusal(&why),
        "the mibsdk package's jars are not a checked build (hash mismatch for mibsdk-adapter-hqc.jar); build it with ./gradlew :dist:mibpkgPackage; nothing changed (M7)"
    );
}

// was: test_refuses_a_stale_stamp
#[test]
fn stamp_rejects_stale_stamp_after_jar_edit() {
    let d = stamps();
    let jar = d.path().join("hello.jar");
    let mut bytes = fs::read(&jar).unwrap();
    bytes.extend_from_slice(b"rebuilt after stamping");
    fs::write(&jar, bytes).unwrap();
    assert_eq!(reason(verify_hello(d.path())), "hash mismatch for hello.jar");
}

#[test]
fn stamp_rejects_jar_without_hash_line() {
    let d = stamps();
    let x = d.path().join("x.jar");
    fs::write(&x, "x").unwrap();
    let hello = d.path().join("hello.jar");
    let r = stamp::verify(&d.path().join("hello.jar.stamp"), StampKind::Plugin, Some("hello"), &[&hello, &x]);
    assert_eq!(reason(r), "no hash for x.jar");
}

#[test]
fn stamp_ignores_unknown_keys() {
    let d = stamps();
    let p = d.path().join("hello.jar.stamp");
    let good = fs::read_to_string(&p).unwrap();
    // Latin-1 bytes in an unknown value must not trip the decoder.
    let mut b = good.into_bytes();
    b.extend_from_slice(b"future.key=caf\xe9\n");
    fs::write(&p, b).unwrap();
    assert_eq!(verify_hello(d.path()).unwrap().unrestricted, Some(false));
}

// was: test_a_restricted_plugin_gets_no_warning
#[test]
fn stamp_restricted_plugin_has_no_warning() {
    let d = stamps();
    let v = verify_hello(d.path()).unwrap();
    assert_eq!(v.unrestricted, Some(false));
    assert_eq!(v.warning("hello"), None);
    let p = d.path().join("hello.jar.stamp");
    fs::write(&p, fs::read_to_string(&p).unwrap().replace("unrestricted=false", "unrestricted=true")).unwrap();
    assert_eq!(verify_hello(d.path()).unwrap().warning("hello"), Some(stamp::unrestricted_warning("hello")));
}

// ---- put: verified writes to a mount others may flip read-only ----

const TARGET: &str = "/mnt/app/eso/hmi/lsd/mibsdk/plugins/hello.jar";

fn temp_of(ctx: &Ctx, dst_abs: &str) -> PathBuf {
    ctx.layout.p(dst_abs).parent().unwrap().join(".mibpkg.tmp")
}

fn rw_calls(sys: &RecordingSys) -> usize {
    sys.calls.borrow().iter().filter(|c| *c == "rw /mnt/app").count()
}

/// Real file operations, except that the first `bad_renames` renames report success without
/// moving anything (QNX `mv` on a read-only /mnt/app exits 0).
struct FlakyRename {
    bad_renames: Cell<u32>,
    renames: RefCell<Vec<(PathBuf, PathBuf)>>,
}

impl FlakyRename {
    fn new(bad: u32) -> FlakyRename {
        FlakyRename { bad_renames: Cell::new(bad), renames: RefCell::new(vec![]) }
    }
}

impl FileOps for FlakyRename {
    fn write(&self, p: &Path, b: &[u8], mode: u32) -> io::Result<()> {
        mibsdk::StdOps.write(p, b, mode)
    }
    fn read(&self, p: &Path) -> io::Result<Vec<u8>> {
        mibsdk::StdOps.read(p)
    }
    fn rename(&self, a: &Path, b: &Path) -> io::Result<()> {
        self.renames.borrow_mut().push((a.to_path_buf(), b.to_path_buf()));
        if self.bad_renames.get() > 0 {
            self.bad_renames.set(self.bad_renames.get() - 1);
            return Ok(());
        }
        mibsdk::StdOps.rename(a, b)
    }
    fn remove(&self, p: &Path) -> io::Result<()> {
        mibsdk::StdOps.remove(p)
    }
}

#[test]
fn put_writes_and_reads_back() {
    let f = common::fixture();
    let sys = RecordingSys::default();
    let ctx = Ctx::new(f.layout.clone(), &sys);
    fs::create_dir_all(f.layout.p("/mnt/app/eso/hmi/lsd/mibsdk/plugins")).unwrap();
    fs::write(f.layout.p(TARGET), "old").unwrap();
    let _rw = ctx.mounts.rw("/mnt/app").unwrap();
    mibsdk::put(&ctx, TARGET, b"new bytes", 0o644).unwrap();
    assert_eq!(fs::read(f.layout.p(TARGET)).unwrap(), b"new bytes");
    use std::os::unix::fs::PermissionsExt;
    assert_eq!(fs::metadata(f.layout.p(TARGET)).unwrap().permissions().mode() & 0o777, 0o644);
    assert!(!temp_of(&ctx, TARGET).exists());

    // put_file sends the bytes of a local file.
    let src = f.root().join("src.jar");
    fs::write(&src, "from a file").unwrap();
    mibsdk::put_file(&ctx, TARGET, &src, 0o600).unwrap();
    assert_eq!(fs::read(f.layout.p(TARGET)).unwrap(), b"from a file");
    assert_eq!(fs::metadata(f.layout.p(TARGET)).unwrap().permissions().mode() & 0o777, 0o600);
}

// was: test_push_survives_another_process_remounting_app_read_only
#[test]
fn put_retries_once_after_an_outside_ro_remount() {
    let f = common::fixture();
    let sys = RecordingSys::default();
    let ctx = Ctx::new(f.layout.clone(), &sys);
    fs::create_dir_all(f.layout.p("/mnt/app/eso/hmi/lsd/mibsdk/plugins")).unwrap();
    fs::write(f.layout.p(TARGET), "old").unwrap();
    let _rw = ctx.mounts.rw("/mnt/app").unwrap();
    sys.calls.borrow_mut().clear();
    let ops = FlakyRename::new(1);
    mibsdk::put_with(&ctx, TARGET, b"new", 0o644, &ops).unwrap();
    assert_eq!(fs::read(f.layout.p(TARGET)).unwrap(), b"new");
    assert_eq!(ops.renames.borrow().len(), 2, "one retry");
    // Once before the first attempt, once before the retry.
    assert_eq!(rw_calls(&sys), 2, "{:?}", sys.calls.borrow());
    assert!(!temp_of(&ctx, TARGET).exists());
}

// was: test_a_move_that_fails_with_status_0_fails_the_push
// was: test_a_failed_push_leaves_no_temp_file
#[test]
fn put_fails_after_second_failure_and_leaves_no_temp_file() {
    let f = common::fixture();
    let sys = RecordingSys::default();
    let ctx = Ctx::new(f.layout.clone(), &sys);
    fs::create_dir_all(f.layout.p("/mnt/app/eso/hmi/lsd/mibsdk/plugins")).unwrap();
    fs::write(f.layout.p(TARGET), "old").unwrap();
    let _rw = ctx.mounts.rw("/mnt/app").unwrap();
    let ops = FlakyRename::new(2);
    let e = mibsdk::put_with(&ctx, TARGET, b"new", 0o644, &ops).unwrap_err();
    assert!(format!("{e:#}").contains(TARGET), "{e:#}");
    assert_eq!(ops.renames.borrow().len(), 2, "exactly one retry");
    assert_eq!(fs::read(f.layout.p(TARGET)).unwrap(), b"old");
    assert!(!temp_of(&ctx, TARGET).exists());

    // A write that fails outright (EROFS) also leaves nothing behind.
    struct Erofs;
    impl FileOps for Erofs {
        fn write(&self, p: &Path, _b: &[u8], _mode: u32) -> io::Result<()> {
            fs::write(p, "partial")?;
            Err(io::Error::other("Read-only file system"))
        }
        fn read(&self, p: &Path) -> io::Result<Vec<u8>> {
            fs::read(p)
        }
        fn rename(&self, a: &Path, b: &Path) -> io::Result<()> {
            fs::rename(a, b)
        }
        fn remove(&self, p: &Path) -> io::Result<()> {
            fs::remove_file(p)
        }
    }
    assert!(mibsdk::put_with(&ctx, TARGET, b"new", 0o644, &Erofs).is_err());
    assert_eq!(fs::read(f.layout.p(TARGET)).unwrap(), b"old");
    assert!(!temp_of(&ctx, TARGET).exists());
}

// was: test_install_sweeps_stale_push_temp_files
#[test]
fn put_removes_a_stale_temp_file_first() {
    let f = common::fixture();
    let sys = RecordingSys::default();
    let ctx = Ctx::new(f.layout.clone(), &sys);
    fs::create_dir_all(f.layout.p("/mnt/app/eso/hmi/lsd/mibsdk/plugins")).unwrap();
    let stale = temp_of(&ctx, TARGET);
    fs::write(&stale, "left by an interrupted run").unwrap();

    struct Recording(RefCell<Vec<String>>);
    impl FileOps for Recording {
        fn write(&self, p: &Path, b: &[u8], mode: u32) -> io::Result<()> {
            self.0.borrow_mut().push(format!("write exists={}", p.exists()));
            mibsdk::StdOps.write(p, b, mode)
        }
        fn read(&self, p: &Path) -> io::Result<Vec<u8>> {
            mibsdk::StdOps.read(p)
        }
        fn rename(&self, a: &Path, b: &Path) -> io::Result<()> {
            mibsdk::StdOps.rename(a, b)
        }
        fn remove(&self, p: &Path) -> io::Result<()> {
            self.0.borrow_mut().push("remove".into());
            mibsdk::StdOps.remove(p)
        }
    }
    let _rw = ctx.mounts.rw("/mnt/app").unwrap();
    let ops = Recording(RefCell::new(vec![]));
    mibsdk::put_with(&ctx, TARGET, b"new", 0o644, &ops).unwrap();
    assert_eq!(ops.0.borrow()[..2], ["remove".to_string(), "write exists=false".to_string()]);
    assert_eq!(fs::read(f.layout.p(TARGET)).unwrap(), b"new");
    assert!(!stale.exists());
}

#[test]
fn put_outside_mnt_app_does_not_remount() {
    let f = common::fixture();
    let sys = RecordingSys::default();
    let ctx = Ctx::new(f.layout.clone(), &sys);
    let dst = "/mnt/persist/mibsdk/plugins.properties";
    fs::create_dir_all(f.layout.p("/mnt/persist/mibsdk")).unwrap();
    // No rw guard held: /mnt/persist is writable without a remount.
    let ops = FlakyRename::new(1);
    mibsdk::put_with(&ctx, dst, b"hello.off=true\n", 0o644, &ops).unwrap();
    assert_eq!(fs::read(f.layout.p(dst)).unwrap(), b"hello.off=true\n");
    assert_eq!(ops.renames.borrow().len(), 2, "still retried once");
    assert!(sys.calls.borrow().is_empty(), "{:?}", sys.calls.borrow());
    assert!(!temp_of(&ctx, dst).exists());
}

// ---- installed SDK ----

#[test]
fn installed_sdk_none_until_installed() {
    let f = common::fixture();
    assert!(mibsdk::installed_sdk(&f.layout).is_none());
    let ver_dir = f.layout.pkg_dir(mibsdk::SDK_PKG).join("0.1.0");
    fs::create_dir_all(&ver_dir).unwrap();
    let manifest = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/manifests/sdk-mibsdk.toml");
    fs::copy(manifest, ver_dir.join("manifest.toml")).unwrap();
    assert!(mibsdk::installed_sdk(&f.layout).is_none(), "no current pointer yet");
    fs::write(f.layout.pkg_dir(mibsdk::SDK_PKG).join("current"), "0.1.0\n").unwrap();
    let (ver, spec, dir) = mibsdk::installed_sdk(&f.layout).unwrap();
    assert_eq!(ver, "0.1.0");
    assert_eq!(spec.stamp, "mibsdk-unit.stamp");
    assert_eq!(dir, ver_dir);

    // Running: the log exists and no reboot is pending.
    assert!(!mibsdk::sdk_running(&f.layout));
    fs::write(f.layout.p(mibsdk::LOG), "").unwrap();
    assert!(mibsdk::sdk_running(&f.layout));
    assert_eq!(mibsdk::reboot_flag(&f.layout), f.layout.run("sdk-reboot"));
    fs::write(mibsdk::reboot_flag(&f.layout), "").unwrap();
    assert!(!mibsdk::sdk_running(&f.layout));
}

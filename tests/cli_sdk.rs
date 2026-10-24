//! The CLI's action flags (`--approve`, `--force`, `--purge`/`--purge-data`), `install-file`, and
//! the `mibpkg sdk` verbs, run as the real binary against a temp root. Ports
//! `installer/test_mibsdk_dev.py`'s push, safe-mode, boot-state, status and data-clear tests from
//! the SDK repo (each port carries `// was: <python test>`).
mod common;
use mibpkg::archive;
use mibpkg::ctx::Ctx;
use mibpkg::install;
use mibpkg::mibsdk;
use mibpkg::state::State;
use mibpkg::sys::RecordingSys;
use std::fs;
use std::io::Write;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::thread;
use std::time::{Duration, Instant};

const ID: &str = "0123456789abcdef0123456789abcdef";
const FIX: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/mibsdk");
const SDK_MANIFEST: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/manifests/sdk-mibsdk.toml");
const SDK_JARS: [&str; 3] = ["mibsdk-runtime.jar", "mibsdk-adapter-hqc.jar", "mibsdk-rt-retrotranslator.jar"];
const HMICAR_SDK: &str = "HMICar=mibsdk.adapter.hqc.ui.SdkCarActivator";
const READ: &str = "Read vehicle state: ignition, gear, parking sensors (vehicle.read)";
const NET: &str = "Open network connections (local network or internet) (network)";
const NO_SDK: &str = "mibsdk is not installed (run: mibpkg install mibsdk, or mibsdk-dev install)";

// ---- running the binary ----

struct Run {
    code: i32,
    out: String,
    err: String,
}

fn mibpkg(f: &common::Fixture, args: &[&str]) -> Run {
    let o: Output = Command::new(env!("CARGO_BIN_EXE_mibpkg"))
        .arg("--root")
        .arg(f.root())
        .arg("--no-supervisor")
        .args(args)
        .env("MIBPKG_SDK_REMOVE_WAIT_S", "0")
        .output()
        .unwrap();
    Run {
        code: o.status.code().unwrap_or(-1),
        out: String::from_utf8_lossy(&o.stdout).into_owned(),
        err: String::from_utf8_lossy(&o.stderr).into_owned(),
    }
}

fn ok(f: &common::Fixture, args: &[&str]) -> Run {
    let r = mibpkg(f, args);
    assert_eq!(r.code, 0, "{args:?}: stdout {:?} stderr {:?}", r.out, r.err);
    r
}

fn failed(f: &common::Fixture, args: &[&str]) -> Run {
    let r = mibpkg(f, args);
    assert_ne!(r.code, 0, "{args:?} should fail: stdout {:?}", r.out);
    r
}

// ---- fixtures (as tests/plugin_pkg.rs) ----

fn p(f: &common::Fixture, abs: &str) -> PathBuf {
    f.layout.p(abs)
}

fn write(path: &Path, data: &[u8]) {
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, data).unwrap();
}

fn read(path: &Path) -> String {
    fs::read_to_string(path).unwrap()
}

fn plugin_jar(f: &common::Fixture, id: &str) -> PathBuf {
    p(f, &format!("{}/plugins/{id}.jar", mibsdk::BASE))
}

fn marker(f: &common::Fixture, id: &str) -> PathBuf {
    p(f, &format!("{}/plugins/{id}.reload", mibsdk::BASE))
}

fn data(f: &common::Fixture, id: &str) -> PathBuf {
    p(f, &format!("{}/data/{id}.properties", mibsdk::PERSIST))
}

fn boot_state(f: &common::Fixture) -> PathBuf {
    p(f, &format!("{}/boot.state", mibsdk::PERSIST))
}

fn grants_text(f: &common::Fixture) -> String {
    fs::read_to_string(p(f, mibsdk::GRANTS)).unwrap_or_default()
}

fn installed_pkg(f: &common::Fixture, id: &str) -> bool {
    State::load(&f.layout).unwrap().packages.contains_key(id)
}

fn is_token(s: &str) -> bool {
    let t = s.strip_suffix('\n').unwrap_or("");
    let parts: Vec<&str> = t.split('-').collect();
    parts.len() == 3 && parts.iter().all(|p| !p.is_empty() && p.bytes().all(|b| b.is_ascii_digit()))
}

fn write_stamp(dir: &Path, name: &str, kind: &str, plugin: Option<(&str, bool)>, jars: &[&str]) {
    let mut lines = vec!["format=1".to_string(), format!("kind={kind}"), "checks=classVersion48,linkCheck,permissionScan".to_string()];
    if let Some((id, unrestricted)) = plugin {
        lines.push(format!("plugin.id={id}"));
        lines.push(format!("plugin.unrestricted={unrestricted}"));
    }
    let mut sorted = jars.to_vec();
    sorted.sort();
    for j in sorted {
        let (h, _) = archive::sha256_file(&dir.join(j)).unwrap();
        lines.push(format!("sha256.{j}={h}"));
    }
    fs::write(dir.join(name), lines.join("\n") + "\n").unwrap();
}

fn sdk_unit(f: &common::Fixture) {
    write(&p(f, mibsdk::BUNDLES), &fs::read(Path::new(FIX).join("bundles_dsitracer_enabled.properties")).unwrap());
    fs::set_permissions(p(f, mibsdk::BUNDLES), fs::Permissions::from_mode(0o777)).unwrap();
    write(&p(f, mibsdk::MARKER), b"");
    write(&p(f, mibsdk::TRACER_JAR), b"tracer");
    fs::create_dir_all(p(f, mibsdk::JARS)).unwrap();
    write(&p(f, mibsdk::VERSION_INFO), &fs::read(Path::new(FIX).join("version_info.txt")).unwrap());
    fs::create_dir_all(p(f, "/mnt/persist")).unwrap();
}

fn sdk_src(dir: &Path) -> PathBuf {
    let s = dir.join("src-mibsdk-0.1.0");
    fs::create_dir_all(&s).unwrap();
    fs::write(s.join("manifest.toml"), read(Path::new(SDK_MANIFEST))).unwrap();
    for j in SDK_JARS {
        fs::write(s.join(j), format!("{j} 0.1.0\n")).unwrap();
    }
    write_stamp(&s, "mibsdk-unit.stamp", "unit", None, &SDK_JARS);
    fs::copy(Path::new(FIX).join("permissions.json"), s.join("permissions.json")).unwrap();
    s
}

fn install_src(ctx: &Ctx, src: &Path) -> anyhow::Result<String> {
    let mpk = src.with_extension("mpk");
    archive::pack(src, &mpk).unwrap();
    install::install_package(ctx, &[], &archive::Package::read(&mpk).unwrap(), "test", "Test")
}

/// The unit with the SDK package installed (not running: it waits for the reboot).
fn with_sdk(f: &common::Fixture, dir: &Path) {
    sdk_unit(f);
    let sys = RecordingSys::default();
    let ctx = Ctx::new(f.layout.clone(), &sys);
    install_src(&ctx, &sdk_src(dir)).unwrap();
}

/// The SDK runtime started this boot: no reboot pending, and its log exists.
fn running(f: &common::Fixture) {
    let _ = fs::remove_file(mibsdk::reboot_flag(&f.layout));
    write(&p(f, mibsdk::LOG), b"100 STATUS mibsdk runtime started adapter=hqc safeMode=false\n");
}

fn plugin_src(dir: &Path, id: &str, ver: &str, perms: &[&str]) -> PathBuf {
    let s = dir.join(format!("src-{id}-{ver}"));
    fs::create_dir_all(&s).unwrap();
    let list: Vec<String> = perms.iter().map(|p| format!("{p:?}")).collect();
    fs::write(
        s.join("manifest.toml"),
        format!("[package]\nname = \"{id}\"\nversion = \"{ver}\"\n\n[plugin]\njar = \"{id}.jar\"\napi = 2\npermissions = [{}]\n", list.join(", ")),
    )
    .unwrap();
    fs::write(s.join(format!("{id}.jar")), format!("{id} {ver} jar\n")).unwrap();
    write_stamp(&s, &format!("{id}.jar.stamp"), "plugin", Some((id, perms.contains(&"platform.unrestricted"))), &[&format!("{id}.jar")]);
    s
}

/// Install plugin package `id` with every declared permission approved.
fn install_plugin(f: &common::Fixture, dir: &Path, id: &str, perms: &[&str]) {
    let sys = RecordingSys::default();
    let mut ctx = Ctx::new(f.layout.clone(), &sys);
    ctx.approved = Some(perms.iter().map(|s| s.to_string()).collect());
    install_src(&ctx, &plugin_src(dir, id, "1.0.0", perms)).unwrap();
}

fn upload_plugin(f: &common::Fixture, dir: &Path, id: &str, perms: &[&str]) {
    archive::pack(&plugin_src(dir, id, "1.0.0", perms), &f.layout.run(&format!("upload.{ID}.mpk"))).unwrap();
}

/// A jar `mibsdk-dev push` left in BASE/plugins (no package).
fn pushed(f: &common::Fixture, id: &str) {
    write(&plugin_jar(f, id), format!("pushed {id}\n").as_bytes());
}

/// What `mibsdk-dev push` uploads: `/tmp/mibpkg.push.<id>.jar` and its stamp. `stamp`: None for
/// no stamp, else (stamp id, unrestricted).
fn push_upload(f: &common::Fixture, dir: &Path, id: &str, content: &str, stamp: Option<(&str, bool)>) -> (PathBuf, PathBuf) {
    let d = dir.join(format!("push-{id}-{}", content.len()));
    fs::create_dir_all(&d).unwrap();
    let jar = format!("{id}.jar");
    fs::write(d.join(&jar), content).unwrap();
    let (tjar, tstamp) = (f.layout.run(&format!("push.{id}.jar")), f.layout.run(&format!("push.{id}.jar.stamp")));
    fs::copy(d.join(&jar), &tjar).unwrap();
    let _ = fs::remove_file(&tstamp);
    if let Some(s) = stamp {
        write_stamp(&d, &format!("{jar}.stamp"), "plugin", Some(s), &[&jar]);
        fs::copy(d.join(format!("{jar}.stamp")), &tstamp).unwrap();
    }
    (tjar, tstamp)
}

fn append_log(path: &Path, text: &str) {
    let mut fh = fs::OpenOptions::new().append(true).create(true).open(path).unwrap();
    fh.write_all(text.as_bytes()).unwrap();
}

/// The SDK runtime's side of a reload: once `plugins/<id>.reload` changes, log the dev channel's
/// line (with the marker's token) and then `STATUS <id> <status>`.
fn fake_runtime(f: &common::Fixture, id: &str, status: &str) -> thread::JoinHandle<bool> {
    let (m, log, id, status) = (marker(f, id), p(f, mibsdk::LOG), id.to_string(), status.to_string());
    let before = fs::read(&m).ok();
    thread::spawn(move || {
        let end = Instant::now() + Duration::from_secs(10);
        while Instant::now() < end {
            let now = fs::read(&m).ok();
            if now.is_some() && now != before {
                append_log(&log, &format!("200 INFO dev dev channel: reload requested for {id}\n"));
                append_log(&log, &format!("202 STATUS {id} {status}\n"));
                return true;
            }
            thread::sleep(Duration::from_millis(20));
        }
        false
    })
}

// ---- install-file, --approve, --force, --purge ----

#[test]
fn install_file_needing_approval_exits_3_and_prints_approve_hint() {
    let (f, dir) = (common::fixture(), tempfile::tempdir().unwrap());
    with_sdk(&f, dir.path());
    upload_plugin(&f, dir.path(), "hello", &["vehicle.read", "network"]);
    let r = mibpkg(&f, &["install-file", "hello", ID]);
    assert_eq!(r.code, 3, "{r:?}", r = (&r.out, &r.err));
    // Progress lines first, then exactly the request.
    assert!(r.out.ends_with(&format!("\n{READ}\n{NET}\napprove with: --approve=vehicle.read,network\n")), "{}", r.out);
    assert!(!installed_pkg(&f, "hello"));
    assert!(!plugin_jar(&f, "hello").exists());
    assert!(!f.layout.run(&format!("upload.{ID}.mpk")).exists(), "the upload is used once");
    // An approval that does not cover every ask is no approval.
    upload_plugin(&f, dir.path(), "hello", &["vehicle.read", "network"]);
    assert_eq!(mibpkg(&f, &["install-file", "hello", ID, "-", "--approve=network"]).code, 3);
    // The gem arm (the web's path) records FAILED with the request in plain words.
    upload_plugin(&f, dir.path(), "hello", &["vehicle.read", "network"]);
    assert_eq!(mibpkg(&f, &["gem", "install-file", "hello", ID, "-"]).code, 0);
    let res = State::load(&f.layout).unwrap().results["hello"].clone();
    assert!(res.starts_with(&format!("FAILED: approve these permissions to install hello 1.0.0: {READ}")), "{res}");
}

#[test]
fn install_file_with_approve_installs() {
    let (f, dir) = (common::fixture(), tempfile::tempdir().unwrap());
    with_sdk(&f, dir.path());
    upload_plugin(&f, dir.path(), "hello", &["vehicle.read", "network"]);
    let r = ok(&f, &["install-file", "--approve=vehicle.read,network", "hello", ID]);
    assert!(r.out.contains("\ninstalled hello 1.0.0"), "{}", r.out);
    assert!(installed_pkg(&f, "hello"));
    assert_eq!(grants_text(&f), "hello=network,vehicle.read\n");
    assert_eq!(State::load(&f.layout).unwrap().packages["hello"].collection, "uploaded");
    // Through the gem arm too (the web appends --approve=… to its argv).
    upload_plugin(&f, dir.path(), "bike", &["vehicle.read"]);
    assert_eq!(mibpkg(&f, &["gem", "install-file", "bike", ID, "-", "--approve=vehicle.read"]).code, 0);
    let res = State::load(&f.layout).unwrap().results["bike"].clone();
    assert!(res.starts_with("OK: installed bike 1.0.0"), "{res}");
    // A malformed approval list changes nothing.
    upload_plugin(&f, dir.path(), "other", &[]);
    let r = failed(&f, &["install-file", "other", ID, "-", "--approve=a b"]);
    assert!(r.err.contains("invalid permission"), "{}", r.err);
    assert!(!installed_pkg(&f, "other"));
}

#[test]
fn install_file_rejects_short_upload_id() {
    let (f, dir) = (common::fixture(), tempfile::tempdir().unwrap());
    with_sdk(&f, dir.path());
    upload_plugin(&f, dir.path(), "hello", &[]);
    for bad in ["0123", "../x", "0123456789ABCDEF0123456789ABCDEF"] {
        let r = failed(&f, &["install-file", "hello", bad]);
        assert!(r.err.contains("invalid id"), "{bad}: {}", r.err);
    }
    assert!(!installed_pkg(&f, "hello"));
}

#[test]
fn update_all_refuses_a_global_approval() {
    let f = common::fixture();
    let r = failed(&f, &["update-all", "--approve=vehicle.read"]);
    assert!(r.err.contains("mibpkg install <pkg> --approve="), "{}", r.err);
}

#[test]
fn remove_mibsdk_force_flag_reaches_undeploy() {
    let (f, dir) = (common::fixture(), tempfile::tempdir().unwrap());
    with_sdk(&f, dir.path());
    pushed(&f, "hello");
    let r = failed(&f, &["remove", "mibsdk"]);
    assert!(r.err.contains("pushed by mibsdk-dev: hello"), "{}", r.err);
    assert!(installed_pkg(&f, "mibsdk"));
    let r = ok(&f, &["remove", "mibsdk", "--force"]);
    assert!(r.out.contains("removed mibsdk"), "{}", r.out);
    assert!(!installed_pkg(&f, "mibsdk"));
}

#[test]
fn remove_accepts_purge_data_alias() {
    let f = common::fixture();
    let src = common::pkg_src(f.root(), "demo", "1.0.0", "sleep 30", "");
    common::collection(&f.sd(), "complex256", &[src]);
    for flag in ["--purge", "--purge-data"] {
        ok(&f, &["install", "demo"]);
        write(&f.layout.config_dir("demo").join("demo.conf"), b"x");
        let r = ok(&f, &["remove", "demo", flag]);
        assert!(r.out.contains("\nremoved demo and its config"), "{flag}: {}", r.out);
        assert!(!f.layout.config_dir("demo").exists(), "{flag}");
    }
}

// ---- sdk status ----

#[test]
fn sdk_status_lists_plugins_pushed_off_blocked_grants_ledger_and_hmicar() {
    let (f, dir) = (common::fixture(), tempfile::tempdir().unwrap());
    with_sdk(&f, dir.path());
    install_plugin(&f, dir.path(), "hello", &["vehicle.read", "network"]);
    pushed(&f, "pushy");
    // Revoke network for hello, grant the pushed one, switch it off.
    write(&p(&f, mibsdk::GRANTS), b"hello=vehicle.read\npushy=vehicle.read,hooks.parking\n");
    write(&p(&f, mibsdk::TOGGLES), b"pushy.off=true\n");
    write(&p(&f, mibsdk::LEDGER), b"parking.soundLevel=pushy|PERSISTENT|a|b|A\n");
    write(&p(&f, mibsdk::LOG), b"1 INFO x\n2 STATUS hello ENABLED ok\n");
    let r = ok(&f, &["sdk", "status"]);
    assert_eq!(
        r.out,
        format!(
            "{HMICAR_SDK}\nsdk: 0.1.0\napi: 1..2\nhmi: HMI_AU_MIB2_HQC_NAR_G22_C10_180803 (supported)\nrunning: no\nreboot needed: yes\n\
             plugin hello 1.0.0 grants=vehicle.read blocked: network\n\
             plugin pushy pushed grants=hooks.parking,vehicle.read off\n\
             ledger: 1 override(s)\nparking.soundLevel=pushy|PERSISTENT|a|b|A\n\
             WARNING: vehicle overrides are in force; disable their plugins before a firmware update (a firmware without a matching adapter cannot restore them)\n\
             2 STATUS hello ENABLED ok\n"
        )
    );
}

#[test]
fn sdk_status_prints_the_installed_hmicar_line() {
    // was: test_prints_the_installed_hmicar_line
    let (f, dir) = (common::fixture(), tempfile::tempdir().unwrap());
    with_sdk(&f, dir.path());
    write(&p(&f, mibsdk::LOG), b"x STATUS ui=on\n");
    let r = ok(&f, &["sdk", "status"]);
    assert!(r.out.starts_with(&format!("{HMICAR_SDK}\n")), "{}", r.out);
    assert!(r.out.ends_with("ledger: 0 override(s)\nx STATUS ui=on\n"), "{}", r.out);
}

#[test]
fn sdk_status_without_hmicar_line_or_readable_bundles() {
    // was: test_missing_hmicar_line_and_unreadable_bundles
    let f = common::fixture();
    write(&p(&f, mibsdk::LOG), b"x STATUS ui=on\n");
    write(&p(&f, mibsdk::BUNDLES), b"Bundle.Startup.Autostart = A\r\n");
    let r = ok(&f, &["sdk", "status"]);
    assert!(r.out.starts_with("HMICar=<missing>\nsdk: not installed\n"), "{}", r.out);
    assert!(r.out.ends_with("x STATUS ui=on\n"), "{}", r.out);
    fs::remove_file(p(&f, mibsdk::BUNDLES)).unwrap();
    let r = ok(&f, &["sdk", "status"]);
    assert!(r.out.starts_with("HMICar=<missing>\n"), "{}", r.out);
    assert!(r.out.ends_with("x STATUS ui=on\n"), "{}", r.out);
    assert!(r.err.contains("warning: cannot read"), "{}", r.err);
}

#[test]
fn sdk_status_warns_about_ledger_before_firmware_update() {
    let (f, dir) = (common::fixture(), tempfile::tempdir().unwrap());
    with_sdk(&f, dir.path());
    let r = ok(&f, &["sdk", "status"]);
    assert!(!r.out.contains("WARNING"), "{}", r.out);
    write(&p(&f, mibsdk::LEDGER), b"k=hello|PERSISTENT|a|b|A\nj=hello|PERSISTENT|a|b|R\n");
    let r = ok(&f, &["sdk", "status"]);
    assert!(r.out.contains("ledger: 2 override(s)\nk=hello|PERSISTENT|a|b|A\nj=hello|PERSISTENT|a|b|R\nWARNING: vehicle overrides are in force; disable their plugins before a firmware update"), "{}", r.out);
    assert!(!r.out.contains("cannot restore these overrides"), "{}", r.out);
    // A firmware the SDK does not support.
    write(&p(&f, mibsdk::VERSION_INFO), b"HMI = HMI_SOMETHING_ELSE\n");
    let r = ok(&f, &["sdk", "status"]);
    assert!(r.out.contains("hmi: HMI_SOMETHING_ELSE (UNSUPPORTED)\n"), "{}", r.out);
    assert!(r.out.contains("\nWARNING: the SDK is off on this firmware and cannot restore these overrides\n"), "{}", r.out);
}

// ---- sdk grant ----

#[test]
fn sdk_grant_rewrites_line_and_writes_marker() {
    let (f, dir) = (common::fixture(), tempfile::tempdir().unwrap());
    with_sdk(&f, dir.path());
    pushed(&f, "hello");
    write(&p(&f, mibsdk::GRANTS), b"# kept\nother=network\nhello=vehicle.read\nhello-menu=vehicle.read\n");
    running(&f);
    let rt = fake_runtime(&f, "hello", "ENABLED ok");
    let r = ok(&f, &["sdk", "grant", "hello", "vehicle.read,network"]);
    assert!(rt.join().unwrap(), "no reload requested");
    assert_eq!(r.out, "granted hello: network,vehicle.read\nloaded: ENABLED ok\n");
    assert_eq!(grants_text(&f), "# kept\nother=network\nhello=network,vehicle.read\nhello-menu=vehicle.read\n");
    assert!(is_token(&read(&marker(&f, "hello"))));
    for bad in [&["sdk", "grant", "hello", "a b"][..], &["sdk", "grant", "../x", "network"], &["sdk", "grant", "hello"]] {
        failed(&f, bad);
    }
    assert_eq!(grants_text(&f), "# kept\nother=network\nhello=network,vehicle.read\nhello-menu=vehicle.read\n");
}

#[test]
fn sdk_grant_empty_list_revokes_all() {
    let (f, dir) = (common::fixture(), tempfile::tempdir().unwrap());
    with_sdk(&f, dir.path());
    install_plugin(&f, dir.path(), "hello", &["vehicle.read"]);
    running(&f);
    let rt = fake_runtime(&f, "hello", "BLOCKED_PERMISSIONS missing permissions: vehicle.read");
    let r = ok(&f, &["sdk", "grant", "hello", ""]);
    assert!(rt.join().unwrap());
    assert_eq!(r.out, "granted hello: <none>\nloaded: BLOCKED_PERMISSIONS missing permissions: vehicle.read\n");
    assert_eq!(grants_text(&f), "hello=\n");
}

// ---- sdk push ----

#[test]
fn sdk_push_refuses_unstamped_and_package_owned_ids_and_cleans_tmp() {
    let (f, dir) = (common::fixture(), tempfile::tempdir().unwrap());
    with_sdk(&f, dir.path());
    let (jar, stamp) = push_upload(&f, dir.path(), "hello", "v1", None);
    let r = failed(&f, &["sdk", "push", "hello"]);
    assert!(r.err.contains("has no valid check stamp (missing)") && r.err.contains(":mibPluginJar"), "{}", r.err);
    assert!(!jar.exists() && !stamp.exists());
    assert!(!plugin_jar(&f, "hello").exists());
    // A stamp for another id, then a stale one.
    push_upload(&f, dir.path(), "hello", "v1", Some(("other", false)));
    let r = failed(&f, &["sdk", "push", "hello"]);
    assert!(r.err.contains("(id other != hello)"), "{}", r.err);
    let (jar, stamp) = push_upload(&f, dir.path(), "hello", "v1", Some(("hello", false)));
    fs::write(&jar, "rebuilt after stamping").unwrap();
    let r = failed(&f, &["sdk", "push", "hello"]);
    assert!(r.err.contains("(hash mismatch for hello.jar)"), "{}", r.err);
    assert!(!jar.exists() && !stamp.exists());
    assert!(!plugin_jar(&f, "hello").exists());
    // An id a package owns.
    install_plugin(&f, dir.path(), "bike", &[]);
    let before = fs::read(plugin_jar(&f, "bike")).unwrap();
    let (jar, stamp) = push_upload(&f, dir.path(), "bike", "dev build", Some(("bike", false)));
    let r = failed(&f, &["sdk", "push", "bike"]);
    assert!(r.err.contains("bike is installed as a package; remove it first or install a newer version"), "{}", r.err);
    assert_eq!(fs::read(plugin_jar(&f, "bike")).unwrap(), before);
    assert!(!jar.exists() && !stamp.exists());
    // A bad id names no /tmp file.
    failed(&f, &["sdk", "push", "../x"]);
}

#[test]
fn sdk_push_refuses_without_sdk() {
    let (f, dir) = (common::fixture(), tempfile::tempdir().unwrap());
    let (jar, stamp) = push_upload(&f, dir.path(), "hello", "v1", Some(("hello", false)));
    let r = failed(&f, &["sdk", "push", "hello"]);
    assert!(r.err.contains(NO_SDK), "{}", r.err);
    assert!(!jar.exists() && !stamp.exists());
    assert!(!plugin_jar(&f, "hello").exists());
    // remove and data-clear work without the SDK (see sdk_remove_and_data_clear_work_after_the_sdk_is_gone).
    for verb in [&["sdk", "grant", "hello", "network"][..], &["sdk", "safe-mode", "on"], &["sdk", "boot-state", "set-wrap", "1"]] {
        let r = failed(&f, verb);
        assert!(r.err.contains(NO_SDK), "{verb:?}: {}", r.err);
    }
}

#[test]
fn sdk_push_keeps_existing_grants_and_writes_fresh_token() {
    // was: test_each_push_writes_a_fresh_reload_marker
    let (f, dir) = (common::fixture(), tempfile::tempdir().unwrap());
    with_sdk(&f, dir.path());
    write(&p(&f, mibsdk::GRANTS), b"hello=vehicle.read\n");
    let (jar, stamp) = push_upload(&f, dir.path(), "hello", "v1", Some(("hello", false)));
    let bytes = fs::read(&jar).unwrap();
    let r = ok(&f, &["sdk", "push", "hello"]);
    assert!(r.out.starts_with("pushed hello; reload requested"), "{}", r.out);
    assert!(!r.err.contains("unrestricted"), "{}", r.err);
    assert_eq!(fs::read(plugin_jar(&f, "hello")).unwrap(), bytes);
    assert_eq!(grants_text(&f), "hello=vehicle.read\n");
    assert!(!jar.exists() && !stamp.exists());
    let first = read(&marker(&f, "hello"));
    assert!(is_token(&first), "{first:?}");
    push_upload(&f, dir.path(), "hello", "v1", Some(("hello", false)));
    ok(&f, &["sdk", "push", "hello"]);
    assert_ne!(read(&marker(&f, "hello")), first);
    // An unrestricted plugin is pushed with a loud warning.
    push_upload(&f, dir.path(), "hello", "v2!", Some(("hello", true)));
    let r = ok(&f, &["sdk", "push", "hello"]);
    assert!(r.err.contains("WARNING: hello declares platform.unrestricted"), "{}", r.err);
    assert_eq!(read(&plugin_jar(&f, "hello")), "v2!");
}

// ---- sdk remove ----

#[test]
fn sdk_remove_and_data_clear_work_after_the_sdk_is_gone() {
    let (f, dir) = (common::fixture(), tempfile::tempdir().unwrap());
    with_sdk(&f, dir.path());
    pushed(&f, "hello");
    write(&p(&f, mibsdk::GRANTS), b"hello=vehicle.read\n");
    write(&data(&f, "hello"), b"x=1\n");
    write(&p(&f, mibsdk::LEDGER), b"k=hello|PERSISTENT|a|b|A\n");
    ok(&f, &["remove", "mibsdk", "--force"]);
    assert!(!installed_pkg(&f, "mibsdk"));
    assert!(plugin_jar(&f, "hello").exists() && data(&f, "hello").exists());
    // No runtime to answer or to restore the ledger: neither waited for (the wait would be 30 s).
    let t = Instant::now();
    let o = Command::new(env!("CARGO_BIN_EXE_mibpkg"))
        .arg("--root")
        .arg(f.root())
        .args(["--no-supervisor", "sdk", "remove", "hello"])
        .env("MIBPKG_SDK_REMOVE_WAIT_S", "30")
        .output()
        .unwrap();
    let out = String::from_utf8_lossy(&o.stdout);
    assert!(o.status.success(), "{out} {}", String::from_utf8_lossy(&o.stderr));
    assert!(t.elapsed() < Duration::from_secs(10), "waited {:?}", t.elapsed());
    assert!(out.starts_with("removed hello: plugins/hello.jar and its grants line; the SDK is neither installed nor running, so no reload was requested"), "{out}");
    assert!(out.contains("WARNING: these overrides of hello are not restored yet"), "{out}");
    assert!(!plugin_jar(&f, "hello").exists());
    assert!(!marker(&f, "hello").exists(), "no reload marker without an SDK");
    assert_eq!(grants_text(&f), "");
    assert!(data(&f, "hello").exists(), "data kept without --purge");
    let r = ok(&f, &["sdk", "data-clear", "hello"]);
    assert!(r.out.starts_with("cleared the data of hello"), "{}", r.out);
    assert!(!data(&f, "hello").exists());
}

#[test]
fn sdk_grant_accepts_any_id_the_runtime_accepts() {
    let (f, dir) = (common::fixture(), tempfile::tempdir().unwrap());
    with_sdk(&f, dir.path());
    pushed(&f, "Foo.Bar");
    let r = ok(&f, &["sdk", "grant", "Foo.Bar", "network"]);
    assert!(r.out.starts_with("granted Foo.Bar: network\n"), "{}", r.out);
    assert_eq!(grants_text(&f), "Foo.Bar=network\n");
    for bad in ["../x", ".x", "a b", ""] {
        failed(&f, &["sdk", "grant", bad, "network"]);
    }
    assert_eq!(grants_text(&f), "Foo.Bar=network\n");
}

#[test]
fn sdk_remove_refuses_a_package_whose_manifest_cannot_be_read() {
    let (f, dir) = (common::fixture(), tempfile::tempdir().unwrap());
    with_sdk(&f, dir.path());
    install_plugin(&f, dir.path(), "bike", &[]);
    let m = install::current_dir(&f.layout, "bike").unwrap().join("manifest.toml");
    fs::write(&m, "not toml [").unwrap();
    let r = failed(&f, &["sdk", "remove", "bike"]);
    assert!(r.err.contains("the manifest of the bike package cannot be read"), "{}", r.err);
    assert!(r.err.contains("mibpkg remove bike, then: mibpkg sdk remove bike"), "{}", r.err);
    assert!(installed_pkg(&f, "bike") && plugin_jar(&f, "bike").exists(), "nothing changed");
    // The way it names works.
    ok(&f, &["remove", "bike"]);
    ok(&f, &["sdk", "remove", "bike"]);
    assert!(!plugin_jar(&f, "bike").exists());
}

#[test]
fn sdk_surplus_or_missing_arguments_exit_2() {
    let (f, dir) = (common::fixture(), tempfile::tempdir().unwrap());
    with_sdk(&f, dir.path());
    pushed(&f, "hello");
    for bad in [
        &["sdk", "status", "x"][..],
        &["sdk", "grant", "hello", "network", "x"],
        &["sdk", "grant", "hello"],
        &["sdk", "push", "hello", "x"],
        &["sdk", "push"],
        &["sdk", "remove", "hello", "x"],
        &["sdk", "remove"],
        &["sdk", "safe-mode", "on", "x"],
        &["sdk", "boot-state", "set-wrap", "1", "2"],
        &["sdk", "data-clear", "hello", "x"],
        &["sdk", "data-clear"],
    ] {
        assert_eq!(mibpkg(&f, bad).code, 2, "{bad:?}");
    }
    assert!(plugin_jar(&f, "hello").exists());
}

#[test]
fn sdk_remove_pushed_id_and_package_id() {
    let (f, dir) = (common::fixture(), tempfile::tempdir().unwrap());
    with_sdk(&f, dir.path());
    pushed(&f, "hello");
    write(&p(&f, mibsdk::GRANTS), b"hello=vehicle.read\n");
    write(&data(&f, "hello"), b"x=1\n");
    let r = ok(&f, &["sdk", "remove", "hello", "--purge-data"]);
    assert!(r.out.starts_with("removed hello: plugins/hello.jar and its grants line; reload requested"), "{}", r.out);
    assert!(r.out.contains("cleared the data of hello"), "{}", r.out);
    assert!(!plugin_jar(&f, "hello").exists() && !data(&f, "hello").exists());
    assert!(!grants_text(&f).contains("hello="));
    let r = failed(&f, &["sdk", "remove", "nothere"]);
    assert!(r.err.contains("nothere is not installed"), "{}", r.err);
    // A package goes through the package removal, data kept without --purge.
    install_plugin(&f, dir.path(), "bike", &["vehicle.read"]);
    write(&data(&f, "bike"), b"y=1\n");
    let r = ok(&f, &["sdk", "remove", "bike"]);
    assert!(r.out.contains("removed bike"), "{}", r.out);
    assert!(!installed_pkg(&f, "bike") && !plugin_jar(&f, "bike").exists());
    assert!(data(&f, "bike").exists());
    install_plugin(&f, dir.path(), "bike", &["vehicle.read"]);
    ok(&f, &["sdk", "remove", "bike", "--purge"]);
    assert!(!data(&f, "bike").exists());
}

// ---- sdk safe-mode, boot-state, data-clear ----

#[test]
fn sdk_safe_mode_on_off() {
    // was: test_off_writes_boot_guard_reset_content
    let (f, dir) = (common::fixture(), tempfile::tempdir().unwrap());
    with_sdk(&f, dir.path());
    let safe = p(&f, &format!("{}/SAFE_MODE", mibsdk::BASE));
    let r = ok(&f, &["sdk", "safe-mode", "on"]);
    assert_eq!(r.out, "safe mode marker set; it takes effect on the next HMI restart\n");
    assert!(safe.exists());
    write(&boot_state(&f), b"unhealthyBoots=3\nlastShutdownClean=false\nwrapBoots=3\n");
    let r = ok(&f, &["sdk", "safe-mode", "off"]);
    assert!(!safe.exists());
    // Must equal BootGuard.reset()'s file (BootGuardTest.resetLeavesSafeMode).
    assert_eq!(read(&boot_state(&f)), "unhealthyBoots=0\nlastShutdownClean=true\nwrapBoots=0\n");
    assert!(!p(&f, &format!("{}/boot.state", mibsdk::BASE)).exists());
    assert!(r.out.starts_with("safe mode off: SAFE_MODE removed and boot.state reset"), "{}", r.out);
    assert!(r.out.contains("restart promptly"), "{}", r.out);
    // Off again: nothing to remove is fine.
    ok(&f, &["sdk", "safe-mode", "off"]);
    assert_eq!(failed(&f, &["sdk", "safe-mode", "maybe"]).code, 2);
}

#[test]
fn sdk_boot_state_set_wrap() {
    // was: test_set_wrap_keeps_other_keys
    let (f, dir) = (common::fixture(), tempfile::tempdir().unwrap());
    with_sdk(&f, dir.path());
    write(&boot_state(&f), b"unhealthyBoots=2\nlastShutdownClean=false\nwrapBoots=0\nextra=1\n");
    let r = ok(&f, &["sdk", "boot-state", "set-wrap", "3"]);
    assert_eq!(r.out, "boot.state: wrapBoots=3 (other keys kept); it is read on the next HMI start\n");
    assert_eq!(read(&boot_state(&f)), "unhealthyBoots=2\nlastShutdownClean=false\nwrapBoots=3\nextra=1\n");
    // CRs are stripped; a file without the key gets it appended.
    write(&boot_state(&f), b"unhealthyBoots=1\r\nlastShutdownClean=true");
    ok(&f, &["sdk", "boot-state", "set-wrap", "7"]);
    assert_eq!(read(&boot_state(&f)), "unhealthyBoots=1\nlastShutdownClean=true\nwrapBoots=7\n");
}

#[test]
fn sdk_boot_state_set_wrap_without_a_file_uses_the_reset_keys() {
    // was: test_set_wrap_without_a_file_uses_the_reset_keys
    let (f, dir) = (common::fixture(), tempfile::tempdir().unwrap());
    with_sdk(&f, dir.path());
    let _ = fs::remove_file(boot_state(&f));
    ok(&f, &["sdk", "boot-state", "set-wrap", "2"]);
    assert_eq!(read(&boot_state(&f)), "unhealthyBoots=0\nlastShutdownClean=true\nwrapBoots=2\n");
}

#[test]
fn sdk_boot_state_set_wrap_rejects_a_bad_count() {
    // was: test_set_wrap_rejects_a_bad_count
    let (f, dir) = (common::fixture(), tempfile::tempdir().unwrap());
    with_sdk(&f, dir.path());
    let _ = fs::remove_file(boot_state(&f));
    for bad in [&["sdk", "boot-state", "set-wrap", "x"][..], &["sdk", "boot-state", "set-wrap"], &["sdk", "boot-state", "set-wrap", "1234567"], &["sdk", "boot-state", "set-wrap", "-1"], &["sdk", "boot-state"]] {
        assert_eq!(mibpkg(&f, bad).code, 2, "{bad:?}");
    }
    assert!(!boot_state(&f).exists());
}

#[test]
fn sdk_data_clear() {
    // was: test_removes_only_that_plugins_data
    let (f, dir) = (common::fixture(), tempfile::tempdir().unwrap());
    with_sdk(&f, dir.path());
    let d = p(&f, &format!("{}/data", mibsdk::PERSIST));
    write(&d.join("hello.properties"), b"x=1\n");
    write(&d.join("hello.properties.tmp"), b"x=");
    write(&d.join("hello2.properties"), b"y=1\n");
    write(&p(&f, mibsdk::LEDGER), b"k=hello|PERSISTENT|a|b|A\n");
    write(&boot_state(&f), b"unhealthyBoots=1\n");
    let r = ok(&f, &["sdk", "data-clear", "hello"]);
    assert!(r.out.starts_with("cleared the data of hello (/mnt/persist/mibsdk/data/hello.properties)\nRestart the HMI now"), "{}", r.out);
    let mut left: Vec<String> = fs::read_dir(&d).unwrap().map(|e| e.unwrap().file_name().to_string_lossy().into_owned()).collect();
    left.sort();
    assert_eq!(left, vec!["hello2.properties".to_string()]);
    assert_eq!(read(&p(&f, mibsdk::LEDGER)), "k=hello|PERSISTENT|a|b|A\n");
    assert_eq!(read(&boot_state(&f)), "unhealthyBoots=1\n");
    let r = ok(&f, &["sdk", "data-clear", "hello"]); // nothing left: still fine
    assert_eq!(r.out, "no data for hello in /mnt/persist/mibsdk/data; nothing to clear\n");
}

#[test]
fn sdk_data_clear_rejects_a_bad_id() {
    // was: test_rejects_a_bad_id
    let (f, dir) = (common::fixture(), tempfile::tempdir().unwrap());
    with_sdk(&f, dir.path());
    write(&data(&f, "x"), b"x=1\n");
    for bad in ["../x", "*", ""] {
        failed(&f, &["sdk", "data-clear", bad]);
    }
    failed(&f, &["sdk", "data-clear"]);
    assert!(data(&f, "x").exists());
}

#[test]
fn sdk_without_verb_exits_2() {
    let f = common::fixture();
    let r = mibpkg(&f, &["sdk"]);
    assert_eq!(r.code, 2);
    assert!(r.err.contains("mibpkg sdk status"), "{}", r.err);
    assert!(r.err.contains("mibpkg sdk remove <id> [--purge|--purge-data] [--force]"), "{}", r.err);
    assert_eq!(mibpkg(&f, &["sdk", "frobnicate"]).code, 2);
}

// ---- the web's view and verbs ----

#[test]
fn gem_sdk_grant_is_the_webs_revoke_path() {
    let (f, dir) = (common::fixture(), tempfile::tempdir().unwrap());
    with_sdk(&f, dir.path());
    install_plugin(&f, dir.path(), "hello", &["vehicle.read", "network"]);
    running(&f);
    let rt = fake_runtime(&f, "hello", "BLOCKED_PERMISSIONS missing permissions: network");
    let r = ok(&f, &["gem", "sdk", "grant", "hello", "vehicle.read"]);
    assert!(rt.join().unwrap(), "no reload requested");
    assert!(r.out.contains("OK: granted hello: vehicle.read"), "{}", r.out);
    assert_eq!(grants_text(&f), "hello=vehicle.read\n");
    // Only grant goes through gem; a malformed one is FAILED, nothing changes.
    for bad in [&["gem", "sdk", "remove", "hello"][..], &["gem", "sdk", "grant", "hello", "a b"], &["gem", "sdk", "grant", "hello"]] {
        let r = mibpkg(&f, bad);
        assert!(r.out.contains("FAILED: "), "{bad:?}: {}", r.out);
    }
    assert_eq!(grants_text(&f), "hello=vehicle.read\n");
    assert!(installed_pkg(&f, "hello"));
}

#[test]
fn catalog_shows_plugin_permissions_in_plain_words_granted_and_blocked() {
    let (f, dir) = (common::fixture(), tempfile::tempdir().unwrap());
    with_sdk(&f, dir.path());
    install_plugin(&f, dir.path(), "hello", &["vehicle.read", "platform.unrestricted"]);
    write(&p(&f, mibsdk::GRANTS), b"hello=vehicle.read\n");
    let views = mibpkg::catalog::build(&f.layout, &mibpkg::unit::detect(&f.layout));
    let hello = views.iter().find(|v| v.name == "hello").unwrap();
    let inst = hello.installed.as_ref().unwrap();
    assert_eq!(inst.kind.as_deref(), Some("plugin"));
    let json = serde_json::to_value(&inst.permissions).unwrap();
    assert_eq!(
        json,
        serde_json::json!([
            {"id": "vehicle.read", "text": "Read vehicle state: ignition, gear, parking sensors", "loud": false, "granted": true},
            {"id": "platform.unrestricted", "text": "Unrestricted: may use reflection and stock HMI classes; the SDK cannot contain it", "loud": true, "granted": false},
        ])
    );
    assert_eq!(inst.blocked, ["platform.unrestricted"]);
    let sdk = views.iter().find(|v| v.name == "mibsdk").unwrap().installed.as_ref().unwrap();
    assert_eq!(sdk.kind.as_deref(), Some("sdk"));
    assert!(sdk.permissions.is_empty() && sdk.blocked.is_empty());
}

fn install_version(f: &common::Fixture, dir: &Path, id: &str, ver: &str, perms: &[&str]) {
    let sys = RecordingSys::default();
    let mut ctx = Ctx::new(f.layout.clone(), &sys);
    ctx.approved = Some(perms.iter().map(|s| s.to_string()).collect());
    install_src(&ctx, &plugin_src(dir, id, ver, perms)).unwrap();
}

#[test]
fn rollback_takes_approve_for_permissions_only_the_previous_version_declares() {
    let (f, dir) = (common::fixture(), tempfile::tempdir().unwrap());
    with_sdk(&f, dir.path());
    install_version(&f, dir.path(), "hello", "1.0.0", &["vehicle.read", "network"]);
    install_version(&f, dir.path(), "hello", "1.1.0", &["vehicle.read"]);
    assert_eq!(grants_text(&f), "hello=vehicle.read\n");
    let r = mibpkg(&f, &["rollback", "hello"]);
    assert_eq!(r.code, 3, "{:?}", (&r.out, &r.err));
    assert!(r.out.ends_with("approve with: --approve=network\n"), "{}", r.out);
    assert_eq!(State::load(&f.layout).unwrap().packages["hello"].version, "1.1.0");
    ok(&f, &["rollback", "hello", "--approve=network"]);
    assert_eq!(State::load(&f.layout).unwrap().packages["hello"].version, "1.0.0");
    assert_eq!(grants_text(&f), "hello=network,vehicle.read\n");
    // Through the gem arm too (the web's path): 1.2.0 drops network again; its previous is 1.0.0.
    install_version(&f, dir.path(), "hello", "1.2.0", &["vehicle.read"]);
    assert_eq!(mibpkg(&f, &["gem", "rollback", "hello"]).code, 0);
    assert!(State::load(&f.layout).unwrap().results["hello"].starts_with("FAILED: approve these permissions"));
    assert_eq!(mibpkg(&f, &["gem", "rollback", "hello", "--approve=network"]).code, 0);
    let res = State::load(&f.layout).unwrap().results["hello"].clone();
    assert!(res.starts_with("OK: "), "{res}");
    assert_eq!(State::load(&f.layout).unwrap().packages["hello"].version, "1.0.0");
}

#[test]
fn sdk_grant_on_a_package_grants_only_what_its_plugin_declares() {
    let (f, dir) = (common::fixture(), tempfile::tempdir().unwrap());
    with_sdk(&f, dir.path());
    install_plugin(&f, dir.path(), "hello", &["vehicle.read"]);
    let before = grants_text(&f);
    let r = failed(&f, &["sdk", "grant", "hello", "vehicle.read,network"]);
    assert!(r.err.contains("hello does not declare network"), "{}", r.err);
    let r = failed(&f, &["sdk", "grant", "mibsdk", ""]);
    assert!(r.err.contains("mibsdk is not a plugin package"), "{}", r.err);
    let sys = RecordingSys::default();
    install_src(&Ctx::new(f.layout.clone(), &sys), &common::pkg_src(dir.path(), "plain", "1.0.0", "sleep 1", "")).unwrap();
    let r = failed(&f, &["sdk", "grant", "plain", "network"]);
    assert!(r.err.contains("plain is not a plugin package"), "{}", r.err);
    assert_eq!(grants_text(&f), before);
    // A pushed jar has no manifest: the installed SDK's catalog decides.
    pushed(&f, "pushy");
    let r = failed(&f, &["sdk", "grant", "pushy", "made.up"]);
    assert!(r.err.contains("made.up"), "{}", r.err);
    assert_eq!(grants_text(&f), before);
}

#[test]
fn catalog_shows_previous_permissions_unknown_as_loud_and_unreadable_grants() {
    let (f, dir) = (common::fixture(), tempfile::tempdir().unwrap());
    with_sdk(&f, dir.path());
    install_version(&f, dir.path(), "hello", "1.0.0", &["vehicle.read", "network"]);
    install_version(&f, dir.path(), "hello", "1.1.0", &["vehicle.read"]);
    let unit = mibpkg::unit::detect(&f.layout);
    let view = |name: &str| mibpkg::catalog::build(&f.layout, &unit).into_iter().find(|v| v.name == name).unwrap().installed.unwrap();
    let inst = view("hello");
    let prev: Vec<&str> = inst.previous_permissions.iter().map(|p| p.id.as_str()).collect();
    assert_eq!(prev, ["vehicle.read", "network"]);
    assert_eq!(inst.grants_error, None);
    // Without a catalog every permission is unknown, and unknown is loud.
    fs::remove_file(p(&f, mibsdk::PERMISSIONS)).unwrap();
    let inst = view("hello");
    assert!(inst.permissions[0].loud && inst.permissions[0].text.contains("not known"), "{:?}", inst.permissions);
    // An unreadable grants file is an error, not "nothing granted".
    fs::remove_file(p(&f, mibsdk::GRANTS)).unwrap();
    fs::create_dir(p(&f, mibsdk::GRANTS)).unwrap();
    let inst = view("hello");
    assert!(inst.grants_error.as_deref().is_some_and(|e| e.contains("grants.properties")), "{:?}", inst.grants_error);
    assert!(inst.permissions.iter().all(|p| p.granted.is_none()), "{:?}", inst.permissions);
    assert!(inst.blocked.is_empty());
}

// ---- final review fixes: a runtime up while a reboot is pending (I2) ----

/// The runtime started this boot and a reboot is pending (an SDK update waits for it): the
/// running runtime still acts on reload markers.
fn up_with_reboot_pending(f: &common::Fixture) {
    assert!(mibsdk::reboot_flag(&f.layout).exists());
    write(&p(f, mibsdk::LOG), b"100 STATUS mibsdk runtime started adapter=hqc safeMode=false\n");
}

#[test]
fn sdk_grant_revoke_during_a_pending_reboot_reloads_the_running_plugin() {
    let (f, dir) = (common::fixture(), tempfile::tempdir().unwrap());
    with_sdk(&f, dir.path());
    install_plugin(&f, dir.path(), "hello", &["vehicle.read"]);
    up_with_reboot_pending(&f);
    let rt = fake_runtime(&f, "hello", "BLOCKED_PERMISSIONS missing permissions: vehicle.read");
    let r = ok(&f, &["sdk", "grant", "hello", ""]);
    assert!(rt.join().unwrap(), "no reload requested");
    assert!(is_token(&read(&marker(&f, "hello"))));
    assert_eq!(r.out, "granted hello: <none>\nloaded: BLOCKED_PERMISSIONS missing permissions: vehicle.read\n");
}

#[test]
fn plugin_update_during_a_pending_reboot_reloads_the_running_plugin() {
    let (f, dir) = (common::fixture(), tempfile::tempdir().unwrap());
    with_sdk(&f, dir.path());
    install_plugin(&f, dir.path(), "hello", &["vehicle.read", "network"]);
    up_with_reboot_pending(&f);
    let rt = fake_runtime(&f, "hello", "ENABLED ok");
    let sys = RecordingSys::default();
    let ctx = Ctx::new(f.layout.clone(), &sys);
    let msg = install_src(&ctx, &plugin_src(dir.path(), "hello", "1.1.0", &["vehicle.read"])).unwrap();
    assert!(rt.join().unwrap(), "no reload requested");
    assert!(is_token(&read(&marker(&f, "hello"))));
    assert!(msg.ends_with("loaded: ENABLED ok"), "{msg}");
}

#[test]
fn sdk_push_during_a_pending_reboot_waits_for_the_running_runtime() {
    let (f, dir) = (common::fixture(), tempfile::tempdir().unwrap());
    with_sdk(&f, dir.path());
    up_with_reboot_pending(&f);
    push_upload(&f, dir.path(), "hello", "v1", Some(("hello", false)));
    let rt = fake_runtime(&f, "hello", "ENABLED ok");
    let r = ok(&f, &["sdk", "push", "hello"]);
    assert!(rt.join().unwrap(), "no reload requested");
    assert_eq!(r.out, "pushed hello; reload requested\nloaded: ENABLED ok\n");
}

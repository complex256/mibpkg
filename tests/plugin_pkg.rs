//! `[plugin]` packages: the install checks, permission approval, deploy, hot-load through the
//! reload marker, and the one removal path (`plugin_pkg::remove_id`) for packages and pushed ids.
//! Ports `installer/test_mibsdk_dev.py`'s RemoveTest from the SDK repo (each port carries
//! `// was: <python test>`; they drive `remove_id` on a pushed id).
mod common;
use mibpkg::archive::{self, Package};
use mibpkg::ctx::Ctx;
use mibpkg::install;
use mibpkg::manifest::Manifest;
use mibpkg::mibsdk::plugin_pkg::{self, NeedsApproval};
use mibpkg::mibsdk::{self, ack, grants};
use mibpkg::state::State;
use mibpkg::sys::RecordingSys;
use std::collections::BTreeSet;
use std::fs;
use std::io::Write;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::thread;
use std::time::{Duration, Instant};

const FIX: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/mibsdk");
const SDK_MANIFEST: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/manifests/sdk-mibsdk.toml");
const SDK_JARS: [&str; 3] = ["mibsdk-runtime.jar", "mibsdk-adapter-hqc.jar", "mibsdk-rt-retrotranslator.jar"];
const LINE: &str = "parking.soundDirections=hello|PERSISTENT|F1R1L1X1|F1R0L1X1|R";
const NO_WAIT: Duration = Duration::ZERO;

// ---- fixtures ----

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

fn grants_text(f: &common::Fixture) -> String {
    fs::read_to_string(p(f, mibsdk::GRANTS)).unwrap_or_default()
}

fn granted(f: &common::Fixture, id: &str) -> Option<BTreeSet<String>> {
    grants::read(&f.layout).unwrap().get(id).cloned()
}

fn set(perms: &[&str]) -> BTreeSet<String> {
    perms.iter().map(|s| s.to_string()).collect()
}

/// A check stamp in CheckStamp's format 1.
fn write_stamp(dir: &Path, name: &str, kind: &str, plugin: Option<(&str, bool)>, jars: &[&str]) {
    let mut lines = vec!["format=1".to_string(), format!("kind={kind}"), "checks=classVersion48,distLinkCheck".to_string()];
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

/// The unit side the SDK install needs (as tests/sdk_pkg.rs `sdk_unit`).
fn sdk_unit(f: &common::Fixture) {
    write(&p(f, mibsdk::BUNDLES), &fs::read(Path::new(FIX).join("bundles_dsitracer_enabled.properties")).unwrap());
    fs::set_permissions(p(f, mibsdk::BUNDLES), fs::Permissions::from_mode(0o777)).unwrap();
    write(&p(f, mibsdk::MARKER), b"");
    write(&p(f, mibsdk::TRACER_JAR), b"tracer");
    fs::create_dir_all(p(f, mibsdk::JARS)).unwrap();
    write(&p(f, mibsdk::VERSION_INFO), &fs::read(Path::new(FIX).join("version_info.txt")).unwrap());
    fs::create_dir_all(p(f, "/mnt/persist")).unwrap();
}

fn sdk_src(dir: &Path, version: &str) -> PathBuf {
    let s = dir.join(format!("src-mibsdk-{version}"));
    fs::create_dir_all(&s).unwrap();
    let text = read(Path::new(SDK_MANIFEST)).replace("version = \"0.1.0\"", &format!("version = \"{version}\""));
    fs::write(s.join("manifest.toml"), text).unwrap();
    for j in SDK_JARS {
        fs::write(s.join(j), format!("{j} {version}\n")).unwrap();
    }
    write_stamp(&s, "mibsdk-unit.stamp", "unit", None, &SDK_JARS);
    fs::copy(Path::new(FIX).join("permissions.json"), s.join("permissions.json")).unwrap();
    s
}

fn package(src: &Path) -> Package {
    let mpk = src.with_extension("mpk");
    archive::pack(src, &mpk).unwrap();
    Package::read(&mpk).unwrap()
}

fn install_src(ctx: &Ctx, src: &Path) -> anyhow::Result<String> {
    install::install_package(ctx, &[], &package(src), "test", "Test")
}

/// The unit with the SDK package installed (not running: it waits for the reboot).
fn with_sdk(f: &common::Fixture, dir: &Path) {
    sdk_unit(f);
    let sys = RecordingSys::default();
    let ctx = Ctx::new(f.layout.clone(), &sys);
    install_src(&ctx, &sdk_src(dir, "0.1.0")).unwrap();
}

/// The SDK runtime started this boot: no reboot pending, and its log exists.
fn running(f: &common::Fixture) {
    let _ = fs::remove_file(mibsdk::reboot_flag(&f.layout));
    write(&p(f, mibsdk::LOG), b"100 STATUS mibsdk runtime started adapter=hqc safeMode=false\n");
}

/// A `[plugin]` package source: manifest, jar and a valid `<id>.jar.stamp`.
fn plugin_src(dir: &Path, id: &str, ver: &str, api: u32, perms: &[&str]) -> PathBuf {
    plugin_src_with(dir, id, ver, api, perms, id)
}

fn plugin_src_with(dir: &Path, id: &str, ver: &str, api: u32, perms: &[&str], stamp_id: &str) -> PathBuf {
    let s = dir.join(format!("src-{id}-{ver}"));
    fs::create_dir_all(&s).unwrap();
    let list: Vec<String> = perms.iter().map(|p| format!("{p:?}")).collect();
    fs::write(
        s.join("manifest.toml"),
        format!(
            "[package]\nname = \"{id}\"\nversion = \"{ver}\"\n\n[plugin]\njar = \"{id}.jar\"\napi = {api}\npermissions = [{}]\n",
            list.join(", ")
        ),
    )
    .unwrap();
    fs::write(s.join(format!("{id}.jar")), format!("{id} {ver} jar\n")).unwrap();
    let unrestricted = perms.contains(&"platform.unrestricted");
    write_stamp(&s, &format!("{id}.jar.stamp"), "plugin", Some((stamp_id, unrestricted)), &[&format!("{id}.jar")]);
    s
}

/// A jar `mibsdk-dev push` left in BASE/plugins (no package).
fn pushed(f: &common::Fixture, id: &str) {
    write(&plugin_jar(f, id), format!("pushed {id}\n").as_bytes());
}

/// `mibsdk-dev grant`: the grants line of a pushed plugin.
fn grant_line(f: &common::Fixture, id: &str, perms: &str) {
    let mut t = grants_text(f);
    t.push_str(&format!("{id}={perms}\n"));
    write(&p(f, mibsdk::GRANTS), t.as_bytes());
}

fn ledger(f: &common::Fixture, text: &str) {
    write(&p(f, mibsdk::LEDGER), text.as_bytes());
}

fn append_log(path: &Path, text: &str) {
    let mut fh = fs::OpenOptions::new().append(true).create(true).open(path).unwrap();
    fh.write_all(text.as_bytes()).unwrap();
}

/// The SDK runtime's side of a reload: once `plugins/<id>.reload` changes, log the dev channel's
/// line and then `STATUS <id> <status>`. Spawned before the call.
fn fake_runtime(f: &common::Fixture, id: &str, status: &str) -> thread::JoinHandle<bool> {
    let (m, log, id, status) = (marker(f, id), p(f, mibsdk::LOG), id.to_string(), status.to_string());
    let before = fs::read(&m).ok();
    thread::spawn(move || {
        let end = Instant::now() + Duration::from_secs(10);
        while Instant::now() < end {
            let now = fs::read(&m).ok();
            if now.is_some() && now != before {
                append_log(&log, &format!("200 INFO dev dev channel: reload requested for {id}\n"));
                append_log(&log, &format!("201 STATUS {id} DISABLED reloading\n"));
                append_log(&log, &format!("202 STATUS {id} {status}\n"));
                return true;
            }
            thread::sleep(Duration::from_millis(20));
        }
        false
    })
}

fn approving(ctx: &mut Ctx, perms: &[&str]) {
    ctx.approved = Some(perms.iter().map(|s| s.to_string()).collect());
}

fn needs_approval(r: anyhow::Result<String>) -> NeedsApproval {
    let e = r.expect_err("expected NeedsApproval");
    let n = e.downcast_ref::<NeedsApproval>().unwrap_or_else(|| panic!("not NeedsApproval: {e:#}"));
    NeedsApproval { package: n.package.clone(), version: n.version.clone(), ask: n.ask.clone(), lines: n.lines.clone() }
}

fn err(r: anyhow::Result<String>) -> String {
    format!("{:#}", r.expect_err("expected a refusal"))
}

fn installed_pkg(f: &common::Fixture, id: &str) -> bool {
    State::load(&f.layout).unwrap().packages.contains_key(id)
}

fn is_token(s: &str) -> bool {
    let t = s.strip_suffix('\n').unwrap_or("");
    let parts: Vec<&str> = t.split('-').collect();
    parts.len() == 3 && parts.iter().all(|p| !p.is_empty() && p.bytes().all(|b| b.is_ascii_digit()))
}

// ---- check and approval ----

#[test]
fn plugin_install_without_approval_returns_needs_approval_and_changes_nothing() {
    let (f, dir) = (common::fixture(), tempfile::tempdir().unwrap());
    with_sdk(&f, dir.path());
    let grants_before = fs::read(p(&f, mibsdk::GRANTS)).ok();
    let sys = RecordingSys::default();
    let ctx = Ctx::new(f.layout.clone(), &sys);
    let r = install_src(&ctx, &plugin_src(dir.path(), "hello", "1.0.0", 2, &["vehicle.read"]));
    let n = needs_approval(r);
    assert_eq!(n.ask, vec!["vehicle.read".to_string()]);
    assert_eq!(n.lines, vec!["Read vehicle state: ignition, gear, parking sensors (vehicle.read)".to_string()]);
    assert_eq!(
        n.to_string(),
        "approve these permissions to install hello 1.0.0: Read vehicle state: ignition, gear, parking sensors (vehicle.read)"
    );
    assert!(!plugin_jar(&f, "hello").exists());
    assert_eq!(fs::read(p(&f, mibsdk::GRANTS)).ok(), grants_before);
    assert!(!installed_pkg(&f, "hello"));
    assert!(!f.layout.pkg_dir("hello").exists());
    assert!(sys.calls.borrow().is_empty(), "remounted: {:?}", sys.calls.borrow());
    // An approval that does not cover the ask is no approval.
    let mut ctx = ctx;
    approving(&mut ctx, &["network"]);
    let n = needs_approval(install_src(&ctx, &plugin_src(dir.path(), "hello", "1.0.0", 2, &["vehicle.read"])));
    assert_eq!(n.ask, vec!["vehicle.read".to_string()]);
}

#[test]
fn plugin_install_with_approval_deploys_jar_writes_grants_and_marker() {
    let (f, dir) = (common::fixture(), tempfile::tempdir().unwrap());
    with_sdk(&f, dir.path());
    running(&f);
    let sys = RecordingSys::default();
    let mut ctx = Ctx::new(f.layout.clone(), &sys);
    approving(&mut ctx, &["vehicle.read"]);
    let src = plugin_src(dir.path(), "hello", "1.0.0", 2, &["vehicle.read"]);
    let rt = fake_runtime(&f, "hello", "ENABLED ok");
    let msg = install_src(&ctx, &src).unwrap();
    assert!(rt.join().unwrap(), "the marker was never written");
    assert!(msg.starts_with("installed hello 1.0.0"), "{msg}");
    assert!(msg.contains("loaded: ENABLED ok"), "{msg}");
    assert_eq!(fs::read(plugin_jar(&f, "hello")).unwrap(), fs::read(src.join("hello.jar")).unwrap());
    assert_eq!(fs::metadata(plugin_jar(&f, "hello")).unwrap().permissions().mode() & 0o777, 0o644);
    assert_eq!(grants_text(&f), "hello=vehicle.read\n");
    assert!(is_token(&read(&marker(&f, "hello"))), "{:?}", read(&marker(&f, "hello")));
    assert!(installed_pkg(&f, "hello"));
    assert_eq!(sys.calls.borrow().last().map(String::as_str), Some("ro /mnt/app"));
}

#[test]
fn plugin_install_without_permissions_needs_no_approval() {
    let (f, dir) = (common::fixture(), tempfile::tempdir().unwrap());
    with_sdk(&f, dir.path());
    let sys = RecordingSys::default();
    let ctx = Ctx::new(f.layout.clone(), &sys);
    let msg = install_src(&ctx, &plugin_src(dir.path(), "hello", "1.0.0", 2, &[])).unwrap();
    assert!(msg.starts_with("installed hello 1.0.0"), "{msg}");
    assert!(plugin_jar(&f, "hello").is_file());
    assert_eq!(granted(&f, "hello"), Some(BTreeSet::new()));
}

// Review Focus 2.
#[test]
fn plugin_update_asks_only_new_permissions_and_keeps_revocations() {
    let (f, dir) = (common::fixture(), tempfile::tempdir().unwrap());
    with_sdk(&f, dir.path());
    let sys = RecordingSys::default();
    let mut ctx = Ctx::new(f.layout.clone(), &sys);
    approving(&mut ctx, &["vehicle.read", "ui.slot.carmenu"]);
    install_src(&ctx, &plugin_src(dir.path(), "hello", "1.0.0", 2, &["vehicle.read", "ui.slot.carmenu"])).unwrap();
    // The user revoked vehicle.read.
    write(&p(&f, mibsdk::GRANTS), b"# grants\nhello=ui.slot.carmenu\nother=network\n");
    let v2 = plugin_src(dir.path(), "hello", "1.1.0", 2, &["vehicle.read", "ui.slot.carmenu", "network"]);
    ctx.approved = None;
    let n = needs_approval(install_src(&ctx, &v2));
    assert_eq!(n.ask, vec!["network".to_string()]);
    assert_eq!(install::current_version(&f.layout, "hello").as_deref(), Some("1.0.0"));
    approving(&mut ctx, &["network"]);
    let msg = install_src(&ctx, &v2).unwrap();
    assert!(msg.starts_with("updated hello 1.0.0 -> 1.1.0"), "{msg}");
    assert_eq!(granted(&f, "hello"), Some(set(&["ui.slot.carmenu", "network"])));
    assert_eq!(granted(&f, "other"), Some(set(&["network"])));
    assert!(grants_text(&f).starts_with("# grants\n"), "{}", grants_text(&f));
    assert!(
        msg.contains("hello stays blocked until granted: vehicle.read; grant with: mibpkg sdk grant hello vehicle.read,ui.slot.carmenu,network"),
        "{msg}"
    );
}

#[test]
fn plugin_install_accepts_api_1_and_2_on_sdk_1_to_2() {
    let (f, dir) = (common::fixture(), tempfile::tempdir().unwrap());
    with_sdk(&f, dir.path());
    let sys = RecordingSys::default();
    let ctx = Ctx::new(f.layout.clone(), &sys);
    install_src(&ctx, &plugin_src(dir.path(), "one", "1.0.0", 1, &[])).unwrap();
    install_src(&ctx, &plugin_src(dir.path(), "two", "1.0.0", 2, &[])).unwrap();
    assert!(plugin_jar(&f, "one").is_file() && plugin_jar(&f, "two").is_file());
}

#[test]
fn plugin_install_refused_on_api_out_of_range() {
    let (f, dir) = (common::fixture(), tempfile::tempdir().unwrap());
    with_sdk(&f, dir.path());
    let sys = RecordingSys::default();
    let ctx = Ctx::new(f.layout.clone(), &sys);
    let e = err(install_src(&ctx, &plugin_src(dir.path(), "hello", "1.0.0", 3, &[])));
    assert_eq!(e, "hello needs plugin api 3; the installed SDK supports 1..2");
    assert!(!plugin_jar(&f, "hello").exists() && !installed_pkg(&f, "hello"));
}

#[test]
fn plugin_install_refused_without_sdk() {
    let (f, dir) = (common::fixture(), tempfile::tempdir().unwrap());
    sdk_unit(&f);
    let sys = RecordingSys::default();
    let ctx = Ctx::new(f.layout.clone(), &sys);
    let e = err(install_src(&ctx, &plugin_src(dir.path(), "hello", "1.0.0", 2, &[])));
    assert!(e.contains("install the mibsdk package first"), "{e}");
    assert!(!installed_pkg(&f, "hello"));
    assert!(sys.calls.borrow().is_empty());
}

#[test]
fn plugin_install_refused_on_unsupported_hmi() {
    let (f, dir) = (common::fixture(), tempfile::tempdir().unwrap());
    with_sdk(&f, dir.path());
    write(&p(&f, mibsdk::VERSION_INFO), b"HMI = HMI_AU_MIB2_OTHER\n");
    let sys = RecordingSys::default();
    let ctx = Ctx::new(f.layout.clone(), &sys);
    let e = err(install_src(&ctx, &plugin_src(dir.path(), "hello", "1.0.0", 2, &[])));
    assert_eq!(e, "the SDK does not support this firmware (HMI_AU_MIB2_OTHER), so no plugin would load");
    assert!(!installed_pkg(&f, "hello"));
}

#[test]
fn plugin_install_refused_on_permission_unknown_to_sdk() {
    let (f, dir) = (common::fixture(), tempfile::tempdir().unwrap());
    with_sdk(&f, dir.path());
    let sys = RecordingSys::default();
    let mut ctx = Ctx::new(f.layout.clone(), &sys);
    approving(&mut ctx, &["vehicle.read", "teleport"]);
    let e = err(install_src(&ctx, &plugin_src(dir.path(), "hello", "1.0.0", 2, &["vehicle.read", "teleport"])));
    assert_eq!(e, "hello asks for permissions the installed SDK does not know: teleport");
    assert!(!installed_pkg(&f, "hello"));
}

/// catalog::installed Ok(None) or Err: every permission is unknown, never a skipped check.
#[test]
fn plugin_install_refused_when_the_catalog_is_missing_or_corrupt() {
    let (f, dir) = (common::fixture(), tempfile::tempdir().unwrap());
    with_sdk(&f, dir.path());
    let sys = RecordingSys::default();
    let mut ctx = Ctx::new(f.layout.clone(), &sys);
    approving(&mut ctx, &["vehicle.read"]);
    let src = plugin_src(dir.path(), "hello", "1.0.0", 2, &["vehicle.read"]);
    fs::remove_file(p(&f, mibsdk::PERMISSIONS)).unwrap();
    let e = err(install_src(&ctx, &src));
    assert!(e.starts_with("hello asks for permissions the installed SDK does not know: vehicle.read"), "{e}");
    write(&p(&f, mibsdk::PERMISSIONS), b"{not json");
    let e = err(install_src(&ctx, &src));
    assert!(e.starts_with("hello asks for permissions the installed SDK does not know: vehicle.read"), "{e}");
    assert!(e.contains("permissions.json"), "{e}");
    assert!(!installed_pkg(&f, "hello"));
    // Without permissions there is nothing to refuse.
    install_src(&ctx, &plugin_src(dir.path(), "plain", "1.0.0", 2, &[])).unwrap();
}

#[test]
fn plugin_install_refused_when_stamp_id_differs() {
    let (f, dir) = (common::fixture(), tempfile::tempdir().unwrap());
    with_sdk(&f, dir.path());
    let sys = RecordingSys::default();
    let ctx = Ctx::new(f.layout.clone(), &sys);
    let e = err(install_src(&ctx, &plugin_src_with(dir.path(), "hello", "1.0.0", 2, &[], "other")));
    assert!(e.starts_with("hello.jar has no valid check stamp (id other != hello)"), "{e}");
    assert!(!plugin_jar(&f, "hello").exists());
    assert!(granted(&f, "hello").is_none());
    assert!(!installed_pkg(&f, "hello"));
}

#[test]
fn plugin_install_warns_for_unrestricted() {
    let (f, dir) = (common::fixture(), tempfile::tempdir().unwrap());
    with_sdk(&f, dir.path());
    let sys = RecordingSys::default();
    let mut ctx = Ctx::new(f.layout.clone(), &sys);
    let src = plugin_src(dir.path(), "hello", "1.0.0", 2, &["platform.unrestricted"]);
    // The approval sheet marks it loudly.
    let n = needs_approval(install_src(&ctx, &src));
    assert!(n.lines[0].starts_with("WARNING: Unrestricted"), "{:?}", n.lines);
    approving(&mut ctx, &["platform.unrestricted"]);
    let msg = install_src(&ctx, &src).unwrap();
    assert!(
        msg.contains("WARNING: hello declares platform.unrestricted: it may use reflection, exec or exit; the SDK cannot contain it"),
        "{msg}"
    );
}

// Review Focus 5.
#[test]
fn plugin_install_takes_over_a_pushed_jar() {
    let (f, dir) = (common::fixture(), tempfile::tempdir().unwrap());
    with_sdk(&f, dir.path());
    pushed(&f, "hello");
    grant_line(&f, "hello", "vehicle.read");
    let sys = RecordingSys::default();
    let mut ctx = Ctx::new(f.layout.clone(), &sys);
    let src = plugin_src(dir.path(), "hello", "1.0.0", 2, &["vehicle.read"]);
    // Not a refusal; no package before it, so every permission is asked.
    let n = needs_approval(install_src(&ctx, &src));
    assert_eq!(n.ask, vec!["vehicle.read".to_string()]);
    assert_eq!(read(&plugin_jar(&f, "hello")), "pushed hello\n");
    approving(&mut ctx, &["vehicle.read"]);
    let msg = install_src(&ctx, &src).unwrap();
    assert!(msg.contains("replaced the pushed hello.jar"), "{msg}");
    assert_eq!(fs::read(plugin_jar(&f, "hello")).unwrap(), fs::read(src.join("hello.jar")).unwrap());
    assert!(installed_pkg(&f, "hello"));
    // The package owns the id now: the SDK uninstall names it as a plugin package.
    let e = format!("{:#}", install::remove(&ctx, "mibsdk", false).unwrap_err());
    assert!(e.starts_with("remove these plugins first: hello"), "{e}");
    // An update is no takeover.
    let msg = install_src(&ctx, &plugin_src(dir.path(), "hello", "1.1.0", 2, &["vehicle.read"])).unwrap();
    assert!(!msg.contains("replaced the pushed"), "{msg}");
}

// ---- activation ----

#[test]
fn plugin_install_when_sdk_not_running_says_next_boot() {
    let (f, dir) = (common::fixture(), tempfile::tempdir().unwrap());
    with_sdk(&f, dir.path()); // the reboot flag is still set
    let sys = RecordingSys::default();
    let ctx = Ctx::new(f.layout.clone(), &sys);
    let msg = install_src(&ctx, &plugin_src(dir.path(), "hello", "1.0.0", 2, &[])).unwrap();
    assert!(msg.ends_with("the SDK is not running; it loads at the next boot"), "{msg}");
    assert!(!marker(&f, "hello").exists());
}

#[test]
fn plugin_install_without_ack_says_not_confirmed() {
    let (f, dir) = (common::fixture(), tempfile::tempdir().unwrap());
    with_sdk(&f, dir.path());
    let sys = RecordingSys::default();
    let ctx = Ctx::new(f.layout.clone(), &sys);
    install_src(&ctx, &plugin_src(dir.path(), "hello", "1.0.0", 2, &[])).unwrap();
    running(&f);
    let t = Instant::now();
    let note = plugin_pkg::activate_with(&ctx, "hello", Duration::from_millis(300));
    assert!(t.elapsed() >= Duration::from_millis(300));
    assert_eq!(note, "reload requested; the SDK did not confirm within 0.3 s (it loads at the next HMI start)");
    assert!(is_token(&read(&marker(&f, "hello"))));
    // Each request writes a fresh token.
    let first = read(&marker(&f, "hello"));
    plugin_pkg::activate_with(&ctx, "hello", Duration::ZERO);
    assert_ne!(read(&marker(&f, "hello")), first);
}

#[test]
fn plugin_activate_holds_no_rw_guard_while_waiting() {
    let (f, dir) = (common::fixture(), tempfile::tempdir().unwrap());
    with_sdk(&f, dir.path());
    let sys = RecordingSys::default();
    let ctx = Ctx::new(f.layout.clone(), &sys);
    install_src(&ctx, &plugin_src(dir.path(), "hello", "1.0.0", 2, &[])).unwrap();
    running(&f);
    // The runtime answers only once no mibpkg process holds /mnt/app read-write.
    let (m, log, holder) =
        (marker(&f, "hello"), p(&f, mibsdk::LOG), f.layout.p("/tmp").join(format!("mibpkg.rw.app.{}", std::process::id())));
    let rt = thread::spawn(move || {
        let end = Instant::now() + Duration::from_secs(5);
        while Instant::now() < end && !m.exists() {
            thread::sleep(Duration::from_millis(10));
        }
        while Instant::now() < end {
            if !holder.exists() {
                append_log(&log, "300 INFO dev dev channel: reload requested for hello\n300 STATUS hello ENABLED ok\n");
                return true;
            }
            thread::sleep(Duration::from_millis(10));
        }
        false
    });
    let note = plugin_pkg::activate_with(&ctx, "hello", Duration::from_secs(3));
    assert!(rt.join().unwrap(), "the rw holder was kept while waiting");
    assert_eq!(note, "loaded: ENABLED ok");
    assert_eq!(sys.calls.borrow().last().map(String::as_str), Some("ro /mnt/app"));
}

const OLD: &str = "1 INFO dev dev channel: reload requested for hello\n2 STATUS hello ENABLED old\n\
                   3 INFO dev dev channel: reload requested for hello-2\n4 STATUS hello-2 FAILED x\n";

fn ok(state: &str, reason: &str) -> Option<(String, String)> {
    Some((state.to_string(), reason.to_string()))
}

#[test]
fn ack_wait_takes_the_last_status_after_the_new_reload_line() {
    let f = common::fixture();
    let log = p(&f, mibsdk::LOG);
    write(&log, OLD.as_bytes());
    let base = ack::baseline(&f.layout);
    assert_eq!(ack::wait(&f.layout, "hello", &base, None, Duration::ZERO), None);
    append_log(&log, "5 INFO dev dev channel: reload requested for hello\n6 STATUS hello DISABLED reloading\n");
    // Still reloading: keeps waiting.
    assert_eq!(ack::wait(&f.layout, "hello", &base, None, Duration::from_millis(300)), None);
    append_log(&log, "7 STATUS hello-2 ENABLED y\n8 STATUS hello BLOCKED_PERMISSIONS missing permissions: network\n");
    assert_eq!(
        ack::wait(&f.layout, "hello", &base, None, Duration::ZERO),
        ok("BLOCKED_PERMISSIONS", "missing permissions: network")
    );
    // Exact id: hell is not hello.
    assert_eq!(ack::wait(&f.layout, "hell", &base, None, Duration::ZERO), None);
}

/// Fix round 1 (Important 1): /tmp/mibsdk.log is rewritten in place, so a baseline read may be
/// empty or cut short; the old reload line must never look new.
#[test]
fn ack_an_empty_or_partial_baseline_never_acks_an_old_line() {
    let f = common::fixture();
    let log = p(&f, mibsdk::LOG);
    for partial in ["", "1 INFO dev dev channel: reload requested for hello\n2 STATUS hel"] {
        write(&log, partial.as_bytes());
        let t = Instant::now();
        let base = ack::baseline(&f.layout);
        assert!(t.elapsed() >= Duration::from_millis(200), "an unusable baseline is retried for a while");
        assert_eq!(base, ack::Baseline::unknown(), "{partial:?}");
        write(&log, OLD.as_bytes());
        assert_eq!(ack::wait(&f.layout, "hello", &base, None, Duration::from_millis(300)), None, "{partial:?}");
    }
    // A baseline that becomes whole while it is retried is used.
    write(&log, b"");
    let path = log.clone();
    let fill = thread::spawn(move || {
        thread::sleep(Duration::from_millis(100));
        fs::write(path, OLD).unwrap();
    });
    let base = ack::baseline(&f.layout);
    fill.join().unwrap();
    assert_eq!(base, ack::Baseline::of(OLD));
}

#[test]
fn ack_a_partial_read_while_waiting_is_not_taken_for_a_wrapped_ring() {
    let base = ack::Baseline::of(&format!("{OLD}9 INFO x latest\n"));
    // The anchor is cut off this (unstable) read: the old lines in it are not new.
    assert_eq!(ack::answer(OLD, "hello", &base, None, false), None);
    // Read the same twice (stable) without the anchor: the ring wrapped, every line is new.
    assert_eq!(ack::answer(OLD, "hello", &base, None, true), ok("ENABLED", "old"));
    // With the anchor, only what follows it counts, stable or not.
    let text = format!("{OLD}9 INFO x latest\n10 INFO dev dev channel: reload requested for hello\n11 STATUS hello ENABLED new\n");
    assert_eq!(ack::answer(&text, "hello", &base, None, false), ok("ENABLED", "new"));
    assert_eq!(ack::answer(&format!("{OLD}9 INFO x latest\n"), "hello", &base, None, true), None);
}

#[test]
fn ack_ring_wrap_drops_the_anchor_and_the_new_lines_count() {
    let f = common::fixture();
    let log = p(&f, mibsdk::LOG);
    write(&log, OLD.as_bytes());
    let base = ack::baseline(&f.layout);
    // The ring wrapped: every line of the baseline is gone.
    write(&log, b"50 INFO dev dev channel: reload requested for hello\n51 STATUS hello ENABLED wrapped\n");
    assert_eq!(ack::wait(&f.layout, "hello", &base, None, Duration::from_secs(1)), ok("ENABLED", "wrapped"));
}

#[test]
fn ack_matches_the_exact_token_when_the_runtime_logs_one() {
    let f = common::fixture();
    let log = p(&f, mibsdk::LOG);
    // No usable baseline at all: only the token can tell the reload apart.
    let base = ack::Baseline::unknown();
    write(
        &log,
        b"1 INFO dev dev channel: reload requested for hello token=9-9-0\n2 STATUS hello ENABLED old\n\
          3 INFO dev dev channel: reload requested for hello token=9-9-1\n4 STATUS hello DISABLED reloading\n",
    );
    assert_eq!(ack::wait(&f.layout, "hello", &base, Some("9-9-1"), Duration::ZERO), None);
    assert_eq!(ack::wait(&f.layout, "hello", &base, None, Duration::ZERO), None);
    append_log(&log, "5 STATUS hello BLOCKED_PERMISSIONS missing permissions: network\n");
    assert_eq!(
        ack::wait(&f.layout, "hello", &base, Some("9-9-1"), Duration::ZERO),
        ok("BLOCKED_PERMISSIONS", "missing permissions: network")
    );
    // Another request's token after a known anchor is not ours.
    let base = ack::Baseline::of("0 INFO x start\n");
    write(&log, b"0 INFO x start\n1 INFO dev dev channel: reload requested for hello token=1-1-1\n2 STATUS hello ENABLED theirs\n");
    assert_eq!(ack::wait(&f.layout, "hello", &base, Some("2-2-2"), Duration::ZERO), None);
}

/// The runtime logs the marker's token: activate waits for exactly its own request.
#[test]
fn plugin_activate_waits_for_its_own_token() {
    let (f, dir) = (common::fixture(), tempfile::tempdir().unwrap());
    with_sdk(&f, dir.path());
    let sys = RecordingSys::default();
    let ctx = Ctx::new(f.layout.clone(), &sys);
    install_src(&ctx, &plugin_src(dir.path(), "hello", "1.0.0", 2, &[])).unwrap();
    running(&f);
    let (m, log) = (marker(&f, "hello"), p(&f, mibsdk::LOG));
    let rt = thread::spawn(move || {
        let end = Instant::now() + Duration::from_secs(5);
        while Instant::now() < end {
            if let Ok(tok) = fs::read_to_string(&m) {
                append_log(&log, "300 INFO dev dev channel: reload requested for hello token=0-0-0\n301 STATUS hello ENABLED stale\n");
                append_log(&log, &format!("302 INFO dev dev channel: reload requested for hello token={}\n303 STATUS hello ENABLED mine\n", tok.trim()));
                return;
            }
            thread::sleep(Duration::from_millis(10));
        }
    });
    let note = plugin_pkg::activate_with(&ctx, "hello", Duration::from_secs(3));
    rt.join().unwrap();
    assert_eq!(note, "loaded: ENABLED mine");
}

// ---- removal ----

#[test]
fn plugin_remove_keeps_data_by_default_and_drops_grants_and_off_lines() {
    let (f, dir) = (common::fixture(), tempfile::tempdir().unwrap());
    with_sdk(&f, dir.path());
    let sys = RecordingSys::default();
    let mut ctx = Ctx::new(f.layout.clone(), &sys);
    approving(&mut ctx, &["vehicle.read"]);
    install_src(&ctx, &plugin_src(dir.path(), "hello", "1.0.0", 2, &["vehicle.read"])).unwrap();
    grant_line(&f, "other", "network");
    write(&data(&f, "hello"), b"x=1\n");
    write(&p(&f, mibsdk::TOGGLES), b"hello.off=true\nother.off=true\n");
    let msg = install::remove(&ctx, "hello", false).unwrap();
    assert!(
        msg.starts_with("removed hello: plugins/hello.jar and its grants line and its hello.off switch; reload requested"),
        "{msg}"
    );
    assert!(!plugin_jar(&f, "hello").exists());
    assert_eq!(grants_text(&f), "other=network\n");
    assert_eq!(read(&p(&f, mibsdk::TOGGLES)), "other.off=true\n");
    assert!(data(&f, "hello").exists());
    assert!(!installed_pkg(&f, "hello"));
    assert!(!f.layout.pkg_dir("hello").exists());
    assert!(is_token(&read(&marker(&f, "hello"))));
    // No package left: the SDK can go.
    install::remove(&ctx, "mibsdk", false).unwrap();
}

// Review Focus 4.
#[test]
fn plugin_purge_waits_for_unload_before_deleting_data() {
    let (f, dir) = (common::fixture(), tempfile::tempdir().unwrap());
    with_sdk(&f, dir.path());
    running(&f);
    let sys = RecordingSys::default();
    let ctx = Ctx::new(f.layout.clone(), &sys);
    // No confirmation: the data stays.
    pushed(&f, "quiet");
    write(&data(&f, "quiet"), b"x=1\n");
    let msg = plugin_pkg::remove_id_with(&ctx, "quiet", true, Duration::from_millis(300), NO_WAIT).unwrap();
    assert!(data(&f, "quiet").exists());
    assert!(msg.contains("data kept: the SDK did not confirm the unload; run `mibpkg sdk data-clear quiet` after a restart"), "{msg}");
    assert!(!msg.contains("cleared the data"), "{msg}");
    // Confirmed (a package, through install::remove): the data and its .tmp go.
    let rt = fake_runtime(&f, "hello", "ENABLED ok");
    install_src(&ctx, &plugin_src(dir.path(), "hello", "1.0.0", 2, &[])).unwrap();
    assert!(rt.join().unwrap());
    write(&data(&f, "hello"), b"x=1\n");
    write(&p(&f, &format!("{}/data/hello.properties.tmp", mibsdk::PERSIST)), b"x=2\n");
    let rt = fake_runtime(&f, "hello", "DISABLED plugin jar not found");
    let msg = install::remove(&ctx, "hello", true).unwrap();
    assert!(rt.join().unwrap());
    assert!(!data(&f, "hello").exists());
    assert!(!p(&f, &format!("{}/data/hello.properties.tmp", mibsdk::PERSIST)).exists());
    assert!(msg.contains(&format!("cleared the data of hello ({}/data/hello.properties)", mibsdk::PERSIST)), "{msg}");
    assert!(
        msg.contains("Restart the HMI now: a running hello may write its data back, and the storage cap frees the space only from the next runtime start."),
        "{msg}"
    );
}

// Review Focus 5.
#[test]
fn sdk_remove_handles_a_pushed_plugin() {
    let (f, dir) = (common::fixture(), tempfile::tempdir().unwrap());
    with_sdk(&f, dir.path());
    pushed(&f, "hello");
    grant_line(&f, "hello", "vehicle.read");
    let sys = RecordingSys::default();
    let ctx = Ctx::new(f.layout.clone(), &sys);
    let e = format!("{:#}", install::remove(&ctx, "mibsdk", false).unwrap_err());
    assert!(e.contains("pushed by mibsdk-dev: hello"), "{e}");
    let msg = plugin_pkg::remove_id(&ctx, "hello", false).unwrap();
    assert!(msg.starts_with("removed hello: plugins/hello.jar and its grants line; reload requested"), "{msg}");
    assert!(!plugin_jar(&f, "hello").exists());
    assert!(granted(&f, "hello").is_none());
    install::remove(&ctx, "mibsdk", false).unwrap();
}

#[test]
fn remove_id_ledger_drains() {
    let (f, dir) = (common::fixture(), tempfile::tempdir().unwrap());
    with_sdk(&f, dir.path());
    pushed(&f, "hello");
    ledger(&f, &format!("{LINE}\n"));
    let path = p(&f, mibsdk::LEDGER);
    let drain = thread::spawn(move || {
        thread::sleep(Duration::from_millis(500));
        fs::write(path, b"").unwrap();
    });
    let sys = RecordingSys::default();
    let ctx = Ctx::new(f.layout.clone(), &sys);
    let msg = plugin_pkg::remove_id_with(&ctx, "hello", false, ack::ACK_TIMEOUT, Duration::from_secs(10)).unwrap();
    drain.join().unwrap();
    assert!(msg.ends_with("overrides of hello restored"), "{msg}");
}

#[test]
fn remove_id_ledger_does_not_drain_warns() {
    let (f, dir) = (common::fixture(), tempfile::tempdir().unwrap());
    with_sdk(&f, dir.path());
    pushed(&f, "hello");
    ledger(&f, &format!("{LINE}\nother.key=bike|PERSISTENT|F1|F0|A\n"));
    std::env::set_var("MIBPKG_SDK_REMOVE_WAIT_S", "1");
    let sys = RecordingSys::default();
    let ctx = Ctx::new(f.layout.clone(), &sys);
    let t = Instant::now();
    let msg = plugin_pkg::remove_id(&ctx, "hello", false).unwrap();
    assert!(t.elapsed() >= Duration::from_secs(1));
    assert!(
        msg.ends_with(&format!(
            "WARNING: these overrides of hello are not restored yet:\n{LINE}\nthey are restored when the SDK next starts (the plugin is no longer installed); check /tmp/mibsdk.log"
        )),
        "{msg}"
    );
    assert!(!msg.contains("other.key"), "{msg}");
}

#[test]
fn remove_wait_is_digits_only_default_30() {
    assert_eq!(plugin_pkg::parse_remove_wait(None), Duration::from_secs(30));
    assert_eq!(plugin_pkg::parse_remove_wait(Some("")), Duration::from_secs(30));
    assert_eq!(plugin_pkg::parse_remove_wait(Some("-1")), Duration::from_secs(30));
    assert_eq!(plugin_pkg::parse_remove_wait(Some("5s")), Duration::from_secs(30));
    assert_eq!(plugin_pkg::parse_remove_wait(Some("0")), Duration::ZERO);
    assert_eq!(plugin_pkg::parse_remove_wait(Some("12")), Duration::from_secs(12));
}

// ---- rollback, update-all, seed ----

#[test]
fn plugin_rollback_never_adds_grants() {
    let (f, dir) = (common::fixture(), tempfile::tempdir().unwrap());
    with_sdk(&f, dir.path());
    let sys = RecordingSys::default();
    let mut ctx = Ctx::new(f.layout.clone(), &sys);
    // 1.0.0 declares vehicle.read and network; 1.1.0 drops network.
    approving(&mut ctx, &["vehicle.read", "network"]);
    let v1 = plugin_src(dir.path(), "hello", "1.0.0", 2, &["vehicle.read", "network"]);
    install_src(&ctx, &v1).unwrap();
    install_src(&ctx, &plugin_src(dir.path(), "hello", "1.1.0", 2, &["vehicle.read"])).unwrap();
    assert_eq!(granted(&f, "hello"), Some(set(&["vehicle.read"])));
    // Rolling back would add network back: never without an approval.
    ctx.approved = None;
    let n = needs_approval(install::rollback(&ctx, "hello"));
    assert_eq!(n.ask, vec!["network".to_string()]);
    assert_eq!(install::current_version(&f.layout, "hello").as_deref(), Some("1.1.0"));
    assert_eq!(granted(&f, "hello"), Some(set(&["vehicle.read"])));
    // The user revoked vehicle.read: it stays revoked across the rollback.
    write(&p(&f, mibsdk::GRANTS), b"hello=\n");
    approving(&mut ctx, &["network"]);
    let msg = install::rollback(&ctx, "hello").unwrap();
    assert!(msg.starts_with("rolled back hello 1.1.0 -> 1.0.0"), "{msg}");
    assert_eq!(granted(&f, "hello"), Some(set(&["network"])));
    assert_eq!(fs::read(plugin_jar(&f, "hello")).unwrap(), fs::read(v1.join("hello.jar")).unwrap());
    assert!(msg.contains("hello stays blocked until granted: vehicle.read"), "{msg}");
}

#[test]
fn update_all_reports_needs_approval_and_updates_the_rest() {
    let (f, dir) = (common::fixture(), tempfile::tempdir().unwrap());
    with_sdk(&f, dir.path());
    let sys = RecordingSys::default();
    let mut ctx = Ctx::new(f.layout.clone(), &sys);
    let h1 = plugin_src(dir.path(), "hello", "1.0.0", 2, &["vehicle.read"]);
    let t1 = common::pkg_src(f.root(), "tool", "1.0.0", "exit 0", "");
    let coll = common::collection(&f.sd(), "main", &[h1, t1]);
    approving(&mut ctx, &["vehicle.read"]);
    let colls = mibpkg::media::loaded(&mibpkg::media::scan(&f.layout));
    for name in ["hello", "tool"] {
        install::install(&ctx, &[f.sd()], &colls[0], name).unwrap();
    }
    fs::remove_dir_all(coll).unwrap();
    let h2 = plugin_src(dir.path(), "hello", "1.1.0", 2, &["vehicle.read", "network"]);
    let t2 = common::pkg_src(f.root(), "tool", "1.1.0", "exit 0", "");
    common::collection(&f.sd(), "main", &[h2, t2]);
    ctx.approved = None;
    let colls = mibpkg::media::loaded(&mibpkg::media::scan(&f.layout));
    let out = install::update_all(&ctx, &[f.sd()], &colls).unwrap();
    assert!(out.contains(&"hello: needs permission approval (network)".to_string()), "{out:?}");
    assert!(out.iter().any(|l| l.starts_with("updated tool 1.0.0 -> 1.1.0")), "{out:?}");
    assert_eq!(install::current_version(&f.layout, "hello").as_deref(), Some("1.0.0"));
}

#[test]
fn seed_skips_sdk_and_plugin_packages_and_installs_plain_ones() {
    let (f, dir) = (common::fixture(), tempfile::tempdir().unwrap());
    sdk_unit(&f);
    let sys = RecordingSys::default();
    let ctx = Ctx::new(f.layout.clone(), &sys);
    let srcs = vec![
        sdk_src(dir.path(), "0.1.0"),
        plugin_src(dir.path(), "hello", "1.0.0", 2, &[]),
        common::pkg_src(f.root(), "tool", "1.0.0", "exit 0", ""),
    ];
    common::collection(&f.sd(), "seed", &srcs);
    mibpkg::supervisor::seed_once(&ctx, &[f.sd()]).unwrap();
    let st = State::load(&f.layout).unwrap();
    assert!(st.packages.contains_key("tool"), "{:?}", st.packages.keys());
    assert!(!st.packages.contains_key("mibsdk"));
    assert!(!st.packages.contains_key("hello"));
    assert!(f.layout.state_dir().join("seeded").exists());
    assert_eq!(fs::read(p(&f, mibsdk::BUNDLES)).unwrap(), fs::read(Path::new(FIX).join("bundles_dsitracer_enabled.properties")).unwrap());
}

// ---- ported: installer/test_mibsdk_dev.py RemoveTest (on pushed ids) ----

/// The SDK installed and `id` pushed with `vehicle.read` granted (`mibsdk-dev push` + `grant`).
fn pushed_unit(ids: &[&str]) -> (common::Fixture, tempfile::TempDir) {
    let (f, dir) = (common::fixture(), tempfile::tempdir().unwrap());
    with_sdk(&f, dir.path());
    for id in ids {
        pushed(&f, id);
        write(&marker(&f, id), format!("1-1-{id}\n").as_bytes());
        grant_line(&f, id, "vehicle.read");
    }
    (f, dir)
}

#[test]
fn removes_the_jar_and_grant_and_requests_a_reload() {
    // was: test_removes_the_jar_and_grant_and_requests_a_reload
    let (f, _d) = pushed_unit(&["hello"]);
    let before = read(&marker(&f, "hello"));
    let sys = RecordingSys::default();
    let ctx = Ctx::new(f.layout.clone(), &sys);
    let msg = plugin_pkg::remove_id(&ctx, "hello", false).unwrap();
    assert!(!plugin_jar(&f, "hello").exists());
    assert!(!grants_text(&f).contains("hello="));
    assert_ne!(read(&marker(&f, "hello")), before);
    assert!(is_token(&read(&marker(&f, "hello"))));
    assert_eq!(sys.calls.borrow().last().map(String::as_str), Some("ro /mnt/app"));
    assert!(msg.contains("removed hello"), "{msg}");
    assert!(msg.contains("hello has no overrides in the ledger"), "{msg}");
}

#[test]
fn drops_the_plugins_off_switch() {
    // was: test_drops_the_plugins_off_switch
    let (f, _d) = pushed_unit(&["hello"]);
    write(&p(&f, mibsdk::TOGGLES), b"hello.off=true\nhello-2.off=true\nother.off=true\n");
    let sys = RecordingSys::default();
    let ctx = Ctx::new(f.layout.clone(), &sys);
    plugin_pkg::remove_id(&ctx, "hello", false).unwrap();
    assert_eq!(read(&p(&f, mibsdk::TOGGLES)), "hello-2.off=true\nother.off=true\n");
}

/// The reload marker cannot be written: a non-empty directory sits in its place.
fn block_marker(f: &common::Fixture, id: &str) {
    let m = marker(f, id);
    let _ = fs::remove_file(&m);
    write(&m.join("x"), b"");
}

#[test]
fn drops_the_plugins_status_line_settings() {
    let (f, _d) = pushed_unit(&["hello"]);
    write(&p(&f, mibsdk::STATUSPREFS), b"order=other,hello\nhello.hidden=true\nother.hidden=true\n");
    let sys = RecordingSys::default();
    let ctx = Ctx::new(f.layout.clone(), &sys);
    let msg = plugin_pkg::remove_id(&ctx, "hello", false).unwrap();
    assert!(msg.contains("and its status-line settings"), "{msg}");
    assert_eq!(read(&p(&f, mibsdk::STATUSPREFS)), "order=other\nother.hidden=true\n");
}

#[test]
fn an_unreadable_status_line_file_warns_but_never_blocks_remove() {
    let (f, _d) = pushed_unit(&["hello"]);
    // A directory in the file's place: reading it fails.
    write(&p(&f, mibsdk::STATUSPREFS).join("x"), b"");
    let sys = RecordingSys::default();
    let ctx = Ctx::new(f.layout.clone(), &sys);
    let msg = plugin_pkg::remove_id(&ctx, "hello", false).unwrap();
    assert!(msg.contains("WARNING: cannot update the status-line settings of hello"), "{msg}");
    assert!(msg.contains("reload requested"), "{msg}");
    assert!(is_token(&read(&marker(&f, "hello"))), "the reload marker is still written");
}

#[test]
fn plugins_properties_is_pushed_before_the_reload_marker() {
    // was: test_plugins_properties_is_pushed_before_the_reload_marker
    let (f, _d) = pushed_unit(&["hello"]);
    write(&p(&f, mibsdk::TOGGLES), b"hello.off=true\n");
    block_marker(&f, "hello");
    let sys = RecordingSys::default();
    let ctx = Ctx::new(f.layout.clone(), &sys);
    assert!(plugin_pkg::remove_id(&ctx, "hello", false).is_err());
    assert_eq!(read(&p(&f, mibsdk::TOGGLES)), "");
}

#[test]
fn leaves_a_plugin_with_a_longer_id_alone() {
    // was: test_leaves_a_plugin_with_a_longer_id_alone
    let (f, _d) = pushed_unit(&["bike", "bike-rack"]);
    write(&data(&f, "bike-rack"), b"x=1\n");
    ledger(&f, "a.key=bike|PERSISTENT|F1|F0|R\nb.key=bike-rack|PERSISTENT|F1|F0|A\n");
    let sys = RecordingSys::default();
    let ctx = Ctx::new(f.layout.clone(), &sys);
    let msg = plugin_pkg::remove_id_with(&ctx, "bike", false, ack::ACK_TIMEOUT, NO_WAIT).unwrap();
    assert!(plugin_jar(&f, "bike-rack").exists());
    assert!(grants_text(&f).contains("bike-rack=vehicle.read"));
    assert!(!grants_text(&f).replace("bike-rack=", "").contains("bike=vehicle"));
    assert!(data(&f, "bike-rack").exists());
    assert!(read(&p(&f, mibsdk::LEDGER)).contains("b.key=bike-rack|"));
    assert!(msg.contains("a.key=bike|PERSISTENT|F1|F0|R"), "{msg}");
    assert!(!msg.contains("b.key"), "{msg}");
}

#[test]
fn keeps_data_unless_purge_data() {
    // was: test_keeps_data_unless_purge_data
    let (f, _d) = pushed_unit(&["hello", "other"]);
    for id in ["hello", "other"] {
        write(&data(&f, id), b"x=1\n");
    }
    let sys = RecordingSys::default();
    let ctx = Ctx::new(f.layout.clone(), &sys);
    plugin_pkg::remove_id(&ctx, "hello", false).unwrap();
    assert!(data(&f, "hello").exists());
    let msg = plugin_pkg::remove_id(&ctx, "other", true).unwrap();
    assert!(!data(&f, "other").exists());
    assert!(data(&f, "hello").exists());
    assert!(msg.contains("cleared the data of other"), "{msg}");
}

#[test]
fn reports_overrides_still_in_the_ledger() {
    // was: test_reports_overrides_still_in_the_ledger
    let (f, _d) = pushed_unit(&["hello"]);
    ledger(&f, &format!("{LINE}\n"));
    let sys = RecordingSys::default();
    let ctx = Ctx::new(f.layout.clone(), &sys);
    let msg = plugin_pkg::remove_id_with(&ctx, "hello", false, ack::ACK_TIMEOUT, NO_WAIT).unwrap();
    assert!(msg.contains("not restored yet"), "{msg}");
    assert!(msg.contains(LINE), "{msg}");
}

#[test]
fn a_rerun_after_the_reload_request_failed_finishes_the_removal() {
    // was: test_a_rerun_after_the_reload_request_failed_finishes_the_removal
    // M2: jar and grants are gone when the marker write fails; the re-run must still request the reload.
    let (f, _d) = pushed_unit(&["hello"]);
    let before = read(&marker(&f, "hello"));
    block_marker(&f, "hello");
    let sys = RecordingSys::default();
    let ctx = Ctx::new(f.layout.clone(), &sys);
    assert!(plugin_pkg::remove_id(&ctx, "hello", false).is_err());
    assert!(!plugin_jar(&f, "hello").exists());
    assert!(!grants_text(&f).contains("hello="));
    fs::remove_dir_all(marker(&f, "hello")).unwrap();
    write(&marker(&f, "hello"), before.as_bytes());
    let msg = plugin_pkg::remove_id(&ctx, "hello", false).unwrap();
    assert_ne!(read(&marker(&f, "hello")), before);
    assert!(msg.contains("removed hello"), "{msg}");
}

#[test]
fn a_plugin_known_only_by_its_ledger_lines_is_removed() {
    // was: test_a_plugin_known_only_by_its_ledger_lines_is_removed
    // M2: no jar, grants line or marker, but the ledger still holds its overrides.
    let (f, _d) = pushed_unit(&[]);
    ledger(&f, &format!("{LINE}\n"));
    let sys = RecordingSys::default();
    let ctx = Ctx::new(f.layout.clone(), &sys);
    let msg = plugin_pkg::remove_id_with(&ctx, "hello", false, ack::ACK_TIMEOUT, NO_WAIT).unwrap();
    assert!(is_token(&read(&marker(&f, "hello"))));
    assert!(msg.contains(LINE), "{msg}");
    // An unparsable line of the id counts too.
    ledger(&f, "garbage=hello|x\n");
    let msg = plugin_pkg::remove_id_with(&ctx, "hello", false, ack::ACK_TIMEOUT, NO_WAIT).unwrap();
    assert!(msg.contains("garbage=hello|x"), "{msg}");
}

#[test]
fn refuses_an_unknown_plugin() {
    // was: test_refuses_an_unknown_plugin
    let (f, _d) = pushed_unit(&[]);
    let sys = RecordingSys::default();
    let ctx = Ctx::new(f.layout.clone(), &sys);
    let e = format!("{:#}", plugin_pkg::remove_id(&ctx, "ghost", false).unwrap_err());
    assert!(e.contains("is not installed"), "{e}");
    assert!(sys.calls.borrow().is_empty());
    let e = format!("{:#}", plugin_pkg::remove_id(&ctx, "../x", false).unwrap_err());
    assert!(e.contains("invalid plugin id"), "{e}");
}

#[test]
fn manifest_of_a_plugin_parses() {
    let dir = tempfile::tempdir().unwrap();
    let s = plugin_src(dir.path(), "hello", "1.0.0", 2, &["vehicle.read"]);
    let m = Manifest::parse(&read(&s.join("manifest.toml"))).unwrap();
    assert_eq!(m.plugin.unwrap().permissions, vec!["vehicle.read".to_string()]);
}

// ---- fix round 1 ----

/// The commit point cannot be written: a non-empty directory sits where `current` goes.
fn block_current(f: &common::Fixture, id: &str) {
    let cur = f.layout.pkg_dir(id).join("current");
    let _ = fs::remove_file(&cur);
    write(&cur.join("x"), b"");
}

/// Important 2: a failure after the deploy puts the jar and the grants line back.
#[test]
fn plugin_update_failing_after_the_deploy_puts_the_jar_and_grants_back() {
    let (f, dir) = (common::fixture(), tempfile::tempdir().unwrap());
    with_sdk(&f, dir.path());
    let sys = RecordingSys::default();
    let mut ctx = Ctx::new(f.layout.clone(), &sys);
    approving(&mut ctx, &["vehicle.read", "network"]);
    let v1 = plugin_src(dir.path(), "hello", "1.0.0", 2, &["vehicle.read"]);
    install_src(&ctx, &v1).unwrap();
    let grants_before = grants_text(&f);
    block_current(&f, "hello");
    let e = err(install_src(&ctx, &plugin_src(dir.path(), "hello", "1.1.0", 2, &["vehicle.read", "network"])));
    assert!(!e.is_empty());
    assert_eq!(fs::read(plugin_jar(&f, "hello")).unwrap(), fs::read(v1.join("hello.jar")).unwrap());
    assert_eq!(grants_text(&f), grants_before);
    assert_eq!(State::load(&f.layout).unwrap().packages["hello"].version, "1.0.0");
    // A fresh install that fails after its deploy leaves no jar and no grants line.
    block_current(&f, "fresh");
    let _ = err(install_src(&ctx, &plugin_src(dir.path(), "fresh", "1.0.0", 2, &["network"])));
    assert!(!plugin_jar(&f, "fresh").exists());
    assert!(granted(&f, "fresh").is_none());
    assert_eq!(sys.calls.borrow().last().map(String::as_str), Some("ro /mnt/app"));
}

#[test]
fn plugin_rollback_failing_after_the_deploy_puts_the_jar_and_grants_back() {
    let (f, dir) = (common::fixture(), tempfile::tempdir().unwrap());
    with_sdk(&f, dir.path());
    let sys = RecordingSys::default();
    let mut ctx = Ctx::new(f.layout.clone(), &sys);
    approving(&mut ctx, &["vehicle.read", "network"]);
    install_src(&ctx, &plugin_src(dir.path(), "hello", "1.0.0", 2, &["vehicle.read", "network"])).unwrap();
    let v2 = plugin_src(dir.path(), "hello", "1.1.0", 2, &["vehicle.read"]);
    install_src(&ctx, &v2).unwrap();
    let grants_before = grants_text(&f);
    block_current(&f, "hello");
    let _ = err(install::rollback(&ctx, "hello"));
    assert_eq!(fs::read(plugin_jar(&f, "hello")).unwrap(), fs::read(v2.join("hello.jar")).unwrap());
    assert_eq!(grants_text(&f), grants_before);
    assert_eq!(State::load(&f.layout).unwrap().packages["hello"].version, "1.1.0");
}

/// Minor 3: the manifest decides, not only the index's `kind`.
#[test]
fn seed_skips_a_plugin_whose_index_entry_has_no_kind() {
    let (f, dir) = (common::fixture(), tempfile::tempdir().unwrap());
    with_sdk(&f, dir.path()); // so the plugin would install
    let sys = RecordingSys::default();
    let ctx = Ctx::new(f.layout.clone(), &sys);
    let srcs = vec![plugin_src(dir.path(), "hello", "1.0.0", 2, &[]), common::pkg_src(f.root(), "tool", "1.0.0", "exit 0", "")];
    let coll = common::collection(&f.sd(), "seed", &srcs);
    let idx = coll.join("index.json");
    let mut v: serde_json::Value = serde_json::from_str(&read(&idx)).unwrap();
    for e in v["packages"].as_array_mut().unwrap() {
        e.as_object_mut().unwrap().remove("kind");
    }
    fs::write(&idx, serde_json::to_string_pretty(&v).unwrap()).unwrap();
    mibpkg::supervisor::seed_once(&ctx, &[f.sd()]).unwrap();
    let st = State::load(&f.layout).unwrap();
    assert!(st.packages.contains_key("tool"));
    assert!(!st.packages.contains_key("hello"));
}

/// Minor 4: a plugin package whose manifest cannot be read is removed as files, and the result
/// says how to take the plugin itself off.
#[test]
fn plugin_remove_with_an_unreadable_manifest_points_at_sdk_remove() {
    let (f, dir) = (common::fixture(), tempfile::tempdir().unwrap());
    with_sdk(&f, dir.path());
    let sys = RecordingSys::default();
    let ctx = Ctx::new(f.layout.clone(), &sys);
    install_src(&ctx, &plugin_src(dir.path(), "hello", "1.0.0", 2, &[])).unwrap();
    fs::write(f.layout.pkg_dir("hello").join("1.0.0/manifest.toml"), "garbage [").unwrap();
    let msg = install::remove(&ctx, "hello", false).unwrap();
    assert!(msg.contains("run: mibpkg sdk remove hello"), "{msg}");
    assert!(!installed_pkg(&f, "hello"));
    plugin_pkg::remove_id(&ctx, "hello", false).unwrap();
    assert!(!plugin_jar(&f, "hello").exists());
}

/// The runtime logs `token=<tok>`: the marker's first 4096 bytes as Latin-1, only bytes
/// 0x21-0x7e kept, at most 64 chars. mibpkg compares against the same form.
#[test]
fn ack_token_is_the_runtimes_sanitised_form_of_the_marker() {
    assert_eq!(ack::token_of(b"1700000000-123-4\n"), "1700000000-123-4");
    assert_eq!(ack::token_of(b" a b\tc\r\n\x7f\xe9d"), "abcd");
    assert_eq!(ack::token_of(&[b'x'; 100]), "x".repeat(64));
    let mut late = vec![b' '; 4096];
    late.extend(b"zz");
    assert_eq!(ack::token_of(&late), "");
}

/// mibpkg's own marker tokens round-trip exactly: printable ASCII, no spaces, at most 64 chars.
#[test]
fn plugin_marker_token_round_trips_through_the_runtime_form() {
    let (f, dir) = (common::fixture(), tempfile::tempdir().unwrap());
    with_sdk(&f, dir.path());
    let sys = RecordingSys::default();
    let ctx = Ctx::new(f.layout.clone(), &sys);
    install_src(&ctx, &plugin_src(dir.path(), "hello", "1.0.0", 2, &[])).unwrap();
    running(&f);
    plugin_pkg::activate_with(&ctx, "hello", Duration::ZERO);
    let raw = fs::read(marker(&f, "hello")).unwrap();
    let tok = ack::token_of(&raw);
    assert_eq!(format!("{tok}\n").as_bytes(), raw.as_slice());
    assert!(!tok.is_empty() && tok.len() <= 64);
}

// ---- fix round 2 ----

/// Where an install or rollback fails after its commit point (`current` written).
#[derive(Clone, Copy, Debug)]
enum After {
    /// `bootd::write_for_package`: boot.d is a file.
    Bootd,
    /// `State::save`: its temp file is a non-empty directory.
    StateSave,
}

fn block(f: &common::Fixture, at: After) -> PathBuf {
    let p = match at {
        After::Bootd => f.layout.boot_d(),
        After::StateSave => PathBuf::from(format!("{}.tmp", f.layout.state_dir().join("installed.json").display())),
    };
    let _ = fs::remove_dir_all(&p);
    match at {
        After::Bootd => write(&p, b"not a dir"),
        After::StateSave => write(&p.join("x"), b""),
    }
    p
}

fn unblock(p: &Path) {
    if p.is_dir() {
        fs::remove_dir_all(p).unwrap();
    } else {
        fs::remove_file(p).unwrap();
    }
}

/// The jar and the grants line match the version `current` names, right after the failure and
/// after the repair run.
#[test]
fn plugin_update_failing_after_the_commit_point_keeps_the_new_jar_and_repair_records_it() {
    for at in [After::Bootd, After::StateSave] {
        let (f, dir) = (common::fixture(), tempfile::tempdir().unwrap());
        with_sdk(&f, dir.path());
        let sys = RecordingSys::default();
        let mut ctx = Ctx::new(f.layout.clone(), &sys);
        approving(&mut ctx, &["vehicle.read", "network"]);
        install_src(&ctx, &plugin_src(dir.path(), "hello", "1.0.0", 2, &["vehicle.read"])).unwrap();
        let v2 = plugin_src(dir.path(), "hello", "1.1.0", 2, &["vehicle.read", "network"]);
        let b = block(&f, at);
        let _ = err(install_src(&ctx, &v2));
        assert_eq!(install::current_version(&f.layout, "hello").as_deref(), Some("1.1.0"), "{at:?}");
        assert_eq!(fs::read(plugin_jar(&f, "hello")).unwrap(), fs::read(v2.join("hello.jar")).unwrap(), "{at:?}");
        assert_eq!(granted(&f, "hello"), Some(set(&["vehicle.read", "network"])), "{at:?}");
        unblock(&b);
        // The repair run (no approval needed) records 1.1.0, and the unit runs 1.1.0.
        ctx.approved = None;
        let msg = install_src(&ctx, &v2).unwrap();
        assert!(msg.starts_with("repaired hello 1.1.0"), "{at:?}: {msg}");
        assert_eq!(State::load(&f.layout).unwrap().packages["hello"].version, "1.1.0");
        assert_eq!(fs::read(plugin_jar(&f, "hello")).unwrap(), fs::read(v2.join("hello.jar")).unwrap(), "{at:?}");
        assert_eq!(granted(&f, "hello"), Some(set(&["vehicle.read", "network"])), "{at:?}");
    }
}

/// A repair re-deploys from the version directory even when the unit holds another jar, and never
/// grants more than is granted now.
#[test]
fn plugin_repair_redeploys_the_recorded_jar_without_adding_grants() {
    let (f, dir) = (common::fixture(), tempfile::tempdir().unwrap());
    with_sdk(&f, dir.path());
    let sys = RecordingSys::default();
    let mut ctx = Ctx::new(f.layout.clone(), &sys);
    approving(&mut ctx, &["vehicle.read", "network"]);
    install_src(&ctx, &plugin_src(dir.path(), "hello", "1.0.0", 2, &["vehicle.read"])).unwrap();
    let v2 = plugin_src(dir.path(), "hello", "1.1.0", 2, &["vehicle.read", "network"]);
    let b = block(&f, After::StateSave);
    let _ = err(install_src(&ctx, &v2));
    unblock(&b);
    // Something put the old jar and grants back meanwhile.
    write(&plugin_jar(&f, "hello"), b"stale\n");
    write(&p(&f, mibsdk::GRANTS), b"hello=vehicle.read\n");
    ctx.approved = None;
    let msg = install_src(&ctx, &v2).unwrap();
    assert_eq!(fs::read(plugin_jar(&f, "hello")).unwrap(), fs::read(v2.join("hello.jar")).unwrap());
    assert_eq!(granted(&f, "hello"), Some(set(&["vehicle.read"])));
    assert!(msg.contains("hello stays blocked until granted: network"), "{msg}");
}

#[test]
fn plugin_rollback_failing_after_the_commit_point_keeps_the_rolled_back_jar() {
    for at in [After::Bootd, After::StateSave] {
        let (f, dir) = (common::fixture(), tempfile::tempdir().unwrap());
        with_sdk(&f, dir.path());
        let sys = RecordingSys::default();
        let mut ctx = Ctx::new(f.layout.clone(), &sys);
        approving(&mut ctx, &["vehicle.read", "network"]);
        let v1 = plugin_src(dir.path(), "hello", "1.0.0", 2, &["vehicle.read"]);
        install_src(&ctx, &v1).unwrap();
        install_src(&ctx, &plugin_src(dir.path(), "hello", "1.1.0", 2, &["vehicle.read", "network"])).unwrap();
        let b = block(&f, at);
        ctx.approved = None;
        let _ = err(install::rollback(&ctx, "hello"));
        assert_eq!(install::current_version(&f.layout, "hello").as_deref(), Some("1.0.0"), "{at:?}");
        assert_eq!(fs::read(plugin_jar(&f, "hello")).unwrap(), fs::read(v1.join("hello.jar")).unwrap(), "{at:?}");
        assert_eq!(granted(&f, "hello"), Some(set(&["vehicle.read"])), "{at:?}");
        unblock(&b);
        // Running the rollback again finishes it.
        install::rollback(&ctx, "hello").unwrap();
        assert_eq!(State::load(&f.layout).unwrap().packages["hello"].version, "1.0.0", "{at:?}");
        assert_eq!(fs::read(plugin_jar(&f, "hello")).unwrap(), fs::read(v1.join("hello.jar")).unwrap(), "{at:?}");
        assert_eq!(granted(&f, "hello"), Some(set(&["vehicle.read"])), "{at:?}");
    }
}

/// A runtime that logs tokens answers by token only: a token-less line is never taken for ours.
#[test]
fn ack_a_token_capable_runtime_is_answered_by_token_only() {
    let base = ack::Baseline::of("0 INFO x start\n");
    let tail = "2 INFO dev dev channel: reload requested for hello\n3 STATUS hello ENABLED x\n";
    let plain = format!("0 INFO x start\n{tail}");
    assert_eq!(ack::answer(&plain, "hello", &base, Some("9-9-9"), false), ok("ENABLED", "x"));
    let mixed = format!("0 INFO x start\n1 INFO dev dev channel: reload requested for other token=1-1-1\n{tail}");
    assert_eq!(ack::answer(&mixed, "hello", &base, Some("9-9-9"), false), None);
    assert_eq!(ack::answer(&mixed, "hello", &base, Some("9-9-9"), true), None);
}

// ---- removal while the runtime may be live without a package (Task 9 fix round 2) ----

/// The SDK package removed (`--force`, plugins/ kept): the runtime keeps running until the reboot.
fn sdk_package_gone(f: &common::Fixture, dir: &Path) {
    with_sdk(f, dir);
    let sys = RecordingSys::default();
    let mut ctx = Ctx::new(f.layout.clone(), &sys);
    ctx.force = true;
    install::remove(&ctx, "mibsdk", false).unwrap();
    assert!(mibsdk::installed_sdk(&f.layout).is_none());
    assert!(mibsdk::reboot_flag(&f.layout).exists(), "the removal waits for the reboot");
}

#[test]
fn remove_with_a_live_runtime_reloads_waits_and_keeps_purged_data_without_an_ack() {
    // A runtime log this boot means a live runtime, whatever the package state or reboot flag says.
    for case in ["package gone, reboot pending", "package gone, no reboot flag", "package installed, reboot pending"] {
        let (f, dir) = (common::fixture(), tempfile::tempdir().unwrap());
        if case.starts_with("package gone") {
            sdk_package_gone(&f, dir.path());
        } else {
            with_sdk(&f, dir.path());
        }
        if case.ends_with("no reboot flag") {
            fs::remove_file(mibsdk::reboot_flag(&f.layout)).unwrap();
        }
        write(&p(&f, mibsdk::LOG), b"100 STATUS mibsdk runtime started adapter=hqc safeMode=false\n");
        pushed(&f, "hello");
        write(&data(&f, "hello"), b"x=1\n");
        ledger(&f, &format!("{LINE}\n"));
        let sys = RecordingSys::default();
        let ctx = Ctx::new(f.layout.clone(), &sys);
        let t = Instant::now();
        let msg = plugin_pkg::remove_id_with(&ctx, "hello", true, Duration::from_millis(200), Duration::from_millis(300)).unwrap();
        assert!(t.elapsed() >= Duration::from_millis(300), "{case}: the ledger was not waited for");
        assert!(is_token(&read(&marker(&f, "hello"))), "{case}: no reload requested");
        assert!(msg.contains("; reload requested"), "{case}: {msg}");
        assert!(msg.contains("they are restored when the SDK next starts"), "{case}: {msg}");
        assert!(!msg.contains("nothing restores them"), "{case}: {msg}");
        assert!(data(&f, "hello").exists(), "{case}: purged before the runtime confirmed the unload");
        assert!(msg.contains("data kept: the SDK did not confirm the unload"), "{case}: {msg}");
        assert!(!plugin_jar(&f, "hello").exists());
    }
}

#[test]
fn remove_with_neither_package_nor_runtime_is_immediate_and_drops_a_stale_marker() {
    let (f, dir) = (common::fixture(), tempfile::tempdir().unwrap());
    sdk_package_gone(&f, dir.path());
    assert!(!p(&f, mibsdk::LOG).exists());
    pushed(&f, "hello");
    write(&marker(&f, "hello"), b"1-1-1\n"); // left by an earlier partial run
    write(&data(&f, "hello"), b"x=1\n");
    ledger(&f, &format!("{LINE}\n"));
    let sys = RecordingSys::default();
    let ctx = Ctx::new(f.layout.clone(), &sys);
    let t = Instant::now();
    let msg = plugin_pkg::remove_id_with(&ctx, "hello", true, ack::ACK_TIMEOUT, Duration::from_secs(30)).unwrap();
    assert!(t.elapsed() < Duration::from_secs(5), "waited {:?}", t.elapsed());
    assert!(msg.starts_with("removed hello: plugins/hello.jar and its grants line; the SDK is neither installed nor running, so no reload was requested"), "{msg}");
    assert!(msg.contains("nothing restores them while the SDK is not installed"), "{msg}");
    assert!(!marker(&f, "hello").exists(), "the stale marker is gone");
    assert!(!plugin_jar(&f, "hello").exists() && !data(&f, "hello").exists());
}

/// A committed plugin install whose request to the supervisor fails is still installed: success
/// with a warning, never a failure (final review M5).
#[test]
fn plugin_install_reports_success_when_the_reload_request_fails() {
    let (f, dir) = (common::fixture(), tempfile::tempdir().unwrap());
    with_sdk(&f, dir.path());
    let _ = fs::remove_file(f.layout.run("reload"));
    write(&f.layout.run("reload").join("blocker"), b"x"); // the request file cannot be written
    let sys = RecordingSys::default();
    let ctx = Ctx::new(f.layout.clone(), &sys);
    let msg = install_src(&ctx, &plugin_src(dir.path(), "hello", "1.0.0", 2, &[])).unwrap();
    assert!(msg.starts_with("installed hello 1.0.0"), "{msg}");
    assert!(msg.contains("warning: could not ask the mibpkg supervisor to reload"), "{msg}");
    assert!(msg.ends_with("the SDK is not running; it loads at the next boot"), "{msg}");
    assert!(plugin_jar(&f, "hello").exists());
    assert_eq!(install::current_version(&f.layout, "hello").as_deref(), Some("1.0.0"));
    assert!(State::load(&f.layout).unwrap().packages.contains_key("hello"));
}

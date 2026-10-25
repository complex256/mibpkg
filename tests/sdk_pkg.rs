//! The SDK runtime package (`[sdk]`): install guards, adoption of an old `mibsdk-dev` setup,
//! state migration, all-or-undo deploy, and the guarded uninstall. Ports `installer/test_mibsdk_dev.py`'s
//! InstallTest / UninstallTest from the SDK repo (each port carries `// was: <python test>`).
mod common;
use mibpkg::archive::{self, Package};
use mibpkg::backups;
use mibpkg::ctx::Ctx;
use mibpkg::install;
use mibpkg::manifest::{Manifest, SdkSpec};
use mibpkg::mibsdk::{self, bundles, sdk_pkg, zipnames, FileOps, StdOps};
use mibpkg::state::State;
use mibpkg::sys::RecordingSys;
use std::collections::BTreeMap;
use std::fs;
use std::io;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};

const FIX: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/mibsdk");
const MANIFEST: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/manifests/sdk-mibsdk.toml");
const SDK_JARS: [&str; 3] = ["mibsdk-runtime.jar", "mibsdk-adapter-hqc.jar", "mibsdk-rt-retrotranslator.jar"];
const HMICAR_STOCK: &str = "HMICar=de.audi.tghu.car.hmi.Activator";
const HMICAR_SDK: &str = "HMICar=mibsdk.adapter.hqc.ui.SdkCarActivator";
const ENTRY: &str = "parking.soundDirections=bike-rack|PERSISTENT|F1R1L1X1|F0R0L0X0|A\n";
const OTHER: &str = "parking.soundLevel=bike-rack|PERSISTENT|F1R1L1X1|F0R0L0X0|A\n";
const REBOOT_START: &str = "; reboot the unit (or restart the HMI) to start it";
const REBOOT_REMOVE: &str = "; reboot the unit (or restart the HMI) to finish removing it";

// ---- fixtures ----

fn stock() -> Vec<u8> {
    fs::read(Path::new(FIX).join("bundles_dsitracer_enabled.properties")).unwrap()
}

fn spec() -> SdkSpec {
    Manifest::parse(&fs::read_to_string(MANIFEST).unwrap()).unwrap().sdk.unwrap()
}

fn p(f: &common::Fixture, abs: &str) -> PathBuf {
    f.layout.p(abs)
}

fn write(path: &Path, data: &[u8]) {
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, data).unwrap();
}

fn mode(path: &Path) -> u32 {
    fs::metadata(path).unwrap().permissions().mode() & 0o777
}

/// A minimal zip (stored entries, CRC left 0: only the central directory matters here).
fn zip_bytes(names: &[&str]) -> Vec<u8> {
    let (mut out, mut cd) = (Vec::new(), Vec::new());
    for n in names {
        let off = out.len() as u32;
        let name = n.as_bytes();
        out.extend(0x0403_4b50u32.to_le_bytes());
        for v in [20u16, 0, 0, 0, 0] {
            out.extend(v.to_le_bytes());
        }
        for v in [0u32, 1, 1] {
            out.extend(v.to_le_bytes());
        }
        out.extend((name.len() as u16).to_le_bytes());
        out.extend(0u16.to_le_bytes());
        out.extend(name);
        out.push(b'x');
        cd.extend(0x0201_4b50u32.to_le_bytes());
        for v in [20u16, 20, 0, 0, 0, 0] {
            cd.extend(v.to_le_bytes());
        }
        for v in [0u32, 1, 1] {
            cd.extend(v.to_le_bytes());
        }
        for v in [name.len() as u16, 0, 0, 0, 0] {
            cd.extend(v.to_le_bytes());
        }
        cd.extend(0u32.to_le_bytes());
        cd.extend(off.to_le_bytes());
        cd.extend(name);
    }
    let cd_off = out.len() as u32;
    out.extend(&cd);
    out.extend(0x0605_4b50u32.to_le_bytes());
    for v in [0u16, 0, names.len() as u16, names.len() as u16] {
        out.extend(v.to_le_bytes());
    }
    out.extend((cd.len() as u32).to_le_bytes());
    out.extend(cd_off.to_le_bytes());
    out.extend(0u16.to_le_bytes());
    out
}

fn jar_with(path: &Path, entries: &[&str]) {
    write(path, &zip_bytes(entries));
}

/// The unit side: lsd/ with the stock bundles file (0777), the DSITracer marker and jar, jars/
/// with one stock jar, and /ifs/version_info.txt naming the supported HMI.
fn sdk_unit(f: &common::Fixture) {
    write(&p(f, mibsdk::BUNDLES), &stock());
    fs::set_permissions(p(f, mibsdk::BUNDLES), fs::Permissions::from_mode(0o777)).unwrap();
    write(&p(f, mibsdk::MARKER), b"");
    write(&p(f, mibsdk::TRACER_JAR), b"tracer");
    jar_with(&p(f, &format!("{}/stock.jar", mibsdk::JARS)), &["de/audi/other/Thing.class"]);
    write(&p(f, mibsdk::VERSION_INFO), &fs::read(Path::new(FIX).join("version_info.txt")).unwrap());
    fs::create_dir_all(p(f, "/mnt/persist")).unwrap();
}

fn tracer_off(f: &common::Fixture) {
    fs::remove_file(p(f, mibsdk::MARKER)).unwrap();
}

/// A check stamp in CheckStamp's format 1 for `jars` in `dir`.
fn write_stamp(dir: &Path, name: &str, kind: &str, jars: &[&str]) {
    let mut lines = vec!["format=1".to_string(), format!("kind={kind}"), "checks=classVersion48,distLinkCheck".to_string()];
    if kind == "plugin" {
        lines.push("plugin.id=mibsdk".into());
        lines.push("plugin.unrestricted=false".into());
    }
    let mut sorted = jars.to_vec();
    sorted.sort();
    for j in sorted {
        let (h, _) = archive::sha256_file(&dir.join(j)).unwrap();
        lines.push(format!("sha256.{j}={h}"));
    }
    fs::write(dir.join(name), lines.join("\n") + "\n").unwrap();
}

/// An `[sdk]` package source: the manifest, the jars (content names the version), a valid
/// `mibsdk-unit.stamp` and `permissions.json`.
fn sdk_src(dir: &Path, version: &str) -> PathBuf {
    sdk_src_with(dir, version, &SDK_JARS, 1, 2)
}

fn sdk_src_with(dir: &Path, version: &str, jars: &[&str], min_api: u32, api: u32) -> PathBuf {
    let s = dir.join(format!("src-mibsdk-{version}"));
    fs::create_dir_all(&s).unwrap();
    let list: Vec<String> = jars.iter().map(|j| format!("{j:?}")).collect();
    let text = fs::read_to_string(MANIFEST)
        .unwrap()
        .replace("version = \"0.1.0\"", &format!("version = \"{version}\""))
        .replace(
            "jars = [\"mibsdk-runtime.jar\", \"mibsdk-adapter-hqc.jar\", \"mibsdk-rt-retrotranslator.jar\"]",
            &format!("jars = [{}]", list.join(", ")),
        )
        .replace("api = 2\nmin_api = 1", &format!("api = {api}\nmin_api = {min_api}"));
    fs::write(s.join("manifest.toml"), text).unwrap();
    for j in jars {
        fs::write(s.join(j), format!("{j} {version}\n")).unwrap();
    }
    write_stamp(&s, "mibsdk-unit.stamp", "unit", jars);
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

fn install_sdk(ctx: &Ctx, dir: &Path, version: &str) -> anyhow::Result<String> {
    install_src(ctx, &sdk_src(dir, version))
}

/// A `[plugin]` package without permissions (deployed by Task 8: needs the SDK installed).
fn install_plugin(ctx: &Ctx, dir: &Path, id: &str, api: u32) {
    let s = dir.join(format!("src-{id}"));
    fs::create_dir_all(&s).unwrap();
    fs::write(
        s.join("manifest.toml"),
        format!(
            "[package]\nname = \"{id}\"\nversion = \"1.0.0\"\n\n[plugin]\njar = \"{id}.jar\"\napi = {api}\npermissions = []\n"
        ),
    )
    .unwrap();
    fs::write(s.join(format!("{id}.jar")), "jar").unwrap();
    let jar = format!("{id}.jar");
    let (h, _) = archive::sha256_file(&s.join(&jar)).unwrap();
    fs::write(
        s.join(format!("{jar}.stamp")),
        format!("format=1\nkind=plugin\nchecks=classVersion48\nplugin.id={id}\nplugin.unrestricted=false\nsha256.{jar}={h}\n"),
    )
    .unwrap();
    install_src(ctx, &s).unwrap();
}

fn err(r: anyhow::Result<String>) -> String {
    format!("{:#}", r.expect_err("expected a refusal"))
}

fn autostart(f: &common::Fixture) -> String {
    let b = String::from_utf8(fs::read(p(f, mibsdk::BUNDLES)).unwrap()).unwrap();
    let lines: Vec<&str> = b.lines().filter(|l| l.starts_with("Bundle.Startup.Autostart")).collect();
    assert_eq!(lines.len(), 1, "{lines:?}");
    lines[0].to_string()
}

fn sdk_props(f: &common::Fixture) -> String {
    fs::read_to_string(p(f, &format!("{}/sdk.properties", mibsdk::BASE))).unwrap()
}

/// Every file and directory under the root except /tmp: (kind, bytes, mode).
fn tree(root: &Path) -> BTreeMap<PathBuf, (bool, Vec<u8>, u32)> {
    fn walk(root: &Path, dir: &Path, out: &mut BTreeMap<PathBuf, (bool, Vec<u8>, u32)>) {
        for e in fs::read_dir(dir).unwrap() {
            let e = e.unwrap();
            let path = e.path();
            let rel = path.strip_prefix(root).unwrap().to_path_buf();
            if rel == Path::new("tmp") {
                continue;
            }
            let md = fs::symlink_metadata(&path).unwrap();
            let m = md.permissions().mode();
            if md.is_dir() {
                out.insert(rel, (true, Vec::new(), m));
                walk(root, &path, out);
            } else if md.file_type().is_symlink() {
                out.insert(rel, (false, fs::read_link(&path).unwrap().into_os_string().into_encoded_bytes(), m));
            } else {
                out.insert(rel, (false, fs::read(&path).unwrap(), m));
            }
        }
    }
    let mut out = BTreeMap::new();
    walk(root, root, &mut out);
    out
}

fn sdk_jars_present(f: &common::Fixture) -> Vec<&'static str> {
    SDK_JARS.iter().copied().filter(|j| p(f, &format!("{}/{j}", mibsdk::JARS)).is_file()).collect()
}

/// Every `check` refusal: the message, no remount, and no file anywhere changed.
fn assert_refused_untouched(f: &common::Fixture, dir: &Path, want: &str) {
    let before = tree(f.root());
    let sys = RecordingSys::default();
    let ctx = Ctx::new(f.layout.clone(), &sys);
    let e = err(install_sdk(&ctx, dir, "0.1.0"));
    assert!(e.contains(want), "{e}");
    assert!(e.ends_with("; nothing changed"), "{e}");
    assert!(sys.calls.borrow().is_empty(), "/mnt/app was remounted: {:?}", sys.calls.borrow());
    assert_eq!(tree(f.root()), before, "{want}");
}

fn unit() -> (common::Fixture, tempfile::TempDir) {
    let f = common::fixture();
    sdk_unit(&f);
    (f, tempfile::tempdir().unwrap())
}

// ---- zipnames ----

#[test]
fn zipnames_lists_entries_and_rejects_garbage() {
    let d = tempfile::tempdir().unwrap();
    let jar = d.path().join("a.jar");
    jar_with(&jar, &["META-INF/MANIFEST.MF", "de/audi/tghu/car/hmi/Activator.class", "a/B.class"]);
    assert_eq!(
        zipnames::list(&jar).unwrap(),
        vec!["META-INF/MANIFEST.MF", "de/audi/tghu/car/hmi/Activator.class", "a/B.class"]
    );
    // An archive comment after the end record is fine.
    let mut with_comment = zip_bytes(&["x/Y.class"]);
    let n = with_comment.len();
    with_comment[n - 2..].copy_from_slice(&5u16.to_le_bytes());
    with_comment.extend(b"hello");
    fs::write(&jar, &with_comment).unwrap();
    assert_eq!(zipnames::list(&jar).unwrap(), vec!["x/Y.class"]);
    // An empty zip lists nothing.
    fs::write(&jar, zip_bytes(&[])).unwrap();
    assert!(zipnames::list(&jar).unwrap().is_empty());

    let good = zip_bytes(&["x/Y.class", "z/W.class"]);
    let mut bad: Vec<Vec<u8>> = vec![
        b"not a zip".to_vec(),
        Vec::new(),
        good[..good.len() - 1].to_vec(),           // truncated end record
        good[good.len() - 22..].to_vec(),          // central directory gone
    ];
    let mut wrong_count = good.clone();
    let n = wrong_count.len();
    wrong_count[n - 12..n - 10].copy_from_slice(&3u16.to_le_bytes()); // 3 entries claimed, 2 present
    bad.push(wrong_count);
    let mut past_eof = good.clone();
    let n = past_eof.len();
    past_eof[n - 6..n - 2].copy_from_slice(&0xffff_fff0u32.to_le_bytes()); // cd offset beyond the file
    bad.push(past_eof);
    let mut bad_sig = good.clone();
    let cd = u32::from_le_bytes(good[good.len() - 6..good.len() - 2].try_into().unwrap()) as usize;
    bad_sig[cd] = 0;
    bad.push(bad_sig);
    for (i, b) in bad.iter().enumerate() {
        fs::write(&jar, b).unwrap();
        assert!(zipnames::list(&jar).is_err(), "case {i} was accepted");
    }
    assert!(zipnames::list(&d.path().join("missing.jar")).is_err());
}

// ---- fresh install ----

// was: test_tracer_on_unit_keeps_dsitracer_chained
#[test]
fn sdk_fresh_install_writes_jars_props_bundles_last_and_flags_reboot() {
    let (f, dir) = unit();
    let sys = RecordingSys::default();
    let ctx = Ctx::new(f.layout.clone(), &sys);
    let src = sdk_src(dir.path(), "0.1.0");
    let msg = install_src(&ctx, &src).unwrap();
    assert_eq!(msg, format!("installed mibsdk 0.1.0{REBOOT_START}"));
    let mut newest_jar = std::time::SystemTime::UNIX_EPOCH;
    for j in SDK_JARS {
        let dst = p(&f, &format!("{}/{j}", mibsdk::JARS));
        assert_eq!(fs::read(&dst).unwrap(), fs::read(src.join(j)).unwrap(), "{j}");
        newest_jar = newest_jar.max(fs::metadata(&dst).unwrap().modified().unwrap());
    }
    let b = p(&f, mibsdk::BUNDLES);
    assert_eq!(fs::read(&b).unwrap(), bundles::generate(&stock(), &spec(), true).unwrap());
    assert!(fs::metadata(&b).unwrap().modified().unwrap() >= newest_jar, "the bundle list is written last");
    assert_eq!(mode(&b), 0o777);
    assert!(fs::read_to_string(&b).unwrap().contains(&format!("{HMICAR_SDK}\r\n")));
    assert_eq!(
        autostart(&f),
        "Bundle.Startup.Autostart = DSITracer,MibSdk,FwUtilServices,Profiling,Tracing,ATIPImpl"
    );
    let props = sdk_props(&f);
    assert!(props.starts_with("chain.configurator=de.audi.tghu.dsitrace.TraceConfigurator\n"), "{props}");
    assert!(props.contains("ui.enabled=true\n"));
    assert_eq!(
        fs::read(p(&f, mibsdk::PERMISSIONS)).unwrap(),
        fs::read(Path::new(FIX).join("permissions.json")).unwrap()
    );
    assert!(mibsdk::reboot_flag(&f.layout).exists());
    assert_eq!(fs::read(backups::backup_path(&f.layout, mibsdk::BUNDLES)).unwrap(), stock());
    assert!(!backups::is_tombstone(&f.layout, mibsdk::MARKER));
    assert_eq!(install::current_version(&f.layout, "mibsdk").as_deref(), Some("0.1.0"));
    // One hold for the whole install; each write re-asserts read-write (F8), then back to ro once.
    let calls = sys.calls.borrow();
    assert_eq!(calls.last().map(String::as_str), Some("ro /mnt/app"), "{calls:?}");
    assert!(calls[..calls.len() - 1].iter().all(|c| c == "rw /mnt/app"), "{calls:?}");
}

// was: test_tracer_off_unit_does_not_get_dsitracer_autostarted
#[test]
fn sdk_install_on_tracer_off_unit_creates_marker_and_restore_removes_it() {
    let (f, dir) = unit();
    tracer_off(&f);
    let sys = RecordingSys::default();
    let ctx = Ctx::new(f.layout.clone(), &sys);
    install_sdk(&ctx, dir.path(), "0.1.0").unwrap();
    assert_eq!(autostart(&f), "Bundle.Startup.Autostart = MibSdk,FwUtilServices,Profiling,Tracing,ATIPImpl");
    assert!(!sdk_props(&f).contains("chain.configurator"));
    assert!(p(&f, mibsdk::MARKER).exists(), "lsd.sh reads the on-disk bundles file only with the marker");
    assert!(backups::is_tombstone(&f.layout, mibsdk::MARKER));
    let msg = install::remove(&ctx, "mibsdk", false).unwrap();
    assert!(msg.ends_with(REBOOT_REMOVE), "{msg}");
    assert!(!p(&f, mibsdk::MARKER).exists(), "it was absent before the install");
    assert!(!backups::has_backup(&f.layout, mibsdk::MARKER));
}

// was: test_install_points_hmicar_at_the_sdk_keeping_crlf
#[test]
fn sdk_install_points_hmicar_at_the_sdk_keeping_crlf() {
    let (f, dir) = unit();
    tracer_off(&f);
    let sys = RecordingSys::default();
    let ctx = Ctx::new(f.layout.clone(), &sys);
    install_sdk(&ctx, dir.path(), "0.1.0").unwrap();
    assert!(fs::read_to_string(p(&f, mibsdk::BUNDLES)).unwrap().contains(&format!("{HMICAR_SDK}\r\n")));
    assert!(sdk_props(&f).contains("ui.enabled=true\n"));
}

// was: test_creates_the_persistent_root_and_no_runtime_state_on_app
#[test]
fn sdk_install_creates_persist_and_no_runtime_state_on_app() {
    let (f, dir) = unit();
    let sys = RecordingSys::default();
    let ctx = Ctx::new(f.layout.clone(), &sys);
    install_sdk(&ctx, dir.path(), "0.1.0").unwrap();
    assert!(p(&f, mibsdk::PERSIST).is_dir());
    assert!(p(&f, &format!("{}/plugins", mibsdk::BASE)).is_dir());
    for name in ["cache", "data", "boot.state", "ledger.properties"] {
        assert!(!p(&f, &format!("{}/{name}", mibsdk::BASE)).exists(), "{name}");
    }
}

// ---- adoption and migration (Review Focus 3) ----

#[test]
fn sdk_install_adopts_mibsdk_dev_stock_backup() {
    let (f, dir) = unit();
    write(&p(&f, mibsdk::BUNDLES), &bundles::generate(&stock(), &spec(), true).unwrap());
    let st = format!("{}/stock", mibsdk::BASE);
    write(&p(&f, &format!("{st}/bundles_dsitracer_enabled.properties")), &stock());
    write(&p(&f, &format!("{st}/bundles.mode")), b"777\n");
    write(&p(&f, &format!("{st}/tracer.was")), b"true\n");
    for j in SDK_JARS {
        write(&p(&f, &format!("{}/{j}", mibsdk::JARS)), b"old mibsdk-dev jar");
    }
    let sys = RecordingSys::default();
    let ctx = Ctx::new(f.layout.clone(), &sys);
    install_sdk(&ctx, dir.path(), "0.1.0").unwrap();
    let bk = backups::backup_path(&f.layout, mibsdk::BUNDLES);
    assert_eq!(fs::read(&bk).unwrap(), stock(), "the stock file, never the generated one");
    assert_eq!(mode(&bk), 0o777);
    assert!(!p(&f, &st).exists(), "stock/ goes once adopted");
    assert!(!backups::is_tombstone(&f.layout, mibsdk::MARKER));
    assert_eq!(fs::read(p(&f, mibsdk::BUNDLES)).unwrap(), bundles::generate(&stock(), &spec(), true).unwrap());
    // Uninstall then puts the stock file back.
    install::remove(&ctx, "mibsdk", false).unwrap();
    assert_eq!(fs::read(p(&f, mibsdk::BUNDLES)).unwrap(), stock());
    assert!(p(&f, mibsdk::MARKER).exists(), "tracer.was said the marker was there");
}

#[test]
fn sdk_install_adopts_a_tracer_off_backup_as_a_marker_tombstone() {
    let (f, dir) = unit();
    write(&p(&f, mibsdk::BUNDLES), &bundles::generate(&stock(), &spec(), false).unwrap());
    let st = format!("{}/stock", mibsdk::BASE);
    write(&p(&f, &format!("{st}/bundles_dsitracer_enabled.properties")), &stock());
    write(&p(&f, &format!("{st}/bundles.mode")), b"777\n");
    write(&p(&f, &format!("{st}/tracer.was")), b"false\n");
    let sys = RecordingSys::default();
    let ctx = Ctx::new(f.layout.clone(), &sys);
    install_sdk(&ctx, dir.path(), "0.1.0").unwrap();
    assert!(backups::is_tombstone(&f.layout, mibsdk::MARKER));
    assert!(!autostart(&f).contains("DSITracer"));
}

#[test]
fn sdk_install_refuses_a_corrupt_tracer_was() {
    let (f, dir) = unit();
    let st = format!("{}/stock", mibsdk::BASE);
    write(&p(&f, &format!("{st}/bundles_dsitracer_enabled.properties")), &stock());
    write(&p(&f, &format!("{st}/tracer.was")), b"maybe\n");
    let before = fs::read(p(&f, mibsdk::BUNDLES)).unwrap();
    let sys = RecordingSys::default();
    let ctx = Ctx::new(f.layout.clone(), &sys);
    let e = err(install_sdk(&ctx, dir.path(), "0.1.0"));
    assert!(e.contains("stock/tracer.was is corrupt"), "{e}");
    assert_eq!(fs::read(p(&f, mibsdk::BUNDLES)).unwrap(), before);
    assert!(!backups::has_backup(&f.layout, mibsdk::BUNDLES));
    assert!(p(&f, &st).is_dir());
}

// was: test_backup_without_a_recorded_mode_restores_the_stock_mode_0777
#[test]
fn sdk_adopted_backup_without_a_recorded_mode_restores_the_stock_mode_0777() {
    let (f, dir) = unit();
    write(&p(&f, mibsdk::BUNDLES), &bundles::generate(&stock(), &spec(), true).unwrap());
    let st = format!("{}/stock", mibsdk::BASE);
    write(&p(&f, &format!("{st}/bundles_dsitracer_enabled.properties")), &stock());
    write(&p(&f, &format!("{st}/tracer.was")), b"true\n");
    let sys = RecordingSys::default();
    let ctx = Ctx::new(f.layout.clone(), &sys);
    install_sdk(&ctx, dir.path(), "0.1.0").unwrap();
    fs::set_permissions(p(&f, mibsdk::BUNDLES), fs::Permissions::from_mode(0o644)).unwrap();
    install::remove(&ctx, "mibsdk", false).unwrap();
    assert_eq!(mode(&p(&f, mibsdk::BUNDLES)), 0o777);
}

#[test]
fn sdk_install_migrates_old_layout_state() {
    let (f, dir) = unit();
    let base = mibsdk::BASE;
    write(&p(&f, &format!("{base}/data/hello.properties")), b"greeting=hi\n");
    write(&p(&f, &format!("{base}/ledger.properties")), ENTRY.as_bytes());
    write(&p(&f, &format!("{base}/cache/hello-1.jar")), b"jar");
    write(&p(&f, &format!("{base}/boot.state")), b"unhealthyBoots=1\n");
    let sys = RecordingSys::default();
    let ctx = Ctx::new(f.layout.clone(), &sys);
    install_sdk(&ctx, dir.path(), "0.1.0").unwrap();
    assert_eq!(fs::read(p(&f, &format!("{}/data/hello.properties", mibsdk::PERSIST))).unwrap(), b"greeting=hi\n");
    assert_eq!(fs::read_to_string(p(&f, mibsdk::LEDGER)).unwrap(), ENTRY);
    for name in ["data", "ledger.properties", "cache", "boot.state"] {
        assert!(!p(&f, &format!("{base}/{name}")).exists(), "{name}");
    }
}

// was: test_reinstall_moves_state_left_on_app_by_an_older_install
#[test]
fn sdk_update_moves_state_left_on_app_by_an_older_install() {
    let (f, dir) = unit();
    let sys = RecordingSys::default();
    let ctx = Ctx::new(f.layout.clone(), &sys);
    install_sdk(&ctx, dir.path(), "0.1.0").unwrap();
    let base = mibsdk::BASE;
    write(&p(&f, &format!("{base}/data/bike-rack.properties")), b"active=false\n");
    write(&p(&f, &format!("{base}/ledger.properties")), b"");
    write(&p(&f, &format!("{base}/boot.state")), b"unhealthyBoots=1\n");
    write(&p(&f, &format!("{base}/cache/hello-1.jar")), b"jar");
    install_sdk(&ctx, dir.path(), "0.2.0").unwrap();
    assert_eq!(
        fs::read(p(&f, &format!("{}/data/bike-rack.properties", mibsdk::PERSIST))).unwrap(),
        b"active=false\n"
    );
    assert!(p(&f, mibsdk::LEDGER).exists());
    for name in ["cache", "data", "boot.state", "ledger.properties"] {
        assert!(!p(&f, &format!("{base}/{name}")).exists(), "{name}");
    }
}

// was: test_reinstall_uses_the_recorded_stock_state
#[test]
fn sdk_update_uses_the_recorded_stock_state() {
    let (f, dir) = unit();
    tracer_off(&f);
    let sys = RecordingSys::default();
    let ctx = Ctx::new(f.layout.clone(), &sys);
    install_sdk(&ctx, dir.path(), "0.1.0").unwrap();
    // The marker exists now, but the backup says it did not before the first install.
    let msg = install_sdk(&ctx, dir.path(), "0.2.0").unwrap();
    assert_eq!(msg, format!("updated mibsdk 0.1.0 -> 0.2.0{REBOOT_START}"));
    assert!(!autostart(&f).contains("DSITracer"));
    assert!(!sdk_props(&f).contains("chain.configurator"));
    assert_eq!(fs::read(p(&f, mibsdk::BUNDLES)).unwrap(), bundles::generate(&stock(), &spec(), false).unwrap());
}

/// migrate_state on its own: BASE/data and the old ledger, with rw held as deploy holds it.
fn migrate(ctx: &Ctx, ops: &dyn FileOps) -> anyhow::Result<Vec<String>> {
    let _rw = ctx.mounts.rw("/mnt/app").unwrap();
    sdk_pkg::migrate_state_with(ctx, ops)
}

// was: test_reinstall_never_overwrites_persistent_state
#[test]
fn migrate_never_overwrites_persistent_data() {
    let (f, _dir) = unit();
    write(&p(&f, &format!("{}/data/p.properties", mibsdk::PERSIST)), b"new\n");
    write(&p(&f, &format!("{}/data/p.properties", mibsdk::BASE)), b"old\n");
    let sys = RecordingSys::default();
    let ctx = Ctx::new(f.layout.clone(), &sys);
    migrate(&ctx, &StdOps).unwrap();
    assert_eq!(fs::read(p(&f, &format!("{}/data/p.properties", mibsdk::PERSIST))).unwrap(), b"new\n");
    assert!(!p(&f, &format!("{}/data", mibsdk::BASE)).exists());
}

// was: test_old_ledger_entries_are_merged_into_an_existing_ledger
#[test]
fn migrate_merges_old_ledger_into_existing() {
    let (f, _dir) = unit();
    write(&p(&f, mibsdk::LEDGER), OTHER.as_bytes());
    write(&p(&f, mibsdk::OLD_LEDGER), ENTRY.as_bytes());
    let sys = RecordingSys::default();
    let ctx = Ctx::new(f.layout.clone(), &sys);
    assert!(migrate(&ctx, &StdOps).unwrap().is_empty());
    assert_eq!(fs::read_to_string(p(&f, mibsdk::LEDGER)).unwrap(), format!("{OTHER}{ENTRY}"));
    assert!(!p(&f, mibsdk::OLD_LEDGER).exists(), "merged and verified: the old ledger goes");
}

#[test]
fn migrate_reads_the_old_ledgers_tmp_file_when_the_ledger_is_gone() {
    let (f, _dir) = unit();
    write(&p(&f, &format!("{}.tmp", mibsdk::OLD_LEDGER)), ENTRY.as_bytes());
    let sys = RecordingSys::default();
    let ctx = Ctx::new(f.layout.clone(), &sys);
    migrate(&ctx, &StdOps).unwrap();
    assert_eq!(fs::read_to_string(p(&f, mibsdk::LEDGER)).unwrap(), ENTRY);
    assert!(!p(&f, &format!("{}.tmp", mibsdk::OLD_LEDGER)).exists());
}

// was: test_old_ledger_entry_that_conflicts_is_kept_where_it_is
#[test]
fn migrate_keeps_a_conflicting_old_ledger() {
    let (f, _dir) = unit();
    let newer = "parking.soundDirections=bike-rack|PERSISTENT|F0R0L0X0|F1R1L1X1|A\n";
    write(&p(&f, mibsdk::LEDGER), newer.as_bytes());
    write(&p(&f, mibsdk::OLD_LEDGER), ENTRY.as_bytes());
    let sys = RecordingSys::default();
    let ctx = Ctx::new(f.layout.clone(), &sys);
    let notes = migrate(&ctx, &StdOps).unwrap();
    assert_eq!(
        notes,
        vec![
            format!(
                "WARNING: kept {}: these overrides conflict with {} and were not merged:",
                mibsdk::OLD_LEDGER,
                mibsdk::LEDGER
            ),
            format!("  {}", ENTRY.trim()),
        ]
    );
    assert_eq!(fs::read_to_string(p(&f, mibsdk::LEDGER)).unwrap(), newer);
    assert_eq!(fs::read_to_string(p(&f, mibsdk::OLD_LEDGER)).unwrap(), ENTRY, "not merged: never deleted");
}

/// StdOps, except that a rename whose source path contains `needle` fails, either with an
/// error or (QNX mv) "successfully" without moving anything.
struct FailRename {
    needle: &'static str,
    silent: bool,
}

impl FileOps for FailRename {
    fn write(&self, p: &Path, b: &[u8], mode: u32) -> io::Result<()> {
        StdOps.write(p, b, mode)
    }
    fn read(&self, p: &Path) -> io::Result<Vec<u8>> {
        StdOps.read(p)
    }
    fn rename(&self, a: &Path, b: &Path) -> io::Result<()> {
        if a.to_string_lossy().contains(self.needle) {
            return if self.silent { Ok(()) } else { Err(io::Error::other("injected failure")) };
        }
        StdOps.rename(a, b)
    }
    fn remove(&self, p: &Path) -> io::Result<()> {
        StdOps.remove(p)
    }
}

// was: test_failed_ledger_copy_loses_nothing_and_a_rerun_migrates_it
#[test]
fn migrate_failed_ledger_write_loses_nothing_and_a_rerun_migrates_it() {
    let (f, _dir) = unit();
    write(&p(&f, mibsdk::OLD_LEDGER), ENTRY.as_bytes());
    let sys = RecordingSys::default();
    let ctx = Ctx::new(f.layout.clone(), &sys);
    let e = format!("{:#}", migrate(&ctx, &FailRename { needle: "persist/mibsdk/.mibpkg.tmp", silent: false }).unwrap_err());
    assert!(e.contains("(nothing was lost; run the install again)"), "{e}");
    assert_eq!(fs::read_to_string(p(&f, mibsdk::OLD_LEDGER)).unwrap(), ENTRY);
    assert!(!p(&f, mibsdk::LEDGER).exists());
    assert!(!p(&f, &format!("{}/.mibpkg.tmp", mibsdk::PERSIST)).exists(), "no temp file left behind");
    migrate(&ctx, &StdOps).unwrap();
    assert_eq!(fs::read_to_string(p(&f, mibsdk::LEDGER)).unwrap(), ENTRY);
    assert!(!p(&f, mibsdk::OLD_LEDGER).exists());
}

// was: test_failed_data_copy_loses_nothing_and_a_rerun_migrates_it
#[test]
fn migrate_failed_data_copy_loses_nothing_and_a_rerun_migrates_it() {
    let (f, _dir) = unit();
    let (old, new) = (format!("{}/data", mibsdk::BASE), format!("{}/data", mibsdk::PERSIST));
    write(&p(&f, &format!("{old}/a.properties")), b"a=1\n");
    write(&p(&f, &format!("{old}/b.properties")), b"b=2\n");
    let sys = RecordingSys::default();
    let ctx = Ctx::new(f.layout.clone(), &sys);
    let e = format!("{:#}", migrate(&ctx, &FailRename { needle: "b.properties.migrate.tmp", silent: false }).unwrap_err());
    assert!(e.contains("(nothing was lost; run the install again)"), "{e}");
    assert_eq!(fs::read(p(&f, &format!("{old}/a.properties"))).unwrap(), b"a=1\n");
    assert_eq!(fs::read(p(&f, &format!("{old}/b.properties"))).unwrap(), b"b=2\n");
    assert!(!p(&f, &format!("{new}/b.properties")).exists(), "never a partial file");
    migrate(&ctx, &StdOps).unwrap();
    assert_eq!(fs::read(p(&f, &format!("{new}/a.properties"))).unwrap(), b"a=1\n");
    assert_eq!(fs::read(p(&f, &format!("{new}/b.properties"))).unwrap(), b"b=2\n");
    let tmps: Vec<String> = fs::read_dir(p(&f, &new))
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
        .filter(|n| n.ends_with(".tmp"))
        .collect();
    assert!(tmps.is_empty(), "{tmps:?}");
    assert!(!p(&f, &old).exists());
}

// was: test_data_move_that_fails_with_status_0_keeps_the_old_data
#[test]
fn migrate_data_move_that_silently_fails_keeps_the_old_data() {
    let (f, _dir) = unit();
    let (old, new) = (format!("{}/data", mibsdk::BASE), format!("{}/data", mibsdk::PERSIST));
    write(&p(&f, &format!("{old}/b.properties")), b"b=2\n");
    let sys = RecordingSys::default();
    let ctx = Ctx::new(f.layout.clone(), &sys);
    assert!(migrate(&ctx, &FailRename { needle: "b.properties.migrate.tmp", silent: true }).is_err());
    assert_eq!(fs::read(p(&f, &format!("{old}/b.properties"))).unwrap(), b"b=2\n");
    migrate(&ctx, &StdOps).unwrap();
    assert_eq!(fs::read(p(&f, &format!("{new}/b.properties"))).unwrap(), b"b=2\n");
    assert!(!p(&f, &old).exists());
}

// ---- install refusals (check) ----

#[test]
fn sdk_install_refusals_change_nothing() {
    type Setup = fn(&common::Fixture);
    let cases: Vec<(&str, Setup)> = vec![
        ("unsupported HMI 'HMI_OTHER' (supported: HMI_AU_MIB2_HQC_NAR_G22_C10_180803)", |f| {
            write(&p(f, mibsdk::VERSION_INFO), b"HMI = HMI_OTHER\n")
        }),
        ("carplay-rgi is installed", |f| write(&p(f, &format!("{}/CarPlay-RGI.jar", mibsdk::JARS)), b"x")),
        ("carplay-rgi is installed", |f| {
            write(&p(f, &format!("{}/lsd/kzbs/foo.kzb.carplay-stock", mibsdk::HMI_ROOT)), b"x")
        }),
        ("spike jar mibsdk-spike-ui.jar is installed; run spikes/hmi-ui/run.sh uninstall first", |f| {
            jar_with(&p(f, &format!("{}/mibsdk-spike-ui.jar", mibsdk::JARS)), &["a/B.class"])
        }),
        ("other.jar defines de/audi/tghu/car/hmi/Activator.class, which the SDK owns", |f| {
            jar_with(&p(f, &format!("{}/other.jar", mibsdk::JARS)), &["de/audi/tghu/car/hmi/Activator.class"])
        }),
        ("HMICar is not stock in the stock bundles file (HMICar=x.Y)", |f| {
            let b = String::from_utf8(stock()).unwrap().replace(HMICAR_STOCK, "HMICar=x.Y");
            write(&p(f, mibsdk::BUNDLES), b.as_bytes())
        }),
        (
            "/mnt/app/eso/hmi/lsd/DSITracer.jar is missing; the SDK chains DSITracer's configurator and needs it",
            |f| fs::remove_file(p(f, mibsdk::TRACER_JAR)).unwrap(),
        ),
    ];
    for (want, setup) in cases {
        let (f, dir) = unit();
        setup(&f);
        assert_refused_untouched(&f, dir.path(), want);
    }
}

// was: test_refuses_when_carplay_stock_files_exist_anywhere_under_hmi
#[test]
fn check_refuses_carplay_stock_files_anywhere_under_hmi() {
    let (f, dir) = unit();
    write(&p(&f, "/mnt/app/eso/hmi/lsd/kzbs/deep/foo.kzb.carplay-stock"), b"x");
    assert_refused_untouched(&f, dir.path(), "carplay-rgi is installed (*.carplay-stock files exist); the SDK does not support it (spec §13)");
}

#[test]
fn check_fails_closed_when_hmi_cannot_be_searched() {
    let (f, dir) = unit();
    let locked = p(&f, "/mnt/app/eso/hmi/locked");
    fs::create_dir_all(&locked).unwrap();
    fs::set_permissions(&locked, fs::Permissions::from_mode(0o000)).unwrap();
    let readable = fs::read_dir(&locked).is_ok(); // running as root
    if !readable {
        let sys = RecordingSys::default();
        let ctx = Ctx::new(f.layout.clone(), &sys);
        let e = err(install_sdk(&ctx, dir.path(), "0.1.0"));
        assert!(e.contains("could not verify the absence of carplay-rgi"), "{e}");
        assert!(!backups::has_backup(&f.layout, mibsdk::BUNDLES));
    }
    fs::set_permissions(&locked, fs::Permissions::from_mode(0o755)).unwrap();
}

// was: test_refuses_jar_defining_owned_class
#[test]
fn check_refuses_jar_defining_owned_class() {
    for cls in [
        "de/audi/tghu/car/hmi/Activator.class",
        "de/audi/atip/model/CarModelBank.class",
        "de/audi/atip/model/CarModelBank$1.class",
    ] {
        let (f, dir) = unit();
        jar_with(&p(&f, &format!("{}/aa-clean.jar", mibsdk::JARS)), &["a/B.class"]);
        jar_with(&p(&f, &format!("{}/zz-other-mod.jar", mibsdk::JARS)), &["a/B.class", cls]);
        assert_refused_untouched(&f, dir.path(), &format!("zz-other-mod.jar defines {cls}, which the SDK owns; nothing changed"));
    }
}

// was: test_ignores_sdk_jars_in_ownership_guard
#[test]
fn check_ignores_sdk_jars_in_ownership_guard() {
    let (f, dir) = unit();
    jar_with(&p(&f, &format!("{}/mibsdk-adapter-hqc.jar", mibsdk::JARS)), &["de/audi/tghu/car/hmi/Activator.class"]);
    jar_with(&p(&f, &format!("{}/other-mod.jar", mibsdk::JARS)), &["de/audi/tghu/car/hmiextra/X.class", "a/B.txt"]);
    let sys = RecordingSys::default();
    let ctx = Ctx::new(f.layout.clone(), &sys);
    install_sdk(&ctx, dir.path(), "0.1.0").unwrap();
}

// was: test_refuses_while_spike_jar_present
#[test]
fn check_refuses_while_spike_jar_present() {
    let (f, dir) = unit();
    jar_with(&p(&f, &format!("{}/mibsdk-spike-ui.jar", mibsdk::JARS)), &["mibsdk/spike/ui/SpikeCarActivator.class"]);
    assert_refused_untouched(
        &f,
        dir.path(),
        "spike jar mibsdk-spike-ui.jar is installed; run spikes/hmi-ui/run.sh uninstall first; nothing changed",
    );
}

// was: test_guard_checks_every_jar_not_just_the_first
#[test]
fn check_guard_checks_every_jar_not_just_the_first() {
    let (f, dir) = unit();
    for n in ["a1", "b2", "c3"] {
        jar_with(&p(&f, &format!("{}/{n}.jar", mibsdk::JARS)), &["x/Y.class"]);
    }
    jar_with(&p(&f, &format!("{}/zz-last.jar", mibsdk::JARS)), &["de/audi/tghu/car/hmi/Z.class"]);
    assert_refused_untouched(&f, dir.path(), "zz-last.jar defines de/audi/tghu/car/hmi/Z.class");
}

// was: test_guard_only_looks_at_jars
#[test]
fn check_guard_only_looks_at_jars() {
    let (f, dir) = unit();
    write(&p(&f, &format!("{}/lang_data.zip", mibsdk::JARS)), b"not a zip");
    write(&p(&f, &format!("{}/old.jar.bak", mibsdk::JARS)), b"not a zip");
    fs::create_dir_all(p(&f, &format!("{}/dir.jar", mibsdk::JARS))).unwrap();
    let sys = RecordingSys::default();
    let ctx = Ctx::new(f.layout.clone(), &sys);
    install_sdk(&ctx, dir.path(), "0.1.0").unwrap();
}

// was: test_guard_fails_closed_on_an_unreadable_jar
#[test]
fn check_guard_fails_closed_on_an_unreadable_jar() {
    let (f, dir) = unit();
    write(&p(&f, &format!("{}/broken.jar", mibsdk::JARS)), b"not a zip");
    assert_refused_untouched(&f, dir.path(), "cannot list /mnt/app/eso/hmi/lsd/jars/broken.jar (not a zip?: no end of central directory record");
}

// was: test_guard_refuses_a_symlinked_jar
#[test]
fn check_guard_refuses_a_symlinked_jar() {
    let (f, dir) = unit();
    std::os::unix::fs::symlink(p(&f, &format!("{}/stock.jar", mibsdk::JARS)), p(&f, &format!("{}/link.jar", mibsdk::JARS)))
        .unwrap();
    assert_refused_untouched(
        &f,
        dir.path(),
        "/mnt/app/eso/hmi/lsd/jars/link.jar is a symlink; the ownership check cannot follow it",
    );
}

// was: test_guard_refuses_a_jar_name_with_a_space
#[test]
fn check_guard_refuses_a_jar_name_with_a_space() {
    let (f, dir) = unit();
    jar_with(&p(&f, &format!("{}/my mod.jar", mibsdk::JARS)), &["x/Y.class"]);
    assert_refused_untouched(&f, dir.path(), "has a space in its name; the ownership check cannot handle it");
}

// was: test_nonstock_hmicar_changes_nothing_at_all
#[test]
fn sdk_install_nonstock_hmicar_changes_nothing_at_all() {
    for bad in [
        "HMICar=x.Y",
        "HMICar = de.audi.tghu.car.hmi.Activator",
        "HMICar=de.audi.tghu.car.hmi.Activator ",
        HMICAR_SDK,
    ] {
        let (f, dir) = unit();
        let b = String::from_utf8(stock()).unwrap().replace(HMICAR_STOCK, bad);
        write(&p(&f, mibsdk::BUNDLES), b.as_bytes());
        assert_refused_untouched(&f, dir.path(), "HMICar is not stock");
    }
}

// was: test_refuses_a_nonstock_hmicar_line
#[test]
fn sdk_install_refuses_a_nonstock_hmicar_line() {
    let (f, dir) = unit();
    let b = String::from_utf8(stock()).unwrap().replace(HMICAR_STOCK, "HMICar=x.Y");
    write(&p(&f, mibsdk::BUNDLES), b.as_bytes());
    let sys = RecordingSys::default();
    let ctx = Ctx::new(f.layout.clone(), &sys);
    assert!(install_sdk(&ctx, dir.path(), "0.1.0").is_err());
    assert!(!fs::read_to_string(p(&f, mibsdk::BUNDLES)).unwrap().contains("mibsdk.adapter.hqc.ui"));
    assert!(!p(&f, mibsdk::BASE).exists());
}

#[test]
fn sdk_install_refuses_generated_bundles_without_backup() {
    // A list generated by an old tool with the stock HMICar= line: check passes, the backup refuses.
    let (f, dir) = unit();
    let gen = String::from_utf8(bundles::generate(&stock(), &spec(), true).unwrap()).unwrap().replace(HMICAR_SDK, HMICAR_STOCK);
    write(&p(&f, mibsdk::BUNDLES), gen.as_bytes());
    let before = tree(f.root());
    let sys = RecordingSys::default();
    let ctx = Ctx::new(f.layout.clone(), &sys);
    let e = err(install_sdk(&ctx, dir.path(), "0.1.0"));
    assert!(
        e.contains(
            "/mnt/app/eso/hmi/lsd/bundles_dsitracer_enabled.properties already references mibsdk but no stock backup exists; restore the stock file by hand"
        ),
        "{e}"
    );
    assert_eq!(fs::read(p(&f, mibsdk::BUNDLES)).unwrap(), gen.as_bytes());
    assert!(!backups::has_backup(&f.layout, mibsdk::BUNDLES));
    assert!(!backups::has_backup(&f.layout, mibsdk::MARKER));
    assert!(sdk_jars_present(&f).is_empty());
    assert!(install::current_version(&f.layout, "mibsdk").is_none());
    let after = tree(f.root());
    let changed: Vec<_> = after.keys().filter(|k| before.get(*k) != after.get(*k)).filter(|k| {
        // mibpkg's own staging (pkgs/mibsdk/<version>.new is created, then removed).
        *k != Path::new("mnt/app/root/mibpkg") && !k.starts_with("mnt/app/root/mibpkg/pkgs")
    })
    .collect();
    assert!(changed.is_empty(), "{changed:?}");
    // With the SDK's HMICar= line live and no backup, check already refuses.
    let (f, dir) = unit();
    write(&p(&f, mibsdk::BUNDLES), &bundles::generate(&stock(), &spec(), true).unwrap());
    assert_refused_untouched(&f, dir.path(), "HMICar is not stock in the stock bundles file");
}

#[test]
fn sdk_install_refuses_unstamped_jars() {
    let (f, dir) = unit();
    let src = sdk_src(dir.path(), "0.1.0");
    write_stamp(&src, "mibsdk-unit.stamp", "unit", &[]);
    let sys = RecordingSys::default();
    let ctx = Ctx::new(f.layout.clone(), &sys);
    let e = err(install_src(&ctx, &src));
    assert!(e.contains("not a checked build (no hash for mibsdk-runtime.jar)"), "{e}");
    assert!(e.contains("nothing changed (M7)"), "{e}");
    assert_eq!(fs::read(p(&f, mibsdk::BUNDLES)).unwrap(), stock());
    assert!(sdk_jars_present(&f).is_empty());
    assert!(!backups::has_backup(&f.layout, mibsdk::BUNDLES));
    assert!(install::current_version(&f.layout, "mibsdk").is_none());
    // A jar changed after stamping.
    fs::write(src.join("mibsdk-adapter-hqc.jar"), "rebuilt without the checks").unwrap();
    write_stamp(&src, "mibsdk-unit.stamp", "unit", &SDK_JARS);
    fs::write(src.join("mibsdk-adapter-hqc.jar"), "tampered").unwrap();
    let e = err(install_src(&ctx, &src));
    assert!(e.contains("hash mismatch for mibsdk-adapter-hqc.jar"), "{e}");
}

#[test]
fn sdk_install_refuses_plugin_kind_stamp() {
    let (f, dir) = unit();
    let src = sdk_src(dir.path(), "0.1.0");
    write_stamp(&src, "mibsdk-unit.stamp", "plugin", &SDK_JARS);
    let sys = RecordingSys::default();
    let ctx = Ctx::new(f.layout.clone(), &sys);
    let e = err(install_src(&ctx, &src));
    assert!(e.contains("not a checked build (kind)"), "{e}");
    assert!(sdk_jars_present(&f).is_empty());
}

// ---- all-or-undo ----

#[test]
fn sdk_deploy_failure_undoes_fresh_install() {
    let (f, dir) = unit();
    tracer_off(&f);
    fs::create_dir_all(p(&f, &format!("{}/mibsdk-adapter-hqc.jar", mibsdk::JARS))).unwrap();
    let sys = RecordingSys::default();
    let ctx = Ctx::new(f.layout.clone(), &sys);
    assert!(install_sdk(&ctx, dir.path(), "0.1.0").is_err());
    assert_eq!(fs::read(p(&f, mibsdk::BUNDLES)).unwrap(), stock());
    assert_eq!(mode(&p(&f, mibsdk::BUNDLES)), 0o777);
    assert!(sdk_jars_present(&f).is_empty(), "{:?}", sdk_jars_present(&f));
    assert!(!p(&f, mibsdk::MARKER).exists(), "it was absent before");
    assert!(!backups::has_backup(&f.layout, mibsdk::BUNDLES));
    assert!(install::current_version(&f.layout, "mibsdk").is_none());
    assert!(!f.layout.pkg_dir("mibsdk").join("0.1.0").exists());
    assert!(State::load(&f.layout).unwrap().packages.is_empty());
}

#[test]
fn sdk_update_failure_redeploys_previous_version() {
    let (f, dir) = unit();
    let sys = RecordingSys::default();
    let ctx = Ctx::new(f.layout.clone(), &sys);
    let v1 = sdk_src(dir.path(), "0.1.0");
    install_src(&ctx, &v1).unwrap();
    let installed = fs::read(p(&f, mibsdk::BUNDLES)).unwrap();
    // 0.2.0 adds a jar whose target is a directory: the write fails after the three jars changed.
    let mut jars = SDK_JARS.to_vec();
    jars.push("mibsdk-extra.jar");
    let v2 = sdk_src_with(dir.path(), "0.2.0", &jars, 1, 2);
    fs::create_dir_all(p(&f, &format!("{}/mibsdk-extra.jar", mibsdk::JARS))).unwrap();
    assert!(install_src(&ctx, &v2).is_err());
    for j in SDK_JARS {
        assert_eq!(fs::read(p(&f, &format!("{}/{j}", mibsdk::JARS))).unwrap(), fs::read(v1.join(j)).unwrap(), "{j}");
    }
    assert_eq!(fs::read(p(&f, mibsdk::BUNDLES)).unwrap(), installed);
    assert_eq!(install::current_version(&f.layout, "mibsdk").as_deref(), Some("0.1.0"));
    assert!(backups::has_backup(&f.layout, mibsdk::BUNDLES), "the stock backup stays");
    assert!(!f.layout.pkg_dir("mibsdk").join("0.2.0").exists());
}

// ---- update / rollback api range ----

#[test]
fn sdk_update_refused_while_plugin_needs_other_api() {
    let (f, dir) = unit();
    let sys = RecordingSys::default();
    let ctx = Ctx::new(f.layout.clone(), &sys);
    // Installed while an SDK supporting api 3 was.
    install_src(&ctx, &sdk_src_with(dir.path(), "0.1.0", &SDK_JARS, 1, 3)).unwrap();
    install_plugin(&ctx, dir.path(), "bike-rack", 3);
    let e = err(install_sdk(&ctx, dir.path(), "0.2.0"));
    assert_eq!(e, "bike-rack needs plugin api 3; this SDK supports 1..2; nothing changed");
    assert_eq!(install::current_version(&f.layout, "mibsdk").as_deref(), Some("0.1.0"));
}

#[test]
fn sdk_update_accepts_api_1_plugin_on_api_2_sdk() {
    let (f, dir) = unit();
    let sys = RecordingSys::default();
    let ctx = Ctx::new(f.layout.clone(), &sys);
    install_sdk(&ctx, dir.path(), "0.1.0").unwrap();
    install_plugin(&ctx, dir.path(), "bike-rack", 1);
    install_sdk(&ctx, dir.path(), "0.2.0").unwrap();
    assert_eq!(install::current_version(&f.layout, "mibsdk").as_deref(), Some("0.2.0"));
}

#[test]
fn sdk_rollback_redeploys_the_previous_jars() {
    let (f, dir) = unit();
    let sys = RecordingSys::default();
    let ctx = Ctx::new(f.layout.clone(), &sys);
    let v1 = sdk_src(dir.path(), "0.1.0");
    install_src(&ctx, &v1).unwrap();
    install_sdk(&ctx, dir.path(), "0.2.0").unwrap();
    let msg = install::rollback(&ctx, "mibsdk").unwrap();
    assert_eq!(msg, format!("rolled back mibsdk 0.2.0 -> 0.1.0{REBOOT_START}"));
    for j in SDK_JARS {
        assert_eq!(fs::read(p(&f, &format!("{}/{j}", mibsdk::JARS))).unwrap(), fs::read(v1.join(j)).unwrap(), "{j}");
    }
}

#[test]
fn sdk_rollback_refused_while_plugin_needs_other_api() {
    let (f, dir) = unit();
    let sys = RecordingSys::default();
    let ctx = Ctx::new(f.layout.clone(), &sys);
    install_src(&ctx, &sdk_src_with(dir.path(), "0.1.0", &SDK_JARS, 1, 1)).unwrap();
    install_sdk(&ctx, dir.path(), "0.2.0").unwrap();
    install_plugin(&ctx, dir.path(), "bike-rack", 2);
    let e = format!("{:#}", install::rollback(&ctx, "mibsdk").unwrap_err());
    assert_eq!(e, "bike-rack needs plugin api 2; this SDK supports 1..1; nothing changed");
    assert_eq!(install::current_version(&f.layout, "mibsdk").as_deref(), Some("0.2.0"));
}

// ---- HMICar= on update ----

// was: test_upgrade_refuses_a_foreign_live_hmicar
#[test]
fn sdk_update_refuses_a_foreign_live_hmicar() {
    let (f, dir) = unit();
    let sys = RecordingSys::default();
    let ctx = Ctx::new(f.layout.clone(), &sys);
    install_sdk(&ctx, dir.path(), "0.1.0").unwrap();
    let b = fs::read_to_string(p(&f, mibsdk::BUNDLES)).unwrap().replace("mibsdk.adapter.hqc.ui.SdkCarActivator", "other.Act");
    write(&p(&f, mibsdk::BUNDLES), b.as_bytes());
    let before = tree(f.root());
    let e = err(install_sdk(&ctx, dir.path(), "0.2.0"));
    assert!(e.contains("HMICar is not stock in the installed /mnt/app/eso/hmi/lsd/bundles_dsitracer_enabled.properties"), "{e}");
    let after = tree(f.root());
    let changed: Vec<_> = after.keys().filter(|k| before.get(*k) != after.get(*k)).collect();
    assert!(changed.is_empty(), "{changed:?}");
}

// was: test_upgrade_with_the_sdk_value_live_is_fine
#[test]
fn sdk_update_with_the_sdk_value_live_is_fine() {
    let (f, dir) = unit();
    let sys = RecordingSys::default();
    let ctx = Ctx::new(f.layout.clone(), &sys);
    install_sdk(&ctx, dir.path(), "0.1.0").unwrap();
    install_sdk(&ctx, dir.path(), "0.2.0").unwrap();
}

// was: test_refuses_a_backup_that_already_holds_the_sdk_value
#[test]
fn sdk_update_refuses_a_backup_that_already_holds_the_sdk_value() {
    let (f, dir) = unit();
    let sys = RecordingSys::default();
    let ctx = Ctx::new(f.layout.clone(), &sys);
    install_sdk(&ctx, dir.path(), "0.1.0").unwrap();
    let bk = backups::backup_path(&f.layout, mibsdk::BUNDLES);
    let b = fs::read_to_string(&bk).unwrap().replace(HMICAR_STOCK, HMICAR_SDK);
    fs::write(&bk, b).unwrap();
    let e = err(install_sdk(&ctx, dir.path(), "0.2.0"));
    assert!(e.contains("HMICar is not stock in the stock bundles file"), "{e}");
}

// ---- uninstall ----

fn installed(ledger: Option<&str>) -> (common::Fixture, tempfile::TempDir) {
    let (f, dir) = unit();
    tracer_off(&f);
    let sys = RecordingSys::default();
    let ctx = Ctx::new(f.layout.clone(), &sys);
    install_sdk(&ctx, dir.path(), "0.1.0").unwrap();
    if let Some(l) = ledger {
        write(&p(&f, mibsdk::LEDGER), l.as_bytes());
    }
    (f, dir)
}

fn assert_still_installed(f: &common::Fixture) {
    assert!(autostart(f).contains("MibSdk"));
    assert_eq!(sdk_jars_present(f), SDK_JARS.to_vec());
    assert!(p(f, mibsdk::BASE).is_dir());
    assert!(State::load(&f.layout).unwrap().packages.contains_key("mibsdk"));
}

// was: test_refuses_while_an_override_is_in_force
// was: test_force_uninstalls_with_a_loud_warning
#[test]
fn sdk_remove_refuses_on_ledger_entries_unless_force() {
    for purge in [false, true] {
        let (f, _dir) = installed(Some(ENTRY));
        let sys = RecordingSys::default();
        let ctx = Ctx::new(f.layout.clone(), &sys);
        let e = format!("{:#}", install::remove(&ctx, "mibsdk", purge).unwrap_err());
        let lines: Vec<&str> = e.lines().collect();
        assert_eq!(
            lines,
            vec![
                "refusing to uninstall: these overrides of vehicle state are still in force (key=plugin|scope|prior|active|A or R):",
                "  parking.soundDirections=bike-rack|PERSISTENT|F1R1L1X1|F0R0L0X0|A",
                "let the SDK put the prior values back first: mibpkg sdk safe-mode on, then restart the HMI",
                "(safe mode loads no plugin and restores every override in the ledger; check /tmp/mibsdk.log), then run the removal again.",
                "after a later reinstall, leave safe mode with: mibpkg sdk safe-mode off",
                "to uninstall anyway and leave those settings overridden, add --force",
            ],
            "purge={purge}"
        );
        assert_still_installed(&f);
    }
    let (f, _dir) = installed(Some(ENTRY));
    let sys = RecordingSys::default();
    let mut ctx = Ctx::new(f.layout.clone(), &sys);
    ctx.force = true;
    let msg = install::remove(&ctx, "mibsdk", true).unwrap();
    let lines: Vec<&str> = msg.lines().collect();
    assert_eq!(lines[0], "WARNING: --force: uninstalling while these overrides of vehicle state are still in force:");
    assert_eq!(lines[1], "  parking.soundDirections=bike-rack|PERSISTENT|F1R1L1X1|F0R0L0X0|A");
    assert_eq!(
        lines[2],
        "WARNING: nothing will restore their prior values; the car keeps the overridden settings until you change them back by hand (format: key=plugin|scope|prior|active|A or R)."
    );
    assert!(msg.ends_with(REBOOT_REMOVE), "{msg}");
    assert!(!autostart(&f).contains("MibSdk"));
    assert!(!p(&f, mibsdk::BASE).exists());
    assert!(!p(&f, mibsdk::PERSIST).exists(), "--purge removes the plugin data too");
}

// was: test_refusal_shows_a_pending_restore_line
#[test]
fn sdk_remove_refusal_shows_a_pending_restore_line() {
    let line = "parking.soundDirections=bike-rack|PERSISTENT|F1R1L1X1|F0R0L0X0|R";
    let (f, _dir) = installed(Some(&format!("{line}\n")));
    let sys = RecordingSys::default();
    let ctx = Ctx::new(f.layout.clone(), &sys);
    let e = format!("{:#}", install::remove(&ctx, "mibsdk", false).unwrap_err());
    assert!(e.contains(&format!("  {line}\n")), "{e}");
    assert_still_installed(&f);
}

// was: test_refuses_on_an_override_left_in_the_old_ledger_location
#[test]
fn sdk_remove_refuses_on_an_override_in_the_old_ledger_location() {
    let (f, _dir) = installed(None);
    write(&p(&f, mibsdk::OLD_LEDGER), ENTRY.as_bytes());
    let sys = RecordingSys::default();
    let ctx = Ctx::new(f.layout.clone(), &sys);
    let e = format!("{:#}", install::remove(&ctx, "mibsdk", false).unwrap_err());
    assert!(e.contains("parking.soundDirections=bike-rack|PERSISTENT|F1R1L1X1|F0R0L0X0|A"), "{e}");
    assert_still_installed(&f);
}

// was: test_empty_or_missing_ledger_uninstalls
#[test]
fn sdk_remove_with_empty_or_missing_ledger_needs_no_force() {
    for ledger in [None, Some(""), Some("\n")] {
        let (f, _dir) = installed(ledger);
        let sys = RecordingSys::default();
        let ctx = Ctx::new(f.layout.clone(), &sys);
        let msg = install::remove(&ctx, "mibsdk", false).unwrap();
        assert!(!msg.contains("WARNING"), "{msg}");
        assert!(!autostart(&f).contains("MibSdk"));
        assert!(!p(&f, mibsdk::MARKER).exists(), "it was absent before the install");
    }
}

#[test]
fn sdk_remove_refuses_with_plugins_installed() {
    let (f, dir) = installed(None);
    let sys = RecordingSys::default();
    let mut ctx = Ctx::new(f.layout.clone(), &sys);
    install_plugin(&ctx, dir.path(), "bike-rack", 2);
    install_plugin(&ctx, dir.path(), "hello", 2);
    ctx.force = true; // --force does not cover installed plugin packages
    let e = format!("{:#}", install::remove(&ctx, "mibsdk", false).unwrap_err());
    assert!(e.starts_with("remove these plugins first: bike-rack, hello"), "{e}");
    assert_still_installed(&f);
}

#[test]
fn sdk_remove_refuses_with_pushed_plugins_unless_force() {
    let (f, _dir) = installed(None);
    write(&p(&f, &format!("{}/plugins/hello.jar", mibsdk::BASE)), b"pushed");
    let sys = RecordingSys::default();
    let mut ctx = Ctx::new(f.layout.clone(), &sys);
    let e = format!("{:#}", install::remove(&ctx, "mibsdk", false).unwrap_err());
    assert!(
        e.starts_with("these plugins were pushed by mibsdk-dev: hello; remove them first (mibpkg sdk remove <id>) or add --force"),
        "{e}"
    );
    assert_still_installed(&f);
    ctx.force = true;
    install::remove(&ctx, "mibsdk", false).unwrap();
    assert!(!autostart(&f).contains("MibSdk"));
    assert!(p(&f, &format!("{}/plugins/hello.jar", mibsdk::BASE)).exists(), "plugins/ is kept without --purge");
}

// was: test_uninstall_restores_hmicar_byte_identical
#[test]
fn sdk_remove_restores_stock_bundles_byte_for_byte_and_mode_keeps_persist_grants_and_plugins_dir() {
    let (f, _dir) = installed(None);
    assert_ne!(fs::read(p(&f, mibsdk::BUNDLES)).unwrap(), stock());
    write(&p(&f, mibsdk::GRANTS), b"hello=vehicle.read\n");
    write(&p(&f, &format!("{}/data/hello.properties", mibsdk::PERSIST)), b"x=1\n");
    fs::set_permissions(p(&f, mibsdk::BUNDLES), fs::Permissions::from_mode(0o644)).unwrap();
    fs::remove_file(mibsdk::reboot_flag(&f.layout)).unwrap();
    let sys = RecordingSys::default();
    let ctx = Ctx::new(f.layout.clone(), &sys);
    let msg = install::remove(&ctx, "mibsdk", false).unwrap();
    assert_eq!(
        msg,
        format!("removed mibsdk; kept plugin data in /mnt/persist/mibsdk (remove it with: mibpkg remove mibsdk --purge){REBOOT_REMOVE}")
    );
    assert_eq!(fs::read(p(&f, mibsdk::BUNDLES)).unwrap(), stock());
    assert!(fs::read_to_string(p(&f, mibsdk::BUNDLES)).unwrap().contains(&format!("{HMICAR_STOCK}\r\n")));
    assert_eq!(mode(&p(&f, mibsdk::BUNDLES)), 0o777);
    assert!(sdk_jars_present(&f).is_empty());
    assert!(!p(&f, &format!("{}/sdk.properties", mibsdk::BASE)).exists());
    assert!(!p(&f, mibsdk::PERMISSIONS).exists());
    assert_eq!(fs::read(p(&f, mibsdk::GRANTS)).unwrap(), b"hello=vehicle.read\n");
    assert!(p(&f, &format!("{}/plugins", mibsdk::BASE)).is_dir());
    assert_eq!(fs::read(p(&f, &format!("{}/data/hello.properties", mibsdk::PERSIST))).unwrap(), b"x=1\n");
    assert!(!backups::has_backup(&f.layout, mibsdk::BUNDLES));
    assert!(!backups::has_backup(&f.layout, mibsdk::MARKER));
    assert!(mibsdk::reboot_flag(&f.layout).exists());
    assert!(!State::load(&f.layout).unwrap().packages.contains_key("mibsdk"));
    // stock.jar and DSITracer are not ours.
    assert!(p(&f, &format!("{}/stock.jar", mibsdk::JARS)).exists());
    assert!(p(&f, mibsdk::TRACER_JAR).exists());
}

// was: test_uninstall_restores_the_bundles_file_mode_and_leaves_the_marker_alone
#[test]
fn sdk_remove_restores_mode_and_leaves_an_existing_marker_alone() {
    let (f, dir) = unit();
    let t = std::time::SystemTime::UNIX_EPOCH + std::time::Duration::from_secs(1_000_000_000);
    fs::File::options().write(true).open(p(&f, mibsdk::MARKER)).unwrap().set_modified(t).unwrap();
    let sys = RecordingSys::default();
    let ctx = Ctx::new(f.layout.clone(), &sys);
    install_sdk(&ctx, dir.path(), "0.1.0").unwrap();
    assert_eq!(mode(&p(&f, mibsdk::BUNDLES)), 0o777, "install keeps the mode too");
    install::remove(&ctx, "mibsdk", false).unwrap();
    assert_eq!(fs::read(p(&f, mibsdk::BUNDLES)).unwrap(), stock());
    assert_eq!(mode(&p(&f, mibsdk::BUNDLES)), 0o777);
    assert_eq!(fs::metadata(p(&f, mibsdk::MARKER)).unwrap().modified().unwrap(), t, "the marker existed: never touched");
}

// was: test_default_uninstall_keeps_plugin_data_purge_removes_it
#[test]
fn sdk_remove_purge_removes_base_persist_and_ram() {
    let (f, dir) = installed(None);
    write(&p(&f, &format!("{}/data/bike-rack.properties", mibsdk::PERSIST)), b"active=false\n");
    write(&p(&f, &format!("{}/cache/hello-1.jar", mibsdk::RAM)), b"jar");
    let sys = RecordingSys::default();
    let ctx = Ctx::new(f.layout.clone(), &sys);
    install::remove(&ctx, "mibsdk", false).unwrap();
    assert_eq!(
        fs::read(p(&f, &format!("{}/data/bike-rack.properties", mibsdk::PERSIST))).unwrap(),
        b"active=false\n"
    );
    install_sdk(&ctx, dir.path(), "0.1.0").unwrap();
    let msg = install::remove(&ctx, "mibsdk", true).unwrap();
    assert!(msg.contains("removed /mnt/app/eso/hmi/lsd/mibsdk, /mnt/persist/mibsdk and /ramdisk/mibsdk"), "{msg}");
    assert!(!p(&f, mibsdk::PERSIST).exists());
    assert!(!p(&f, mibsdk::RAM).exists());
    assert!(!p(&f, mibsdk::BASE).exists());
    assert!(p(&f, "/mnt/persist").is_dir(), "only our directory goes");
}

#[test]
fn uninstall_self_refuses_while_mibsdk_installed() {
    let run = |f: &common::Fixture| {
        let args: Vec<String> = ["--root", f.root().to_str().unwrap(), "--no-supervisor", "uninstall-self"]
            .iter()
            .map(|s| s.to_string())
            .collect();
        mibpkg::cli::run(&args)
    };
    let (f, dir) = installed(None);
    let e = format!("{:#}", run(&f).unwrap_err());
    assert!(e.contains("mibsdk"), "{e}");
    assert!(e.contains("mibpkg remove"), "{e}");
    assert!(State::load(&f.layout).unwrap().packages.contains_key("mibsdk"));
    // A plugin package alone refuses too (its SDK's record gone).
    let (f2, d2) = unit();
    let sys = RecordingSys::default();
    let ctx = Ctx::new(f2.layout.clone(), &sys);
    install_sdk(&ctx, d2.path(), "0.1.0").unwrap();
    install_plugin(&ctx, dir.path(), "bike-rack", 2);
    let mut st = State::load(&f2.layout).unwrap();
    st.packages.remove("mibsdk");
    st.save(&f2.layout).unwrap();
    let e = format!("{:#}", run(&f2).unwrap_err());
    assert!(e.contains("bike-rack"), "{e}");
}

// ---- fix round 1 ----

/// A failure after the bundle list was restored keeps both backups and the package, and the
/// retry finishes the removal (Important 1).
#[test]
fn sdk_remove_failure_after_restoring_bundles_keeps_backups_and_a_retry_finishes() {
    let (f, _dir) = installed(None);
    let jar = p(&f, &format!("{}/mibsdk-adapter-hqc.jar", mibsdk::JARS));
    fs::remove_file(&jar).unwrap();
    write(&jar.join("blocker"), b"x"); // a non-empty directory: removing it as a file fails
    let sys = RecordingSys::default();
    let ctx = Ctx::new(f.layout.clone(), &sys);
    assert!(install::remove(&ctx, "mibsdk", false).is_err());
    assert_eq!(fs::read(p(&f, mibsdk::BUNDLES)).unwrap(), stock(), "restored first");
    assert!(backups::has_backup(&f.layout, mibsdk::BUNDLES), "the stock backup is not forgotten");
    assert!(backups::has_backup(&f.layout, mibsdk::MARKER));
    assert!(State::load(&f.layout).unwrap().packages.contains_key("mibsdk"));
    fs::remove_dir_all(&jar).unwrap();
    let msg = install::remove(&ctx, "mibsdk", false).unwrap();
    assert!(msg.ends_with(REBOOT_REMOVE), "{msg}");
    assert_eq!(fs::read(p(&f, mibsdk::BUNDLES)).unwrap(), stock());
    assert!(!p(&f, mibsdk::MARKER).exists());
    assert!(!backups::has_backup(&f.layout, mibsdk::BUNDLES));
    assert!(!backups::has_backup(&f.layout, mibsdk::MARKER));
    assert!(!State::load(&f.layout).unwrap().packages.contains_key("mibsdk"));
}

/// Interrupted after the SDK files and the package dir went but before the state was saved:
/// the retry finishes (live is stock, jars gone, manifest gone).
#[test]
fn sdk_remove_retry_after_the_package_files_went_finishes() {
    let (f, _dir) = installed(None);
    let sys = RecordingSys::default();
    let ctx = Ctx::new(f.layout.clone(), &sys);
    {
        let _rw = ctx.mounts.rw("/mnt/app").unwrap();
        sdk_pkg::undeploy(&ctx, false).unwrap();
    }
    assert!(backups::has_backup(&f.layout, mibsdk::BUNDLES), "undeploy leaves forgetting to its caller");
    fs::remove_dir_all(f.layout.pkg_dir("mibsdk")).unwrap();
    install::remove(&ctx, "mibsdk", false).unwrap();
    assert_eq!(fs::read(p(&f, mibsdk::BUNDLES)).unwrap(), stock());
    assert!(!backups::has_backup(&f.layout, mibsdk::BUNDLES));
    assert!(!State::load(&f.layout).unwrap().packages.contains_key("mibsdk"));
}

/// StdOps, except that reading the backup of the bundle list returns other bytes.
struct BadBackupRead;

impl FileOps for BadBackupRead {
    fn write(&self, p: &Path, b: &[u8], mode: u32) -> io::Result<()> {
        StdOps.write(p, b, mode)
    }
    fn read(&self, p: &Path) -> io::Result<Vec<u8>> {
        let mut b = StdOps.read(p)?;
        if p.to_string_lossy().contains("state/backups") {
            b.extend(b"corrupted");
        }
        Ok(b)
    }
    fn rename(&self, a: &Path, b: &Path) -> io::Result<()> {
        StdOps.rename(a, b)
    }
    fn remove(&self, p: &Path) -> io::Result<()> {
        StdOps.remove(p)
    }
}

/// A backup that does not match the stock file is dropped and the live bundle list is never
/// written (Important 2).
#[test]
fn sdk_deploy_backup_mismatch_leaves_the_live_bundle_list_untouched() {
    use std::os::unix::fs::MetadataExt;
    let (f, dir) = unit();
    let src = sdk_src(dir.path(), "0.1.0");
    let m = Manifest::parse(&fs::read_to_string(src.join("manifest.toml")).unwrap()).unwrap();
    let ino = fs::metadata(p(&f, mibsdk::BUNDLES)).unwrap().ino();
    let sys = RecordingSys::default();
    let ctx = Ctx::new(f.layout.clone(), &sys);
    let e = {
        let _rw = ctx.mounts.rw("/mnt/app").unwrap();
        format!("{:#}", sdk_pkg::deploy_with(&ctx, &m, &src, None, &BadBackupRead).unwrap_err())
    };
    assert!(e.contains("does not match the stock file"), "{e}");
    assert_eq!(fs::metadata(p(&f, mibsdk::BUNDLES)).unwrap().ino(), ino, "the live bundle list was rewritten");
    assert_eq!(fs::read(p(&f, mibsdk::BUNDLES)).unwrap(), stock());
    assert!(!backups::has_backup(&f.layout, mibsdk::BUNDLES), "the bad backup is dropped");
    assert!(!backups::has_backup(&f.layout, mibsdk::MARKER));
    assert!(p(&f, mibsdk::MARKER).exists());
    assert!(sdk_jars_present(&f).is_empty());
}

/// An adoption whose deploy fails puts the unit back to stock and says so (Minor 1); a fresh
/// install's undo never suggests removing a package that is not installed.
#[test]
fn sdk_adoption_failure_returns_the_unit_to_stock_and_says_so() {
    let (f, dir) = unit();
    write(&p(&f, mibsdk::BUNDLES), &bundles::generate(&stock(), &spec(), false).unwrap());
    let st = format!("{}/stock", mibsdk::BASE);
    write(&p(&f, &format!("{st}/bundles_dsitracer_enabled.properties")), &stock());
    write(&p(&f, &format!("{st}/bundles.mode")), b"777\n");
    write(&p(&f, &format!("{st}/tracer.was")), b"false\n");
    fs::create_dir_all(p(&f, &format!("{}/mibsdk-adapter-hqc.jar", mibsdk::JARS))).unwrap();
    let sys = RecordingSys::default();
    let ctx = Ctx::new(f.layout.clone(), &sys);
    let e = err(install_sdk(&ctx, dir.path(), "0.1.0"));
    assert!(e.contains("the unit was returned to the stock bundle list"), "{e}");
    assert!(!e.contains("remove mibsdk"), "{e}");
    assert_eq!(fs::read(p(&f, mibsdk::BUNDLES)).unwrap(), stock());
    assert_eq!(mode(&p(&f, mibsdk::BUNDLES)), 0o777);
    assert!(!p(&f, mibsdk::MARKER).exists(), "tracer.was said it was absent");
    assert!(!backups::has_backup(&f.layout, mibsdk::BUNDLES));

    let (f, dir) = unit();
    fs::create_dir_all(p(&f, &format!("{}/mibsdk-adapter-hqc.jar", mibsdk::JARS))).unwrap();
    let sys = RecordingSys::default();
    let ctx = Ctx::new(f.layout.clone(), &sys);
    let e = err(install_sdk(&ctx, dir.path(), "0.1.0"));
    assert!(e.contains("the install was undone"), "{e}");
    assert!(!e.contains("remove mibsdk"), "{e}");
}

/// Deploy notes (adoption, the ledger-conflict WARNING) reach the result (Minor 3).
#[test]
fn sdk_install_result_carries_the_deploy_notes() {
    let (f, dir) = unit();
    write(&p(&f, mibsdk::BUNDLES), &bundles::generate(&stock(), &spec(), true).unwrap());
    let st = format!("{}/stock", mibsdk::BASE);
    write(&p(&f, &format!("{st}/bundles_dsitracer_enabled.properties")), &stock());
    write(&p(&f, &format!("{st}/tracer.was")), b"true\n");
    let newer = "parking.soundDirections=bike-rack|PERSISTENT|F0R0L0X0|F1R1L1X1|A\n";
    write(&p(&f, mibsdk::LEDGER), newer.as_bytes());
    write(&p(&f, mibsdk::OLD_LEDGER), ENTRY.as_bytes());
    let sys = RecordingSys::default();
    let ctx = Ctx::new(f.layout.clone(), &sys);
    let msg = install_sdk(&ctx, dir.path(), "0.1.0").unwrap();
    assert!(msg.starts_with(&format!("installed mibsdk 0.1.0{REBOOT_START}")), "{msg}");
    assert!(msg.contains("using the stock mode 777"), "{msg}");
    assert!(msg.contains("adopted the mibsdk-dev stock backup"), "{msg}");
    assert!(msg.contains(&format!("WARNING: kept {}: these overrides conflict with", mibsdk::OLD_LEDGER)), "{msg}");
    assert!(msg.contains(&format!("\n  {}", ENTRY.trim())), "{msg}");
}

/// With --force, the ledger WARNING comes first, then the pushed-plugins one (Minor 4).
#[test]
fn sdk_force_remove_puts_the_ledger_warning_first() {
    let (f, _dir) = installed(Some(ENTRY));
    write(&p(&f, &format!("{}/plugins/hello.jar", mibsdk::BASE)), b"pushed");
    let sys = RecordingSys::default();
    let mut ctx = Ctx::new(f.layout.clone(), &sys);
    ctx.force = true;
    let msg = install::remove(&ctx, "mibsdk", false).unwrap();
    let lines: Vec<&str> = msg.lines().collect();
    assert_eq!(lines[0], "WARNING: --force: uninstalling while these overrides of vehicle state are still in force:");
    assert!(lines[2].starts_with("WARNING: nothing will restore"), "{msg}");
    assert_eq!(lines[3], "WARNING: --force: uninstalling while these pushed plugins remain: hello");
}

/// /mnt/app removals re-assert read-write first and retry once (Minor 5).
#[test]
fn on_app_reasserts_and_retries_once() {
    let (f, _dir) = unit();
    let sys = RecordingSys::default();
    let ctx = Ctx::new(f.layout.clone(), &sys);
    let _rw = ctx.mounts.rw("/mnt/app").unwrap();
    let n = std::cell::Cell::new(0);
    sdk_pkg::on_app(&ctx, "test", &mut || {
        n.set(n.get() + 1);
        if n.get() == 1 { Err(io::Error::other("read-only file system")) } else { Ok(()) }
    })
    .unwrap();
    assert_eq!(n.get(), 2);
    let rw = sys.calls.borrow().iter().filter(|c| *c == "rw /mnt/app").count();
    assert_eq!(rw, 3, "the hold, then a re-assert before each attempt: {:?}", sys.calls.borrow());
    let e = format!("{:#}", sdk_pkg::on_app(&ctx, "removing x", &mut || Err(io::Error::other("nope"))).unwrap_err());
    assert!(e.contains("removing x"), "{e}");
}

// ---- fix round 2 ----

/// The fixture stock list as a newer firmware might ship it.
fn new_firmware_stock() -> Vec<u8> {
    let mut s = stock();
    s.extend(b"# a newer firmware\r\n");
    s
}

/// A stock record left by an earlier install (its forget failed) is stale once mibsdk is gone and
/// the live list is stock: a fresh install backs up the live files again, and uninstall puts the
/// live (new firmware) list back, never the old one.
#[test]
fn sdk_fresh_install_drops_a_stale_stock_record() {
    let (f, dir) = unit();
    // Stale: the old firmware's list, and the marker recorded as absent though it exists now.
    write(&backups::backup_path(&f.layout, mibsdk::BUNDLES), &stock());
    backups::record_absent(&f.layout, mibsdk::MARKER).unwrap();
    write(&p(&f, mibsdk::BUNDLES), &new_firmware_stock());
    let sys = RecordingSys::default();
    let ctx = Ctx::new(f.layout.clone(), &sys);
    install_sdk(&ctx, dir.path(), "0.1.0").unwrap();
    assert_eq!(fs::read(backups::backup_path(&f.layout, mibsdk::BUNDLES)).unwrap(), new_firmware_stock());
    assert!(!backups::is_tombstone(&f.layout, mibsdk::MARKER), "the live marker is backed up again");
    assert_eq!(
        fs::read(p(&f, mibsdk::BUNDLES)).unwrap(),
        bundles::generate(&new_firmware_stock(), &spec(), true).unwrap()
    );
    install::remove(&ctx, "mibsdk", false).unwrap();
    assert_eq!(fs::read(p(&f, mibsdk::BUNDLES)).unwrap(), new_firmware_stock());
    assert!(p(&f, mibsdk::MARKER).exists());
}

/// A record kept by a failed undo while the live list is still generated stays the stock source.
#[test]
fn sdk_fresh_install_keeps_a_record_while_the_live_list_is_generated() {
    let (f, dir) = unit();
    backups::backup_once(&f.layout, mibsdk::BUNDLES).unwrap();
    backups::backup_once(&f.layout, mibsdk::MARKER).unwrap();
    write(&p(&f, mibsdk::BUNDLES), &bundles::generate(&stock(), &spec(), true).unwrap());
    let sys = RecordingSys::default();
    let ctx = Ctx::new(f.layout.clone(), &sys);
    install_sdk(&ctx, dir.path(), "0.1.0").unwrap();
    assert_eq!(fs::read(backups::backup_path(&f.layout, mibsdk::BUNDLES)).unwrap(), stock());
    install::remove(&ctx, "mibsdk", false).unwrap();
    assert_eq!(fs::read(p(&f, mibsdk::BUNDLES)).unwrap(), stock());
}

/// forget_backups tries every record: one that cannot be removed does not keep the other.
#[test]
fn forget_backups_attempts_every_record() {
    let (f, _dir) = unit();
    backups::backup_once(&f.layout, mibsdk::BUNDLES).unwrap();
    backups::backup_once(&f.layout, mibsdk::MARKER).unwrap();
    let mut t = backups::backup_path(&f.layout, mibsdk::BUNDLES).into_os_string();
    t.push(".absent");
    write(&PathBuf::from(t).join("blocker"), b"x"); // the tombstone path is a non-empty directory
    let sys = RecordingSys::default();
    let ctx = Ctx::new(f.layout.clone(), &sys);
    let _rw = ctx.mounts.rw("/mnt/app").unwrap();
    assert!(sdk_pkg::forget_backups(&ctx).is_err());
    assert!(!backups::has_backup(&f.layout, mibsdk::MARKER), "the marker record is still forgotten");
    assert!(!backups::backup_path(&f.layout, mibsdk::BUNDLES).exists());
    let rw = sys.calls.borrow().iter().filter(|c| *c == "rw /mnt/app").count();
    assert!(rw >= 3, "forgetting re-asserts read-write: {:?}", sys.calls.borrow());
}

/// A deploy that fails before the bundle list is written leaves it untouched (not rewritten).
#[test]
fn sdk_deploy_failure_before_the_bundle_list_never_rewrites_it() {
    use std::os::unix::fs::MetadataExt;
    let (f, dir) = unit();
    tracer_off(&f);
    // put's temp file in lsd/ is a non-empty directory: the marker write (first put into lsd/) fails.
    write(&p(&f, &format!("{}/.mibpkg.tmp/blocker", mibsdk::LSD)), b"x");
    let ino = fs::metadata(p(&f, mibsdk::BUNDLES)).unwrap().ino();
    let sys = RecordingSys::default();
    let ctx = Ctx::new(f.layout.clone(), &sys);
    let e = err(install_sdk(&ctx, dir.path(), "0.1.0"));
    assert!(e.contains("the install was undone"), "{e}");
    assert_eq!(fs::metadata(p(&f, mibsdk::BUNDLES)).unwrap().ino(), ino);
    assert!(!p(&f, mibsdk::MARKER).exists());
    assert!(sdk_jars_present(&f).is_empty());
    assert!(!backups::has_backup(&f.layout, mibsdk::BUNDLES));
}

/// A retry after the package files went still honours --purge.
#[test]
fn sdk_remove_retry_honours_purge() {
    let (f, _dir) = installed(None);
    write(&p(&f, &format!("{}/data/x.properties", mibsdk::PERSIST)), b"x=1\n");
    write(&p(&f, &format!("{}/cache/a.jar", mibsdk::RAM)), b"jar");
    let sys = RecordingSys::default();
    let ctx = Ctx::new(f.layout.clone(), &sys);
    {
        let _rw = ctx.mounts.rw("/mnt/app").unwrap();
        sdk_pkg::undeploy(&ctx, false).unwrap();
    }
    fs::remove_dir_all(f.layout.pkg_dir("mibsdk")).unwrap();
    install::remove(&ctx, "mibsdk", true).unwrap();
    assert!(!p(&f, mibsdk::BASE).exists());
    assert!(!p(&f, mibsdk::PERSIST).exists());
    assert!(!p(&f, mibsdk::RAM).exists());
}

// ---- fix round 3 ----

/// A half-done fresh install (the marker created, jars written, the bundle list still stock,
/// mibsdk not recorded) keeps its records: the marker tombstone still says DSITracer was off.
#[test]
fn sdk_records_of_a_half_done_install_are_not_stale() {
    let (f, dir) = unit();
    tracer_off(&f);
    backups::backup_once(&f.layout, mibsdk::BUNDLES).unwrap();
    backups::backup_once(&f.layout, mibsdk::MARKER).unwrap(); // tombstone: absent before
    write(&p(&f, mibsdk::MARKER), b""); // created by the interrupted install
    for j in SDK_JARS {
        write(&p(&f, &format!("{}/{j}", mibsdk::JARS)), b"half-installed");
    }
    let sys = RecordingSys::default();
    let ctx = Ctx::new(f.layout.clone(), &sys);
    let msg = install_sdk(&ctx, dir.path(), "0.1.0").unwrap();
    assert!(!msg.contains("stale"), "{msg}");
    assert!(backups::is_tombstone(&f.layout, mibsdk::MARKER), "the tombstone is kept");
    assert!(!autostart(&f).contains("DSITracer"));
    install::remove(&ctx, "mibsdk", false).unwrap();
    assert!(!p(&f, mibsdk::MARKER).exists(), "DSITracer is not left enabled");
    assert_eq!(fs::read(p(&f, mibsdk::BUNDLES)).unwrap(), stock());
}

/// Dropping a stale record that fails before anything was written says nothing changed.
#[test]
fn sdk_failed_stale_drop_says_nothing_changed() {
    let (f, dir) = unit();
    write(&backups::backup_path(&f.layout, mibsdk::BUNDLES), &stock());
    let mut t = backups::backup_path(&f.layout, mibsdk::BUNDLES).into_os_string();
    t.push(".absent");
    write(&PathBuf::from(t).join("blocker"), b"x"); // forgetting this record fails
    let sys = RecordingSys::default();
    let ctx = Ctx::new(f.layout.clone(), &sys);
    let e = err(install_sdk(&ctx, dir.path(), "0.1.0"));
    assert!(e.ends_with("; nothing changed"), "{e}");
    assert!(!e.contains("undone"), "{e}");
    assert_eq!(fs::read(p(&f, mibsdk::BUNDLES)).unwrap(), stock());
    assert!(sdk_jars_present(&f).is_empty());
}

fn uninstall_self_purge(f: &common::Fixture) {
    let args: Vec<String> = ["--root", f.root().to_str().unwrap(), "--no-supervisor", "uninstall-self", "--purge"]
        .iter()
        .map(|s| s.to_string())
        .collect();
    assert_eq!(mibpkg::cli::run(&args).unwrap(), 0);
}

/// uninstall-self --purge never restores a stale stock record over the live (newer) list, but
/// still restores a record while the live list is generated.
#[test]
fn uninstall_self_purge_skips_stale_sdk_records() {
    let (f, _dir) = unit();
    write(&backups::backup_path(&f.layout, mibsdk::BUNDLES), &stock());
    backups::record_absent(&f.layout, mibsdk::MARKER).unwrap();
    write(&p(&f, mibsdk::BUNDLES), &new_firmware_stock());
    uninstall_self_purge(&f);
    assert_eq!(fs::read(p(&f, mibsdk::BUNDLES)).unwrap(), new_firmware_stock());
    assert!(p(&f, mibsdk::MARKER).exists(), "a stale tombstone deletes nothing");

    let (f, _dir) = sdk_dropped_from_state();
    uninstall_self_purge(&f);
    assert_eq!(fs::read(p(&f, mibsdk::BUNDLES)).unwrap(), stock());
}

// ---- final review fixes ----

const NEW_HMI: &[u8] = b"HMI = HMI_AU_MIB2_HQC_NAR_G22_C10_190101\n";

/// A firmware update replaced lsd/ while mibsdk stayed recorded: the removal leaves the live
/// bundle list and the marker exactly as the firmware has them, still takes the SDK files off,
/// drops the old firmware's records and says so (final review I1).
#[test]
fn sdk_remove_after_a_firmware_update_leaves_the_live_list_and_marker_alone() {
    let generated = bundles::generate(&stock(), &spec(), false).unwrap();
    // (live list, new HMI): a new stock list on a new HMI, a new stock list on the same HMI, and
    // a list still naming the SDK on a new HMI.
    for (live, new_hmi) in [(new_firmware_stock(), true), (new_firmware_stock(), false), (generated.clone(), true)] {
        let (f, _dir) = installed(None); // DSITracer was off: the record says "no marker"
        write(&p(&f, mibsdk::BUNDLES), &live);
        fs::set_permissions(p(&f, mibsdk::BUNDLES), fs::Permissions::from_mode(0o755)).unwrap();
        write(&p(&f, mibsdk::MARKER), b"new firmware");
        if new_hmi {
            write(&p(&f, mibsdk::VERSION_INFO), NEW_HMI);
        }
        let sys = RecordingSys::default();
        let mut ctx = Ctx::new(f.layout.clone(), &sys);
        ctx.force = live == generated; // refused without --force (the next test)
        let msg = install::remove(&ctx, "mibsdk", false).unwrap();
        let case = format!("new_hmi={new_hmi} generated={}", live == generated);
        assert_eq!(fs::read(p(&f, mibsdk::BUNDLES)).unwrap(), live, "{case}");
        assert_eq!(mode(&p(&f, mibsdk::BUNDLES)), 0o755, "{case}");
        assert_eq!(fs::read(p(&f, mibsdk::MARKER)).unwrap(), b"new firmware", "{case}");
        assert!(sdk_jars_present(&f).is_empty(), "{case}");
        assert!(!p(&f, &format!("{}/sdk.properties", mibsdk::BASE)).exists(), "{case}");
        assert!(!p(&f, mibsdk::PERMISSIONS).exists(), "{case}");
        assert!(!backups::has_backup(&f.layout, mibsdk::BUNDLES), "{case}");
        assert!(!backups::has_backup(&f.layout, mibsdk::MARKER), "{case}");
        assert!(!State::load(&f.layout).unwrap().packages.contains_key("mibsdk"), "{case}");
        assert!(msg.contains("the firmware changed since mibsdk was installed"), "{case}: {msg}");
        assert!(msg.contains("left the live bundle list and DSITracer marker as they are"), "{case}: {msg}");
        assert_eq!(msg.contains("WARNING"), live == generated, "{case}: {msg}");
        assert!(msg.ends_with(REBOOT_REMOVE), "{case}: {msg}");
    }
}

/// A removal after a firmware update, interrupted once the package files went, finishes on the
/// retry without restoring the old firmware's list either.
#[test]
fn sdk_remove_retry_after_a_firmware_update_leaves_the_live_list_alone() {
    let (f, _dir) = installed(None);
    write(&p(&f, mibsdk::BUNDLES), &new_firmware_stock());
    write(&p(&f, mibsdk::MARKER), b"new firmware");
    write(&p(&f, mibsdk::VERSION_INFO), NEW_HMI);
    let sys = RecordingSys::default();
    let ctx = Ctx::new(f.layout.clone(), &sys);
    {
        let _rw = ctx.mounts.rw("/mnt/app").unwrap();
        sdk_pkg::undeploy(&ctx, false).unwrap();
    }
    fs::remove_dir_all(f.layout.pkg_dir("mibsdk")).unwrap();
    install::remove(&ctx, "mibsdk", false).unwrap();
    assert_eq!(fs::read(p(&f, mibsdk::BUNDLES)).unwrap(), new_firmware_stock());
    assert_eq!(fs::read(p(&f, mibsdk::MARKER)).unwrap(), b"new firmware");
    assert!(!backups::has_backup(&f.layout, mibsdk::BUNDLES));
    assert!(!State::load(&f.layout).unwrap().packages.contains_key("mibsdk"));
}

/// The undo of a fresh install also takes off the sdk.properties and permissions.json it wrote,
/// so the records it leaves can go stale once the live list is stock again (final review M1).
#[test]
fn sdk_deploy_failure_undo_removes_sdk_properties_and_permissions() {
    let (f, dir) = unit();
    // The last step (the reboot flag) fails: everything else was written.
    write(&mibsdk::reboot_flag(&f.layout).join("blocker"), b"x");
    let sys = RecordingSys::default();
    let ctx = Ctx::new(f.layout.clone(), &sys);
    let e = err(install_sdk(&ctx, dir.path(), "0.1.0"));
    assert!(e.contains("the install was undone"), "{e}");
    assert_eq!(fs::read(p(&f, mibsdk::BUNDLES)).unwrap(), stock());
    assert!(sdk_jars_present(&f).is_empty());
    assert!(!p(&f, &format!("{}/sdk.properties", mibsdk::BASE)).exists());
    assert!(!p(&f, mibsdk::PERMISSIONS).exists());
    assert!(!backups::has_backup(&f.layout, mibsdk::BUNDLES));
}

fn jar_present(f: &common::Fixture, name: &str) -> bool {
    p(f, &format!("{}/{name}", mibsdk::JARS)).is_file()
}

/// An update or rollback takes off the jars of the replaced version that the new one no longer
/// lists; never a jar no installed version listed (final review M2).
#[test]
fn sdk_update_and_rollback_remove_jars_the_new_version_no_longer_lists() {
    let (f, dir) = unit();
    let sys = RecordingSys::default();
    let ctx = Ctx::new(f.layout.clone(), &sys);
    let mut jars = SDK_JARS.to_vec();
    jars.push("mibsdk-extra.jar");
    install_src(&ctx, &sdk_src_with(dir.path(), "0.1.0", &jars, 1, 2)).unwrap();
    assert!(jar_present(&f, "mibsdk-extra.jar"));
    write(&p(&f, &format!("{}/mibsdk-foreign.jar", mibsdk::JARS)), b"not ours");
    install_sdk(&ctx, dir.path(), "0.2.0").unwrap();
    assert!(!jar_present(&f, "mibsdk-extra.jar"), "0.2.0 does not list it");
    assert_eq!(sdk_jars_present(&f), SDK_JARS.to_vec());
    install::rollback(&ctx, "mibsdk").unwrap();
    assert!(jar_present(&f, "mibsdk-extra.jar"), "0.1.0 lists it again");
    install::rollback(&ctx, "mibsdk").unwrap();
    assert!(!jar_present(&f, "mibsdk-extra.jar"));
    assert!(jar_present(&f, "mibsdk-foreign.jar"), "never listed: not ours");
    assert!(jar_present(&f, "stock.jar"));
}

/// The uninstall takes off every SDK jar an installed version listed, not only the current
/// version's; never a jar none listed (final review M2).
#[test]
fn sdk_remove_takes_off_every_jar_an_installed_version_listed() {
    let (f, dir) = unit();
    let sys = RecordingSys::default();
    let ctx = Ctx::new(f.layout.clone(), &sys);
    let mut jars = SDK_JARS.to_vec();
    jars.push("mibsdk-extra.jar");
    install_src(&ctx, &sdk_src_with(dir.path(), "0.1.0", &jars, 1, 2)).unwrap();
    install_sdk(&ctx, dir.path(), "0.2.0").unwrap();
    install_sdk(&ctx, dir.path(), "0.3.0").unwrap(); // 0.1.0's directory is pruned
    // Left by an update that could not remove it.
    write(&p(&f, &format!("{}/mibsdk-extra.jar", mibsdk::JARS)), b"left behind");
    write(&p(&f, &format!("{}/mibsdk-foreign.jar", mibsdk::JARS)), b"not ours");
    install::remove(&ctx, "mibsdk", false).unwrap();
    assert!(sdk_jars_present(&f).is_empty());
    assert!(!jar_present(&f, "mibsdk-extra.jar"));
    assert!(jar_present(&f, "mibsdk-foreign.jar"), "never listed: not ours");
    assert!(jar_present(&f, "stock.jar"));
}

fn uninstall_self(f: &common::Fixture, purge: bool) -> anyhow::Result<i32> {
    let mut args: Vec<String> =
        ["--root", f.root().to_str().unwrap(), "--no-supervisor", "uninstall-self"].iter().map(|s| s.to_string()).collect();
    if purge {
        args.push("--purge".into());
    }
    mibpkg::cli::run(&args)
}

/// uninstall-self refuses while an SDK no package records is active: a mibsdk-dev backup, or a
/// live list naming the SDK that nothing would restore (final review M3).
#[test]
fn uninstall_self_refuses_while_an_unrecorded_sdk_is_active() {
    // A mibsdk-dev install: its stock backup and a generated live list.
    let (f, _dir) = unit();
    write(&p(&f, &format!("{}/stock/tracer.was", mibsdk::BASE)), b"true\n");
    for purge in [false, true] {
        let e = format!("{:#}", uninstall_self(&f, purge).unwrap_err());
        assert!(e.contains("mibsdk-dev"), "{e}");
        assert!(e.contains("mibpkg install mibsdk") && e.contains("mibpkg remove mibsdk"), "{e}");
    }
    // A live list naming the SDK without a record of the stock list.
    let (f, _dir) = unit();
    let generated = bundles::generate(&stock(), &spec(), true).unwrap();
    write(&p(&f, mibsdk::BUNDLES), &generated);
    for purge in [false, true] {
        let e = format!("{:#}", uninstall_self(&f, purge).unwrap_err());
        assert!(e.contains("references the SDK"), "{e}");
        assert_eq!(fs::read(p(&f, mibsdk::BUNDLES)).unwrap(), generated);
    }
    // With a record, only --purge restores it.
    backups::backup_once(&f.layout, mibsdk::BUNDLES).unwrap();
    let e = format!("{:#}", uninstall_self(&f, false).unwrap_err());
    assert!(e.contains("mibpkg uninstall-self --purge"), "{e}");
    // No promise it restores the list on any firmware.
    assert!(e.contains("only on the firmware the SDK was installed for"), "{e}");
    assert!(e.contains("--force") && e.contains("put the stock bundle list back by hand"), "{e}");
    assert!(f.layout.home().exists());
}

/// A live list still naming the SDK on an HMI the SDK does not support: the stock backup is from
/// older firmware, so the removal refuses, changing nothing, unless --force (controller ruling on I1).
#[test]
fn sdk_remove_refuses_when_the_hmi_changed_but_the_list_still_names_the_sdk() {
    let (f, _dir) = installed(None);
    write(&p(&f, mibsdk::VERSION_INFO), NEW_HMI);
    let before = tree(f.root());
    let sys = RecordingSys::default();
    let ctx = Ctx::new(f.layout.clone(), &sys);
    let e = format!("{:#}", install::remove(&ctx, "mibsdk", false).unwrap_err());
    assert!(e.contains("the firmware changed since mibsdk was installed"), "{e}");
    assert!(e.contains("could stop the HMI booting"), "{e}");
    assert!(e.contains("--force"), "{e}");
    assert!(e.ends_with("nothing changed"), "{e}");
    assert_eq!(tree(f.root()), before);
    assert_still_installed(&f);
}

// ---- uninstall-self --purge after a firmware update ----

/// An installed mibsdk whose state record went (its package files remain): purge may run, and the
/// last installed spec is readable. DSITracer was off: the marker record is a tombstone.
fn sdk_dropped_from_state() -> (common::Fixture, tempfile::TempDir) {
    let (f, dir) = installed(None);
    let mut st = State::load(&f.layout).unwrap();
    st.packages.remove("mibsdk");
    st.save(&f.layout).unwrap();
    (f, dir)
}

fn uninstall_self_with(f: &common::Fixture, extra: &[&str]) -> anyhow::Result<i32> {
    let mut args: Vec<String> =
        ["--root", f.root().to_str().unwrap(), "--no-supervisor", "uninstall-self"].iter().map(|s| s.to_string()).collect();
    args.extend(extra.iter().map(|s| s.to_string()));
    mibpkg::cli::run(&args)
}

/// uninstall-self run as the binary (its message goes to stdout); must succeed. Stdout returned.
fn uninstall_self_bin(f: &common::Fixture, extra: &[&str]) -> String {
    let out = std::process::Command::new(env!("CARGO_BIN_EXE_mibpkg"))
        .args(["--root", f.root().to_str().unwrap(), "--no-supervisor", "uninstall-self"])
        .args(extra)
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&out.stdout).into_owned();
    assert!(out.status.success(), "{stdout}{}", String::from_utf8_lossy(&out.stderr));
    stdout
}

/// The stock records are an older firmware's once the HMI is not one the last installed mibsdk
/// supports (or that spec cannot be read): purge refuses, changing nothing (I1 on the purge path).
#[test]
fn uninstall_self_purge_refuses_when_the_firmware_changed() {
    let (f, _dir) = sdk_dropped_from_state();
    write(&p(&f, mibsdk::VERSION_INFO), NEW_HMI);
    let before = tree(f.root());
    let e = format!("{:#}", uninstall_self_with(&f, &["--purge"]).unwrap_err());
    assert!(e.contains("the firmware changed since mibsdk was installed"), "{e}");
    assert!(e.contains("could stop the HMI booting"), "{e}");
    assert!(e.contains("--force"), "{e}");
    assert!(e.ends_with("nothing changed"), "{e}");
    assert_eq!(tree(f.root()), before);

    // No readable spec: nothing says which firmware the records belong to.
    let (f, _dir) = unit();
    backups::backup_once(&f.layout, mibsdk::BUNDLES).unwrap();
    write(&p(&f, mibsdk::BUNDLES), &bundles::generate(&stock(), &spec(), true).unwrap());
    let before = tree(f.root());
    let e = format!("{:#}", uninstall_self_with(&f, &["--purge"]).unwrap_err());
    assert!(e.contains("the firmware changed since mibsdk was installed"), "{e}");
    assert!(e.ends_with("nothing changed"), "{e}");
    assert_eq!(tree(f.root()), before);
}

/// With --force the purge leaves the live bundle list and marker byte-identical, forgets the old
/// firmware's records and still removes mibpkg.
#[test]
fn uninstall_self_purge_force_leaves_the_live_list_when_the_firmware_changed() {
    let (f, _dir) = sdk_dropped_from_state();
    write(&p(&f, mibsdk::VERSION_INFO), NEW_HMI);
    write(&p(&f, mibsdk::MARKER), b"new firmware");
    let live = fs::read(p(&f, mibsdk::BUNDLES)).unwrap();
    let live_mode = mode(&p(&f, mibsdk::BUNDLES));
    let out = uninstall_self_bin(&f, &["--purge", "--force"]);
    assert!(out.contains("WARNING: --force"), "{out}");
    assert!(!out.contains("stock files restored"), "{out}");
    assert!(out.contains("restored the other stock files"), "{out}");
    assert!(out.contains("left the bundle list and DSITracer marker as they are"), "{out}");
    assert_eq!(fs::read(p(&f, mibsdk::BUNDLES)).unwrap(), live);
    assert_eq!(mode(&p(&f, mibsdk::BUNDLES)), live_mode);
    assert_eq!(fs::read(p(&f, mibsdk::MARKER)).unwrap(), b"new firmware");
    assert!(!f.layout.home().exists(), "purge still deletes mibpkg's files");
    assert_eq!(sdk_jars_present(&f), SDK_JARS.to_vec(), "purge never removes the SDK files");
}

/// On the firmware the last installed mibsdk supports, purge restores the stock records as before.
#[test]
fn uninstall_self_purge_restores_the_records_on_the_same_firmware() {
    let (f, _dir) = sdk_dropped_from_state();
    let out = uninstall_self_bin(&f, &["--purge"]);
    assert!(out.contains("stock files restored and all mibpkg files deleted"), "{out}");
    assert!(!out.contains("WARNING"), "{out}");
    assert_eq!(fs::read(p(&f, mibsdk::BUNDLES)).unwrap(), stock());
    assert!(!p(&f, mibsdk::MARKER).exists(), "the tombstone removes the marker the install created");
    assert!(!f.layout.home().exists());
}

// ---- HMISystem= (an SDK whose [sdk] has hmisystem) ----

const SYSTEM_MANIFEST: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/manifests/sdk-mibsdk-system.toml");
const HMISYSTEM_STOCK: &str = "HMISystem=de.audi.tghu.system.hmi.Activator";
const HMISYSTEM_SDK: &str = "HMISystem=mibsdk.adapter.hqc.ui.SdkSystemActivator";

/// `sdk_src` with the manifest that also takes the `HMISystem=` line over.
fn sdk_src_system(dir: &Path, version: &str) -> PathBuf {
    let s = sdk_src(dir, version);
    let text = fs::read_to_string(SYSTEM_MANIFEST).unwrap().replace("version = \"0.1.0\"", &format!("version = \"{version}\""));
    fs::write(s.join("manifest.toml"), text).unwrap();
    s
}

fn install_sdk_system(ctx: &Ctx, dir: &Path, version: &str) -> anyhow::Result<String> {
    install_src(ctx, &sdk_src_system(dir, version))
}

/// An install of the system SDK refused with `want`: no remount, and no file anywhere changed.
fn assert_system_refused_untouched(f: &common::Fixture, dir: &Path, version: &str, want: &str) {
    let before = tree(f.root());
    let sys = RecordingSys::default();
    let ctx = Ctx::new(f.layout.clone(), &sys);
    let e = err(install_sdk_system(&ctx, dir, version));
    assert!(e.contains(want), "{e}");
    assert!(e.ends_with("; nothing changed"), "{e}");
    assert!(sys.calls.borrow().is_empty(), "/mnt/app was remounted: {:?}", sys.calls.borrow());
    assert_eq!(tree(f.root()), before, "{want}");
}

fn bundles_text(f: &common::Fixture) -> String {
    String::from_utf8(fs::read(p(f, mibsdk::BUNDLES)).unwrap()).unwrap()
}

#[test]
fn sdk_remove_restores_stock_bundles_byte_for_byte_with_hmisystem() {
    let (f, dir) = unit();
    tracer_off(&f);
    let sys = RecordingSys::default();
    let ctx = Ctx::new(f.layout.clone(), &sys);
    install_sdk_system(&ctx, dir.path(), "0.1.0").unwrap();
    let live = bundles_text(&f);
    assert_eq!(live.matches(&format!("{HMISYSTEM_SDK}\r\n")).count(), 1, "{live}");
    assert!(!live.contains(HMISYSTEM_STOCK));
    assert!(live.contains(&format!("{HMICAR_SDK}\r\n")));
    install::remove(&ctx, "mibsdk", false).unwrap();
    assert_eq!(fs::read(p(&f, mibsdk::BUNDLES)).unwrap(), stock());
    assert!(bundles_text(&f).contains(&format!("{HMISYSTEM_STOCK}\r\n")));
    assert_eq!(mode(&p(&f, mibsdk::BUNDLES)), 0o777);
}

#[test]
fn install_refuses_a_foreign_hmisystem_line() {
    // A fresh install: the live list must hold the stock line (the SDK's own value too is refused).
    for bad in ["HMISystem=x.Y", "HMISystem = de.audi.tghu.system.hmi.Activator", HMISYSTEM_SDK] {
        let (f, dir) = unit();
        write(&p(&f, mibsdk::BUNDLES), bundles_text(&f).replace(HMISYSTEM_STOCK, bad).as_bytes());
        assert_system_refused_untouched(&f, dir.path(), "0.1.0", "HMISystem is not stock in the stock bundles file");
    }
    // An update: the live list may hold the stock or the SDK line, nothing else.
    let (f, dir) = unit();
    let sys = RecordingSys::default();
    let ctx = Ctx::new(f.layout.clone(), &sys);
    install_sdk_system(&ctx, dir.path(), "0.1.0").unwrap();
    install_sdk_system(&ctx, dir.path(), "0.2.0").unwrap();
    let ours = bundles_text(&f);
    write(&p(&f, mibsdk::BUNDLES), ours.replace(HMISYSTEM_SDK, "HMISystem=other.Act").as_bytes());
    assert_system_refused_untouched(
        &f,
        dir.path(),
        "0.3.0",
        "HMISystem is not stock in the installed /mnt/app/eso/hmi/lsd/bundles_dsitracer_enabled.properties (HMISystem=other.Act)",
    );
    // ... and the stock backup must hold the stock line.
    write(&p(&f, mibsdk::BUNDLES), ours.as_bytes());
    let bk = backups::backup_path(&f.layout, mibsdk::BUNDLES);
    fs::write(&bk, fs::read_to_string(&bk).unwrap().replace(HMISYSTEM_STOCK, HMISYSTEM_SDK)).unwrap();
    assert_system_refused_untouched(&f, dir.path(), "0.3.0", "HMISystem is not stock in the stock bundles file");
}

#[test]
fn firmware_changed_remove_refuses_without_force_when_only_hmisystem_references_the_sdk() {
    let (f, dir) = unit();
    tracer_off(&f);
    let sys = RecordingSys::default();
    let ctx = Ctx::new(f.layout.clone(), &sys);
    install_sdk_system(&ctx, dir.path(), "0.1.0").unwrap();
    // A new firmware's list: stock but for a non-stock HMISystem= line naming neither mibsdk nor the bundle.
    let live = String::from_utf8(new_firmware_stock()).unwrap().replace(HMISYSTEM_STOCK, "HMISystem=other.Activator");
    write(&p(&f, mibsdk::BUNDLES), live.as_bytes());
    write(&p(&f, mibsdk::VERSION_INFO), NEW_HMI);
    let before = tree(f.root());
    let e = format!("{:#}", install::remove(&ctx, "mibsdk", false).unwrap_err());
    assert!(e.contains("the firmware changed since mibsdk was installed"), "{e}");
    assert!(e.contains("--force"), "{e}");
    assert!(e.ends_with("nothing changed"), "{e}");
    assert_eq!(tree(f.root()), before);
    assert_eq!(sdk_jars_present(&f), SDK_JARS.to_vec());
    assert!(State::load(&f.layout).unwrap().packages.contains_key("mibsdk"));
    // With --force the live list is left as it is.
    let mut ctx = Ctx::new(f.layout.clone(), &sys);
    ctx.force = true;
    let msg = install::remove(&ctx, "mibsdk", false).unwrap();
    assert!(msg.contains("WARNING"), "{msg}");
    assert_eq!(bundles_text(&f), live);
}

#[test]
fn update_from_an_sdk_without_hmisystem_takes_the_line_over() {
    let (f, dir) = unit();
    let sys = RecordingSys::default();
    let ctx = Ctx::new(f.layout.clone(), &sys);
    install_sdk(&ctx, dir.path(), "0.1.0").unwrap();
    assert!(bundles_text(&f).contains(&format!("{HMISYSTEM_STOCK}\r\n")));
    install_sdk_system(&ctx, dir.path(), "0.2.0").unwrap();
    let live = bundles_text(&f);
    assert_eq!(live.matches(&format!("{HMISYSTEM_SDK}\r\n")).count(), 1, "{live}");
    assert!(!live.contains(HMISYSTEM_STOCK));
    install::remove(&ctx, "mibsdk", false).unwrap();
    assert_eq!(fs::read(p(&f, mibsdk::BUNDLES)).unwrap(), stock());
}

#[test]
fn owned_prefix_system_hmi_refuses_a_foreign_jar() {
    let cls = "de/audi/tghu/system/hmi/Activator.class";
    let (f, dir) = unit();
    jar_with(&p(&f, &format!("{}/zz-other-mod.jar", mibsdk::JARS)), &["a/B.class", cls]);
    assert_system_refused_untouched(&f, dir.path(), "0.1.0", &format!("zz-other-mod.jar defines {cls}, which the SDK owns"));
}

#[test]
fn references_sdk_counts_a_nonstock_hmisystem_only_with_the_spec_that_has_it() {
    // Stale-record logic: the live list (HMISystem= foreign, nothing else) references the SDK
    // for a spec with hmisystem, not for one without.
    let live = String::from_utf8(stock()).unwrap().replace(HMISYSTEM_STOCK, "HMISystem=other.Activator");
    let (f, _dir) = unit();
    write(&p(&f, mibsdk::BUNDLES), live.as_bytes());
    backups::backup_once(&f.layout, mibsdk::BUNDLES).unwrap();
    let system = Manifest::parse(&fs::read_to_string(SYSTEM_MANIFEST).unwrap()).unwrap().sdk.unwrap();
    assert!(sdk_pkg::stale_records(&f.layout, Some(&spec())).unwrap());
    assert!(!sdk_pkg::stale_records(&f.layout, Some(&system)).unwrap());
}

// ---- icons (Task 9) ----

const ICON_FIX: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/icons");

fn icon(name: &str) -> Vec<u8> {
    fs::read(Path::new(ICON_FIX).join(name)).unwrap()
}

fn sha16(b: &[u8]) -> String {
    use sha2::{Digest, Sha256};
    Sha256::digest(b)[..8].iter().map(|x| format!("{x:02x}")).collect()
}

/// `sdk_src` declaring the built-in `icons` (name, PNG bytes), shipped as `icons/<name>.png`.
fn sdk_src_icons(dir: &Path, version: &str, icons: &[(&str, Vec<u8>)]) -> PathBuf {
    let s = sdk_src(dir, version);
    let names: Vec<String> = icons.iter().map(|(n, _)| format!("{n:?}")).collect();
    let mut m = fs::read_to_string(s.join("manifest.toml")).unwrap();
    m.push_str(&format!("icons = [{}]\n", names.join(", ")));
    fs::write(s.join("manifest.toml"), m).unwrap();
    for (n, b) in icons {
        write(&s.join(format!("icons/{n}.png")), b);
    }
    s
}

#[test]
fn sdk_install_writes_icons_sdk_and_uninstall_removes_it() {
    let (f, dir) = unit();
    let sys = RecordingSys::default();
    let ctx = Ctx::new(f.layout.clone(), &sys);
    install_src(&ctx, &sdk_src_icons(dir.path(), "0.1.0", &[("star-72", icon("ok-72.png")), ("star-20", icon("ok-20.png"))])).unwrap();
    let d = p(&f, mibsdk::SDK_ICONS);
    let (big, small) = (sha16(&icon("ok-72.png")), sha16(&icon("ok-20.png")));
    assert_eq!(fs::read_to_string(d.join("index.properties")).unwrap(), format!("star-20={small},20,20\nstar-72={big},72,72\n"));
    assert_eq!(fs::read(d.join(format!("{big}.png"))).unwrap(), icon("ok-72.png"));
    assert_eq!(fs::read(d.join(format!("{small}.png"))).unwrap(), icon("ok-20.png"));
    // An update drops what the new set no longer lists.
    install_src(&ctx, &sdk_src_icons(dir.path(), "0.2.0", &[("star-20", icon("ok-20.png"))])).unwrap();
    assert!(!d.join(format!("{big}.png")).exists());
    assert_eq!(fs::read_to_string(d.join("index.properties")).unwrap(), format!("star-20={small},20,20\n"));
    // A plugin's icons, then the plugin removed: its (now empty) ICONS goes with the SDK.
    fs::create_dir_all(p(&f, mibsdk::ICONS)).unwrap();
    let msg = install::remove(&ctx, "mibsdk", false).unwrap();
    assert!(msg.ends_with(REBOOT_REMOVE), "{msg}");
    assert!(!d.exists());
    assert!(!p(&f, mibsdk::ICONS).exists());
}

#[test]
fn sdk_install_with_a_bad_icon_is_refused_untouched() {
    let (f, dir) = unit();
    let before = tree(f.root());
    let sys = RecordingSys::default();
    let ctx = Ctx::new(f.layout.clone(), &sys);
    let e = err(install_src(&ctx, &sdk_src_icons(dir.path(), "0.1.0", &[("star-72", icon("apng.png"))])));
    assert!(e.starts_with("icon star-72: animated PNG"), "{e}");
    assert!(e.ends_with("; nothing changed"), "{e}");
    assert!(sys.calls.borrow().is_empty(), "/mnt/app was remounted: {:?}", sys.calls.borrow());
    assert_eq!(tree(f.root()), before);
}

//! `[plugin]` and `[sdk]`: the two declarative package kinds the mibsdk Gradle build emits.
use mibpkg::manifest::{Kind, Manifest};
use std::fs;
use std::path::Path;

const HELLO: &str = r#"
[package]
name="hello"
version="1.0.0"

[plugin]
jar="hello.jar"
api=1
permissions=["vehicle.read"]
"#;

const SDK_TABLE: &str = r#"[sdk]
api = 2
min_api = 1
hmi = ["HMI_AU_MIB2_HQC_NAR_G22_C10_180803"]
jars = ["mibsdk-runtime.jar", "mibsdk-adapter-hqc.jar", "mibsdk-rt-retrotranslator.jar"]
stamp = "mibsdk-unit.stamp"
configurator = "mibsdk.adapter.hqc.ChainingConfigurator"
bundle = "MibSdk=mibsdk.adapter.hqc.SdkBundleActivator"
tracer_configurator = "de.audi.tghu.dsitrace.TraceConfigurator"
hmicar_stock = "de.audi.tghu.car.hmi.Activator"
hmicar = "mibsdk.adapter.hqc.ui.SdkCarActivator"
owned = ["de/audi/tghu/car/hmi/", "de/audi/atip/model/CarModelBank"]
properties = ["hook.services=org.dsi.ifc.carparkingsystem.DSICarParkingSystem", "ui.enabled=true"]
permissions = "permissions.json"
"#;

fn sdk(name: &str) -> String {
    format!("[package]\nname = \"{name}\"\nversion = \"0.1.0\"\n\n{SDK_TABLE}")
}

/// The SDK manifest with one `[sdk]` line replaced.
fn sdk_with(key: &str, line: &str) -> String {
    let table: String = SDK_TABLE
        .lines()
        .map(|l| if l.starts_with(&format!("{key} =")) { line.to_string() } else { l.to_string() })
        .collect::<Vec<_>>()
        .join("\n");
    format!("[package]\nname = \"mibsdk\"\nversion = \"0.1.0\"\n\n{table}\n")
}

fn plugin(name: &str, body: &str) -> String {
    format!("[package]\nname = \"{name}\"\nversion = \"1.0.0\"\n\n[plugin]\n{body}\n")
}

fn refused(text: &str, want: &str) {
    match Manifest::parse(text) {
        Ok(_) => panic!("accepted, wanted {want:?}:\n{text}"),
        Err(e) => {
            let msg = format!("{e:#}");
            assert!(msg.contains(want), "{msg:?} does not contain {want:?}\n{text}");
        }
    }
}

fn fixture(name: &str) -> String {
    fs::read_to_string(Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/manifests").join(name)).unwrap()
}

#[test]
fn plugin_manifest_parses_and_is_kind_plugin() {
    let m = Manifest::parse(HELLO).unwrap();
    assert_eq!(m.kind(), Kind::Plugin);
    let p = m.plugin.as_ref().unwrap();
    assert_eq!(p.jar, "hello.jar");
    assert_eq!(p.api, 1);
    assert_eq!(p.permissions, vec!["vehicle.read"]);
    assert!(m.sdk.is_none());
}

#[test]
fn sdk_manifest_parses() {
    let m = Manifest::parse(&sdk("mibsdk")).unwrap();
    assert_eq!(m.kind(), Kind::Sdk);
    let s = m.sdk.as_ref().unwrap();
    assert_eq!((s.api, s.min_api), (2, 1));
    assert_eq!(s.hmi, vec!["HMI_AU_MIB2_HQC_NAR_G22_C10_180803"]);
    assert_eq!(s.jars, vec!["mibsdk-runtime.jar", "mibsdk-adapter-hqc.jar", "mibsdk-rt-retrotranslator.jar"]);
    assert_eq!(s.stamp, "mibsdk-unit.stamp");
    assert_eq!(s.configurator, "mibsdk.adapter.hqc.ChainingConfigurator");
    assert_eq!(s.bundle, "MibSdk=mibsdk.adapter.hqc.SdkBundleActivator");
    assert_eq!(s.tracer_configurator, "de.audi.tghu.dsitrace.TraceConfigurator");
    assert_eq!(s.hmicar_stock, "de.audi.tghu.car.hmi.Activator");
    assert_eq!(s.hmicar, "mibsdk.adapter.hqc.ui.SdkCarActivator");
    assert_eq!(s.owned, vec!["de/audi/tghu/car/hmi/", "de/audi/atip/model/CarModelBank"]);
    assert_eq!(
        s.properties,
        vec!["hook.services=org.dsi.ifc.carparkingsystem.DSICarParkingSystem", "ui.enabled=true"]
    );
    assert_eq!(s.permissions, "permissions.json");
    assert!(m.plugin.is_none());
}

#[test]
fn sdk_properties_default_to_empty() {
    let text = sdk("mibsdk").lines().filter(|l| !l.starts_with("properties =")).collect::<Vec<_>>().join("\n");
    assert!(Manifest::parse(&text).unwrap().sdk.unwrap().properties.is_empty());
}

#[test]
fn plain_manifest_is_kind_plain() {
    let m = Manifest::parse("[package]\nname = \"tool\"\nversion = \"1.0.0\"\n").unwrap();
    assert_eq!(m.kind(), Kind::Plain);
}

/// Copied byte for byte from the SDK repo's `./gradlew :dist:mibpkgStage` and
/// `:plugins:bike-rack:mibPackageStage` output.
#[test]
fn gradle_generated_manifests_parse() {
    let s = Manifest::parse(&fixture("sdk-mibsdk.toml")).unwrap();
    assert_eq!(s.kind(), Kind::Sdk);
    assert_eq!(s.compat.units, vec!["MHI2Q"]);
    let p = Manifest::parse(&fixture("plugin-bike-rack.toml")).unwrap();
    assert_eq!(p.kind(), Kind::Plugin);
    assert_eq!(p.package.name, "bike-rack");
    let spec = p.plugin.unwrap();
    assert_eq!((spec.jar.as_str(), spec.api), ("bike-rack.jar", 2));
    assert_eq!(spec.permissions, vec!["vehicle.read", "vehicle.parking.write", "ui.slot.carmenu", "ui.slot.statusbar"]);
}

#[test]
fn plugin_with_no_permissions_parses() {
    let m = Manifest::parse(&plugin("hello", "jar = \"hello.jar\"\napi = 1\npermissions = []")).unwrap();
    assert!(m.plugin.unwrap().permissions.is_empty());
}

#[test]
fn both_tables_are_refused() {
    let text = format!("{}\n[plugin]\njar = \"x.jar\"\napi = 1\npermissions = []\n", sdk("mibsdk"));
    refused(&text, "a package is either [plugin] or [sdk]");
}

#[test]
fn declarative_kinds_refuse_files_services_configs_and_scripts() {
    let extras = [
        "[[file]]\nsrc = \"x\"\n",
        "[[file]]\nsrc = \"x\"\n[[service]]\nname = \"x\"\nexec = \"@pkg/x\"\n",
        "[[config]]\npath = \"x.conf\"\n",
        "[scripts]\npost_install = \"x.sh\"\n",
    ];
    for extra in extras {
        refused(&format!("{HELLO}\n{extra}"), "plugin and sdk packages are declarative: no files, services, configs or scripts");
        refused(
            &format!("{}\n{extra}", sdk("mibsdk")),
            "plugin and sdk packages are declarative: no files, services, configs or scripts",
        );
    }
}

#[test]
fn plugin_id_shorter_than_two_is_refused() {
    refused(&plugin("a", "jar = \"a.jar\"\napi = 1\npermissions = []"), "must be 2-40 characters");
    refused(&plugin("a", "jar = \"a.jar\"\napi = 1\npermissions = []"), "plugin id");
}

#[test]
fn sdk_must_be_named_mibsdk() {
    refused(&sdk("notsdk"), "the SDK package must be named mibsdk");
}

#[test]
fn plain_manifest_named_mibsdk_is_refused() {
    refused("[package]\nname = \"mibsdk\"\nversion = \"1.0.0\"\n", "the name mibsdk is reserved for the SDK package");
    refused(&plugin("mibsdk", "jar = \"mibsdk.jar\"\napi = 1\npermissions = []"), "the name mibsdk is reserved");
}

#[test]
fn sdk_manifest_named_mibsdk_is_accepted() {
    assert_eq!(Manifest::parse(&sdk("mibsdk")).unwrap().package.name, "mibsdk");
}

#[test]
fn plugin_jar_must_be_a_single_jar_file_name() {
    for jar in ["lib/hello.jar", "../hello.jar", "hello.zip", "", "/hello.jar"] {
        refused(&plugin("hello", &format!("jar = \"{jar}\"\napi = 1\npermissions = []")), "jar");
    }
}

#[test]
fn sdk_file_names_must_be_single_components_with_their_extension() {
    refused(&sdk_with("jars", "jars = [\"mibsdk-runtime.jar\", \"lib/x.jar\"]"), "jar");
    refused(&sdk_with("jars", "jars = [\"mibsdk-runtime.zip\"]"), ".jar");
    refused(&sdk_with("jars", "jars = [\"..\"]"), "jar");
    refused(&sdk_with("stamp", "stamp = \"mibsdk-unit.txt\""), ".stamp");
    refused(&sdk_with("stamp", "stamp = \"a/mibsdk-unit.stamp\""), "stamp");
    refused(&sdk_with("permissions", "permissions = \"permissions.txt\""), ".json");
    refused(&sdk_with("permissions", "permissions = \"../permissions.json\""), "permissions");
}

#[test]
fn api_range_is_checked() {
    refused(&sdk_with("min_api", "min_api = 0"), "min_api");
    refused(&sdk_with("min_api", "min_api = 3"), "min_api");
    assert!(Manifest::parse(&sdk_with("min_api", "min_api = 2")).is_ok());
    refused(&plugin("hello", "jar = \"hello.jar\"\napi = 0\npermissions = []"), "api");
}

#[test]
fn permission_ids_are_checked() {
    for bad in ["", "vehicle read", "a,b", "x=y"] {
        refused(
            &plugin("hello", &format!("jar = \"hello.jar\"\napi = 1\npermissions = [\"{bad}\"]")),
            "permission",
        );
    }
}

#[test]
fn sdk_class_names_bundle_and_owned_prefixes_are_checked() {
    refused(&sdk_with("bundle", "bundle = \"MibSdk\""), "bundle");
    refused(&sdk_with("hmicar_stock", "hmicar_stock = \"1bad\""), "hmicar_stock");
    refused(&sdk_with("hmicar", "hmicar = \"a b\""), "hmicar");
    refused(&sdk_with("hmicar", "hmicar = \"\""), "hmicar");
    refused(&sdk_with("owned", "owned = [\"\"]"), "owned");
    refused(&sdk_with("owned", "owned = [\"/de/audi/\"]"), "owned");
    refused(&sdk_with("owned", "owned = [\"de.audi.tghu\"]"), "owned");
    assert!(Manifest::parse(&sdk_with("hmicar", "hmicar = \"a.B$c_1\"")).is_ok());
}

#[test]
fn sdk_lines_written_to_unit_files_cannot_carry_newlines() {
    refused(&sdk_with("bundle", "bundle = \"A=b\\nX=y\""), "bundle");
    refused(&sdk_with("properties", "properties = [\"a=b\\nc=d\"]"), "properties");
    refused(&sdk_with("configurator", "configurator = \"a\\nb\""), "configurator");
    refused(&sdk_with("tracer_configurator", "tracer_configurator = \"a b\""), "tracer_configurator");
}

// ---- archive::pack ----

fn stage(dir: &Path, manifest: &str, files: &[&str]) {
    fs::create_dir_all(dir).unwrap();
    fs::write(dir.join("manifest.toml"), manifest).unwrap();
    for f in files {
        fs::write(dir.join(f), b"x").unwrap();
    }
}

const SDK_FILES: &[&str] =
    &["mibsdk-runtime.jar", "mibsdk-adapter-hqc.jar", "mibsdk-rt-retrotranslator.jar", "mibsdk-unit.stamp", "permissions.json"];

fn pack_err(dir: &Path) -> String {
    let out = dir.parent().unwrap().join("out.mpk");
    format!("{:#}", mibpkg::archive::pack(dir, &out).unwrap_err())
}

#[test]
fn pack_accepts_complete_plugin_and_sdk_stages() {
    let t = tempfile::tempdir().unwrap();
    let p = t.path().join("p");
    stage(&p, HELLO, &["hello.jar", "hello.jar.stamp"]);
    let m = mibpkg::archive::pack(&p, &t.path().join("hello.mpk")).unwrap();
    assert_eq!(m.kind(), Kind::Plugin);
    let s = t.path().join("s");
    stage(&s, &sdk("mibsdk"), SDK_FILES);
    let pkg_path = t.path().join("mibsdk.mpk");
    mibpkg::archive::pack(&s, &pkg_path).unwrap();
    assert_eq!(mibpkg::archive::Package::read(&pkg_path).unwrap().manifest.kind(), Kind::Sdk);
}

#[test]
fn pack_refuses_plugin_without_jar() {
    let t = tempfile::tempdir().unwrap();
    let p = t.path().join("p");
    stage(&p, HELLO, &["hello.jar.stamp"]);
    assert!(pack_err(&p).contains("hello.jar is named in manifest.toml but missing from"));
}

#[test]
fn pack_refuses_plugin_without_stamp() {
    let t = tempfile::tempdir().unwrap();
    let p = t.path().join("p");
    stage(&p, HELLO, &["hello.jar"]);
    assert!(pack_err(&p).contains("hello.jar.stamp is named in manifest.toml but missing from"));
}

#[test]
fn pack_refuses_sdk_without_a_jar() {
    let t = tempfile::tempdir().unwrap();
    let s = t.path().join("s");
    stage(&s, &sdk("mibsdk"), &SDK_FILES[1..]);
    assert!(pack_err(&s).contains("mibsdk-runtime.jar is named in manifest.toml but missing from"));
}

#[test]
fn pack_refuses_sdk_without_unit_stamp() {
    let t = tempfile::tempdir().unwrap();
    let s = t.path().join("s");
    let files: Vec<&str> = SDK_FILES.iter().copied().filter(|f| *f != "mibsdk-unit.stamp").collect();
    stage(&s, &sdk("mibsdk"), &files);
    assert!(pack_err(&s).contains("mibsdk-unit.stamp is named in manifest.toml but missing from"));
}

#[test]
fn pack_refuses_sdk_without_permissions_json() {
    let t = tempfile::tempdir().unwrap();
    let s = t.path().join("s");
    let files: Vec<&str> = SDK_FILES.iter().copied().filter(|f| *f != "permissions.json").collect();
    stage(&s, &sdk("mibsdk"), &files);
    assert!(pack_err(&s).contains("permissions.json is named in manifest.toml but missing from"));
}

// ---- hardening (review follow-up) ----

#[test]
fn sdk_jars_must_be_mibsdk_jars_but_not_spike_jars() {
    refused(&sdk_with("jars", "jars = [\"runtime.jar\"]"), "mibsdk-");
    refused(&sdk_with("jars", "jars = [\"mibsdk-runtime.jar\", \"mibsdk-spike-ui.jar\"]"), "mibsdk-spike-");
    assert!(Manifest::parse(&sdk_with("jars", "jars = [\"mibsdk-runtime.jar\"]")).is_ok());
}

#[test]
fn sdk_bundle_must_be_key_equals_class_and_not_hmicar() {
    for bad in ["=a.B", "Mib Sdk=a.B", "MibSdk=", "MibSdk=a b", "MibSdk=a.B\\\\", "HMICar=a.B", "Mib-Sdk=a.B"] {
        refused(&sdk_with("bundle", &format!("bundle = \"{bad}\"")), "bundle");
    }
    assert!(Manifest::parse(&sdk_with("bundle", "bundle = \"Mib_Sdk2=a.B$c\"")).is_ok());
}

#[test]
fn sdk_properties_must_be_assignments_without_trailing_escape_or_chain_configurator() {
    for bad in [
        "ui.enabled",
        "ui.enabled=true\\\\",
        "a=b\\\\\\\\\\\\",
        "chain.configurator=x.Y",
        " chain.configurator = x.Y",
        "chain.configurator:x.Y",
    ] {
        refused(&sdk_with("properties", &format!("properties = [\"{bad}\"]")), "properties");
    }
    // An even run of backslashes is an escaped backslash, not a continuation.
    assert!(Manifest::parse(&sdk_with("properties", "properties = [\"a=b\\\\\\\\\"]")).is_ok());
}

#[test]
fn sdk_hmi_entries_are_word_characters() {
    for bad in ["", "HMI AU", "HMI-AU", "HMI/AU"] {
        refused(&sdk_with("hmi", &format!("hmi = [\"{bad}\"]")), "hmi");
    }
}

#[test]
fn file_names_refuse_spaces_and_control_characters() {
    refused(&plugin("hello", "jar = \"hel lo.jar\"\napi = 1\npermissions = []"), "jar");
    refused(&sdk_with("stamp", "stamp = \"mibsdk-unit\\t.stamp\""), "stamp");
    refused(&sdk_with("permissions", "permissions = \"perm issions.json\""), "permissions");
    refused(&sdk_with("jars", "jars = [\"mibsdk-run\\ntime.jar\"]"), "jar");
}

#[test]
fn plugin_jar_must_be_named_after_the_plugin_id() {
    refused(&plugin("hello", "jar = \"other.jar\"\napi = 1\npermissions = []"), "hello.jar");
}

#[test]
fn sdk_jars_hmi_and_owned_must_be_non_empty() {
    refused(&sdk_with("jars", "jars = []"), "jars");
    refused(&sdk_with("hmi", "hmi = []"), "hmi");
    refused(&sdk_with("owned", "owned = []"), "owned");
}

// ---- [sdk] hmisystem ----

/// The SDK manifest with `extra` lines added to `[sdk]` after `hmicar`.
fn sdk_plus(extra: &str) -> String {
    sdk("mibsdk").replace(
        "hmicar = \"mibsdk.adapter.hqc.ui.SdkCarActivator\"\n",
        &format!("hmicar = \"mibsdk.adapter.hqc.ui.SdkCarActivator\"\n{extra}\n"),
    )
}

#[test]
fn hmisystem_keys_go_together() {
    let stock = "hmisystem_stock = \"de.audi.tghu.system.hmi.Activator\"";
    let ours = "hmisystem = \"mibsdk.adapter.hqc.ui.SdkSystemActivator\"";
    refused(&sdk_plus(stock), "[sdk] hmisystem and hmisystem_stock go together");
    refused(&sdk_plus(ours), "[sdk] hmisystem and hmisystem_stock go together");
    let s = Manifest::parse(&sdk_plus(&format!("{stock}\n{ours}"))).unwrap().sdk.unwrap();
    assert_eq!(s.hmisystem_stock.as_deref(), Some("de.audi.tghu.system.hmi.Activator"));
    assert_eq!(s.hmisystem.as_deref(), Some("mibsdk.adapter.hqc.ui.SdkSystemActivator"));
    let s = Manifest::parse(&sdk("mibsdk")).unwrap().sdk.unwrap();
    assert_eq!((s.hmisystem_stock, s.hmisystem), (None, None));
}

#[test]
fn hmisystem_values_are_class_names_and_the_bundle_key_is_not_hmisystem() {
    refused(&sdk_plus("hmisystem_stock = \"1bad\"\nhmisystem = \"a.B\""), "[sdk] hmisystem_stock \"1bad\" must be a Java class name");
    refused(&sdk_plus("hmisystem_stock = \"a.B\"\nhmisystem = \"a b\""), "[sdk] hmisystem \"a b\" must be a Java class name");
    refused(&sdk_with("bundle", "bundle = \"HMISystem=mibsdk.adapter.hqc.SdkBundleActivator\""), "and not HMICar or HMISystem");
    refused(&sdk_with("bundle", "bundle = \"HMICar=mibsdk.adapter.hqc.SdkBundleActivator\""), "and not HMICar or HMISystem");
}

#[test]
fn the_system_fixture_parses_with_the_third_owned_prefix() {
    let s = Manifest::parse(&fixture("sdk-mibsdk-system.toml")).unwrap().sdk.unwrap();
    assert_eq!(s.hmisystem.as_deref(), Some("mibsdk.adapter.hqc.ui.SdkSystemActivator"));
    assert_eq!(s.owned, vec!["de/audi/tghu/car/hmi/", "de/audi/atip/model/CarModelBank", "de/audi/tghu/system/hmi/"]);
}

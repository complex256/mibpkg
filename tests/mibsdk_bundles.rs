//! The SDK's bundle list (`bundles_dsitracer_enabled.properties`), `sdk.properties` and the
//! `HMICar=` guard: port of the SDK repo's `installer/test_gen_bundles.py` plus `mibsdk-dev`'s
//! `check_hmicar`.
use mibpkg::manifest::SdkSpec;
use mibpkg::mibsdk::bundles::{check_hmicar, generate, sdk_properties};

const STOCK_FILE: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/tests/fixtures/mibsdk/bundles_dsitracer_enabled.properties"
);
const CFG: &[u8] = b"de.audi.tghu.serviceDelegateConfigurator=mibsdk.adapter.hqc.ChainingConfigurator";
const HMICAR_STOCK: &str = "HMICar=de.audi.tghu.car.hmi.Activator";
const HMICAR_SDK: &str = "HMICar=mibsdk.adapter.hqc.ui.SdkCarActivator";

fn spec() -> SdkSpec {
    SdkSpec {
        api: 2,
        min_api: 1,
        hmi: vec!["HMI_AU_MIB2_HQC_NAR_G22_C10_180803".into()],
        jars: vec![
            "mibsdk-runtime.jar".into(),
            "mibsdk-adapter-hqc.jar".into(),
            "mibsdk-rt-retrotranslator.jar".into(),
        ],
        stamp: "mibsdk-unit.stamp".into(),
        configurator: "mibsdk.adapter.hqc.ChainingConfigurator".into(),
        bundle: "MibSdk=mibsdk.adapter.hqc.SdkBundleActivator".into(),
        tracer_configurator: "de.audi.tghu.dsitrace.TraceConfigurator".into(),
        hmicar_stock: "de.audi.tghu.car.hmi.Activator".into(),
        hmicar: "mibsdk.adapter.hqc.ui.SdkCarActivator".into(),
        owned: vec!["de/audi/tghu/car/hmi/".into(), "de/audi/atip/model/CarModelBank".into()],
        properties: vec![
            "hook.services=org.dsi.ifc.carparkingsystem.DSICarParkingSystem".into(),
            "ui.enabled=true".into(),
        ],
        permissions: "permissions.json".into(),
    }
}

fn stock() -> Vec<u8> {
    std::fs::read(STOCK_FILE).unwrap()
}

fn gen(src: &[u8], tracer: bool) -> Vec<u8> {
    generate(src, &spec(), tracer).unwrap()
}

fn refusal(src: &[u8], tracer: bool) -> String {
    format!("{:#}", generate(src, &spec(), tracer).expect_err("generate accepted the input"))
}

fn contains(hay: &[u8], needle: &[u8]) -> bool {
    count(hay, needle) > 0
}

fn count(hay: &[u8], needle: &[u8]) -> usize {
    hay.windows(needle.len()).filter(|w| *w == needle).count()
}

fn replace(hay: &[u8], from: &[u8], to: &[u8]) -> Vec<u8> {
    let mut out = Vec::new();
    let mut i = 0;
    while i < hay.len() {
        if hay[i..].starts_with(from) {
            out.extend_from_slice(to);
            i += from.len();
        } else {
            out.push(hay[i]);
            i += 1;
        }
    }
    out
}

fn split_lf(b: &[u8]) -> Vec<&[u8]> {
    b.split(|c| *c == b'\n').collect()
}

// ---- generate (port of test_gen_bundles.py) ----

#[test]
fn tracer_active() {
    // was: test_tracer_active
    let out = gen(&stock(), true);
    assert!(contains(&out, b"Bundle.Startup.Autostart = DSITracer,MibSdk,FwUtilServices,Profiling,Tracing,ATIPImpl\n"));
    assert!(contains(&out, &[CFG, b"\r\n"].concat()));
    assert_eq!(count(&out, b"serviceDelegateConfigurator="), 1);
}

#[test]
fn tracer_inactive() {
    // was: test_tracer_inactive
    let out = gen(&stock(), false);
    assert!(contains(&out, b"Bundle.Startup.Autostart = MibSdk,FwUtilServices,Profiling,Tracing,ATIPImpl\n"));
    assert!(!contains(&out, b"DSITracer,"));
}

#[test]
fn other_lines_unchanged() {
    // was: test_other_lines_unchanged
    let stock_bytes = stock();
    let out_bytes = gen(&stock_bytes, true);
    let stock = split_lf(&stock_bytes);
    let out = split_lf(&out_bytes);
    let changed: Vec<usize> = (0..stock.len() - 1).filter(|&i| out[i] != stock[i]).collect();
    assert_eq!(changed.len(), 3);
    assert!(out[changed[0]].starts_with(b"de.audi.tghu.serviceDelegateConfigurator="));
    assert!(out[changed[1]].starts_with(b"Bundle.Startup.Autostart"));
    assert!(out[changed[2]].starts_with(b"HMICar="));
    let extra = &out[stock.len() - 1..];
    assert_eq!(out.iter().filter(|l| l.starts_with(b"MibSdk=")).count(), 1);
    assert!(contains(&extra.join(&b'\n'), b"MibSdk=mibsdk.adapter.hqc.SdkBundleActivator"));
    // stock ends "\r\n", so the split leaves a trailing empty element that the appended line replaces
    assert_eq!(out[stock.len() - 2], stock[stock.len() - 2]);
}

#[test]
fn sdkprops_without_chain() {
    // was: test_sdkprops_without_chain
    assert_eq!(
        sdk_properties(&spec(), false),
        b"hook.services=org.dsi.ifc.carparkingsystem.DSICarParkingSystem\nui.enabled=true\n"
    );
    assert_eq!(
        sdk_properties(&spec(), true),
        b"chain.configurator=de.audi.tghu.dsitrace.TraceConfigurator\n\
          hook.services=org.dsi.ifc.carparkingsystem.DSICarParkingSystem\nui.enabled=true\n"
    );
}

#[test]
fn hmicar_substituted_once_keeping_crlf() {
    // was: test_hmicar_substituted_once_keeping_crlf
    let stock = stock();
    assert_eq!(1, count(&stock, b"HMICar=de.audi.tghu.car.hmi.Activator\r\n"));
    let out = gen(&stock, true);
    assert_eq!(1, count(&out, b"HMICar="));
    assert!(contains(&out, b"HMICar=mibsdk.adapter.hqc.ui.SdkCarActivator\r\n"));
    assert!(!contains(&out, b"de.audi.tghu.car.hmi.Activator"));
}

#[test]
fn nonstock_hmicar_refused() {
    // was: test_nonstock_hmicar_refused
    let bad = replace(&stock(), b"HMICar=de.audi.tghu.car.hmi.Activator", b"HMICar=other.Activator");
    assert_eq!(refusal(&bad, true), "HMICar is not stock (other.Activator); refusing");
}

#[test]
fn generate_is_fixed_point() {
    // was: test_generate_is_fixed_point
    for tracer in [true, false] {
        let once = gen(&stock(), tracer);
        assert_eq!(once, gen(&once, tracer));
        assert!(contains(&once, b"HMICar=mibsdk.adapter.hqc.ui.SdkCarActivator\r\n"));
    }
}

#[test]
fn sdkprops_enables_ui() {
    // was: test_sdkprops_enables_ui
    for tracer in [true, false] {
        assert!(contains(&sdk_properties(&spec(), tracer), b"ui.enabled=true\n"));
    }
}

#[test]
fn idempotent_on_already_generated() {
    // was: test_idempotent_on_already_generated
    for tracer in [true, false] {
        let first = gen(&stock(), tracer);
        let second = gen(&first, tracer);
        assert_eq!(first, second);
        assert_eq!(count(&second, b"MibSdk=mibsdk"), 1);
        assert_eq!(count(&second, b"MibSdk,"), 1);
    }
}

#[test]
fn multiple_active_configurator_lines_refused() {
    // was: test_multiple_active_configurator_lines_refused
    let dup = [&stock()[..], b"de.audi.tghu.serviceDelegateConfigurator=other.Configurator\r\n"].concat();
    assert_eq!(
        refusal(&dup, true),
        "2 active serviceDelegateConfigurator lines in the stock file (the last would win); refusing to guess"
    );
}

// ---- generate: edge cases of the port ----

#[test]
fn ends_with_the_sdk_bundle_line_in_the_files_own_eol() {
    assert!(gen(&stock(), true).ends_with(b"MibSdk=mibsdk.adapter.hqc.SdkBundleActivator\r\n"));
    assert_eq!(
        gen(b"de.audi.tghu.serviceDelegateConfigurator=x\nBundle.Startup.Autostart=A\nHMICar=de.audi.tghu.car.hmi.Activator", false),
        b"de.audi.tghu.serviceDelegateConfigurator=mibsdk.adapter.hqc.ChainingConfigurator\n\
          Bundle.Startup.Autostart=MibSdk,A\n\
          HMICar=mibsdk.adapter.hqc.ui.SdkCarActivator\n\
          MibSdk=mibsdk.adapter.hqc.SdkBundleActivator\n"
    );
}

#[test]
fn a_commented_configurator_is_activated_when_none_is_active() {
    let src = b"#  de.audi.tghu.serviceDelegateConfigurator = old\r\nBundle.Startup.Autostart=\r\nHMICar=de.audi.tghu.car.hmi.Activator\r\n";
    assert_eq!(
        gen(src, true),
        b"de.audi.tghu.serviceDelegateConfigurator=mibsdk.adapter.hqc.ChainingConfigurator\r\n\
          Bundle.Startup.Autostart=DSITracer,MibSdk\r\n\
          HMICar=mibsdk.adapter.hqc.ui.SdkCarActivator\r\n\
          MibSdk=mibsdk.adapter.hqc.SdkBundleActivator\r\n"
    );
}

#[test]
fn autostart_drops_existing_sdk_and_tracer_entries_and_blanks() {
    let src = b"de.audi.tghu.serviceDelegateConfigurator=x\nBundle.Startup.Autostart\t= A , MibSdk,,DSITracer ,B\nHMICar=de.audi.tghu.car.hmi.Activator\nMibSdk =keep\n";
    let out = gen(src, false);
    assert!(contains(&out, b"Bundle.Startup.Autostart\t= MibSdk,A,B\n"), "{}", String::from_utf8_lossy(&out));
    assert!(out.ends_with(b"MibSdk =keep\n"), "an existing bundle line is left alone");
}

#[test]
fn refusals_name_what_is_wrong() {
    let hmi: &[u8] = b"HMICar=de.audi.tghu.car.hmi.Activator\n";
    let auto: &[u8] = b"Bundle.Startup.Autostart=A\n";
    let cfg: &[u8] = b"de.audi.tghu.serviceDelegateConfigurator=x\n";
    assert_eq!(refusal(&[auto, hmi].concat(), true), "no serviceDelegateConfigurator line in the stock file");
    assert_eq!(
        refusal(&[cfg, hmi].concat(), true),
        "expected exactly one Bundle.Startup.Autostart line, found 0"
    );
    assert_eq!(
        refusal(&[cfg, auto, auto, hmi].concat(), true),
        "expected exactly one Bundle.Startup.Autostart line, found 2"
    );
    assert_eq!(refusal(&[cfg, auto].concat(), true), "expected exactly one HMICar= line, found 0");
    assert_eq!(
        refusal(&[cfg, auto, hmi, b"HMICar \t=de.audi.tghu.car.hmi.Activator\n"].concat(), true),
        "expected exactly one HMICar= line, found 2"
    );
    assert_eq!(
        refusal(&[cfg, auto, b"HMICar = de.audi.tghu.car.hmi.Activator \r\n"].concat(), true),
        "HMICar is not stock (de.audi.tghu.car.hmi.Activator); refusing"
    );
}

// ---- sdk.properties ----

#[test]
fn sdk_properties_with_and_without_tracer() {
    assert_eq!(
        sdk_properties(&spec(), true),
        b"chain.configurator=de.audi.tghu.dsitrace.TraceConfigurator\n\
          hook.services=org.dsi.ifc.carparkingsystem.DSICarParkingSystem\n\
          ui.enabled=true\n"
    );
    assert_eq!(
        sdk_properties(&spec(), false),
        b"hook.services=org.dsi.ifc.carparkingsystem.DSICarParkingSystem\nui.enabled=true\n"
    );
}

// ---- check_hmicar (port of mibsdk-dev's check_hmicar) ----

fn hmicar_refusal(text: &[u8], allowed: &[&str]) -> String {
    format!("{:#}", check_hmicar(text, "the stock bundles file", allowed).expect_err("check_hmicar accepted"))
}

#[test]
fn check_hmicar_accepts_listed_lines_only_and_tolerates_crlf() {
    check_hmicar(&stock(), "the stock bundles file", &[HMICAR_STOCK]).unwrap();
    let generated = gen(&stock(), true);
    check_hmicar(&generated, "the installed bundles", &[HMICAR_STOCK, HMICAR_SDK]).unwrap();
    check_hmicar(b"a=b\nHMICar=de.audi.tghu.car.hmi.Activator", "x", &[HMICAR_STOCK]).unwrap();
    assert_eq!(
        hmicar_refusal(&generated, &[HMICAR_STOCK]),
        "HMICar is not stock in the stock bundles file (HMICar=mibsdk.adapter.hqc.ui.SdkCarActivator); refusing; nothing changed"
    );
    assert_eq!(
        hmicar_refusal(b"a=b\r\n#HMICar=x\r\n", &[HMICAR_STOCK]),
        "HMICar is not stock in the stock bundles file (no HMICar= line); refusing; nothing changed"
    );
    // Only one trailing CR is tolerated.
    assert_eq!(
        hmicar_refusal(b"HMICar=de.audi.tghu.car.hmi.Activator\r\r\n", &[HMICAR_STOCK]),
        "HMICar is not stock in the stock bundles file (HMICar=de.audi.tghu.car.hmi.Activator\r); refusing; nothing changed"
    );
}

#[test]
fn check_hmicar_refuses_two_lines_and_spaced_foreign_value() {
    let two = [&stock()[..], b"HMICar=de.audi.tghu.car.hmi.Activator\r\n"].concat();
    assert_eq!(
        hmicar_refusal(&two, &[HMICAR_STOCK]),
        "HMICar is not stock in the stock bundles file (HMICar=de.audi.tghu.car.hmi.Activator); refusing; nothing changed"
    );
    let spaced = replace(&stock(), HMICAR_STOCK.as_bytes(), b"HMICar = other.Activator");
    assert_eq!(
        hmicar_refusal(&spaced, &[HMICAR_STOCK]),
        "HMICar is not stock in the stock bundles file (HMICar = other.Activator); refusing; nothing changed"
    );
    // A second key spelled with a blank before the `=` counts too, wherever its `=` is.
    let hidden = [&stock()[..], b"HMICar\tx=y\n"].concat();
    assert_eq!(
        hmicar_refusal(&hidden, &[HMICAR_STOCK]),
        "HMICar is not stock in the stock bundles file (HMICar\tx=y); refusing; nothing changed"
    );
}

// ---- the HMICar key as java.util.Properties reads it ----

/// Extra lines that each define the key `HMICar` to `Properties.load`, shown in the refusal as
/// the (first physical) line that carries it.
const SPELLINGS: [(&[u8], &str); 13] = [
    (b" HMICar=x\n", " HMICar=x"),
    (b"\tHMICar=x\n", "\tHMICar=x"),
    (b"\x0cHMICar=x\n", "\x0cHMICar=x"),
    (b"HMICar:x\n", "HMICar:x"),
    (b"HMICar  :  x\n", "HMICar  :  x"),
    (b"HMICar x\n", "HMICar x"),
    (b"HMICar\n", "HMICar"),
    (b"HMICar\tx=y\n", "HMICar\tx=y"),
    (b"HMI\\u0043ar=x\n", "HMI\\u0043ar=x"),
    (b"\\HMICar=x\n", "\\HMICar=x"),
    (b"HMI\\\n   Car=x\n", "HMI\\"),
    (b"a=b\rHMICar=x\r\n", "HMICar=x"),
    (b"c=d\\\\\nHMICar=x\n", "HMICar=x"),
];

#[test]
fn check_hmicar_counts_every_properties_spelling_of_the_key() {
    for (extra, shown) in SPELLINGS {
        let text = [&stock()[..], extra].concat();
        assert_eq!(
            hmicar_refusal(&text, &[HMICAR_STOCK]),
            format!("HMICar is not stock in the stock bundles file ({shown}); refusing; nothing changed"),
            "{:?}",
            String::from_utf8_lossy(extra)
        );
    }
}

#[test]
fn check_hmicar_skips_continuations_comments_and_other_keys() {
    for extra in [
        &b"a=b\\\nHMICar=x\n"[..],
        b"a=b\\\r\n  HMICar:x\r\n",
        b"a=b\\\rHMICar x\r",
        b"# HMICar=x\n",
        b"  ! HMICar=x\n",
        b"HMICarX=x\n",
        b"xHMICar=x\n",
        b"HMICa\\r=x\n",
    ] {
        let text = [&stock()[..], extra].concat();
        check_hmicar(&text, "x", &[HMICAR_STOCK]).unwrap_or_else(|e| panic!("{extra:?}: {e:#}"));
    }
    // A comment does not continue onto the next line.
    let text = [&stock()[..], b"# c\\\nHMICar=x\n"].concat();
    assert!(hmicar_refusal(&text, &[HMICAR_STOCK]).contains("(HMICar=x)"));
}

#[test]
fn check_hmicar_shows_other_bytes_as_latin1() {
    assert_eq!(
        hmicar_refusal(b"HMICar=caf\xe9\xff\n", &[HMICAR_STOCK]),
        "HMICar is not stock in the stock bundles file (HMICar=caf\u{e9}\u{ff}); refusing; nothing changed"
    );
}

#[test]
fn generate_refuses_every_spelling_check_hmicar_refuses() {
    for (extra, _) in SPELLINGS {
        for base in [stock(), gen(&stock(), true)] {
            let text = [&base[..], extra].concat();
            let why = refusal(&text, true);
            assert!(why.contains("HMICar"), "{:?}: {why}", String::from_utf8_lossy(extra));
        }
    }
    // The only HMICar= line continues the line before it: Properties sees no HMICar key at all.
    let src = b"de.audi.tghu.serviceDelegateConfigurator=x\nBundle.Startup.Autostart=A\na=b\\\nHMICar=de.audi.tghu.car.hmi.Activator\n";
    assert_eq!(refusal(src, true), "expected exactly one HMICar= line, found 0");
}

#[test]
fn generate_refuses_a_touched_line_ending_in_a_bare_cr() {
    let cfg = &b"de.audi.tghu.serviceDelegateConfigurator=x"[..];
    let auto = &b"Bundle.Startup.Autostart=A"[..];
    let hmi = &b"HMICar=de.audi.tghu.car.hmi.Activator"[..];
    for (i, what) in ["de.audi.tghu.serviceDelegateConfigurator", "Bundle.Startup.Autostart", "HMICar"].iter().enumerate() {
        let lines: Vec<Vec<u8>> =
            [cfg, auto, hmi].iter().enumerate().map(|(j, l)| [*l, if i == j { b"\r" } else { b"\n" }].concat()).collect();
        assert_eq!(
            refusal(&lines.concat(), true),
            format!("the {what} line ends in a bare CR; rewriting it would join it to the next line; refusing")
        );
    }
}

#[test]
fn generate_refuses_a_bundle_line_without_a_name() {
    for bundle in ["MibSdk", "=mibsdk.adapter.hqc.SdkBundleActivator"] {
        let sdk = SdkSpec { bundle: bundle.into(), ..spec() };
        let why = format!("{:#}", generate(&stock(), &sdk, true).expect_err("accepted"));
        assert_eq!(why, format!("[sdk] bundle {bundle:?} is not a <name>=<activator> line"));
    }
}

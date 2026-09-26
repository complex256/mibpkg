mod common;
use mibpkg::manifest::Compat;
use mibpkg::unit::{detect, parse_img_ver, UnitInfo};
use std::fs;

#[test]
fn parses_img_ver() {
    let u = parse_img_ver(common::IMG_VER);
    assert_eq!(u.unit.as_deref(), Some("MHI2Q"));
    assert_eq!(u.firmware.as_deref(), Some("DEV_MMX2QC_AU_NAR_G22_061PROD"));
    assert_eq!(parse_img_ver("#MMX2_EU_P1234\n").unit.as_deref(), Some("MHI2"));
    assert_eq!(parse_img_ver(""), UnitInfo::default());
    assert_eq!(parse_img_ver("#SOMETHING\n").unit, None);
}

#[test]
fn detect_reads_file_and_settings_override() {
    let f = common::fixture();
    assert_eq!(detect(&f.layout).unit.as_deref(), Some("MHI2Q"));
    fs::create_dir_all(f.layout.home()).unwrap();
    fs::write(f.layout.home().join("settings.toml"), "unit = \"MHI2\"\n").unwrap();
    let l = mibpkg::paths::Layout::load(f.root()).unwrap();
    assert_eq!(detect(&l).unit.as_deref(), Some("MHI2"));
}

#[test]
fn compat_rules() {
    let q = parse_img_ver(common::IMG_VER);
    let any = Compat::default();
    assert!(any.matches(&q));
    assert!(any.matches(&UnitInfo::default()));
    let only_q = Compat { units: vec!["MHI2Q".into()], firmware: vec![] };
    assert!(only_q.matches(&q));
    assert!(!only_q.matches(&UnitInfo::default()), "unknown unit never matches a unit list");
    let only_2 = Compat { units: vec!["MHI2".into()], firmware: vec![] };
    assert!(!only_2.matches(&q));
    let fw = Compat { units: vec![], firmware: vec!["G22".into()] };
    assert!(fw.matches(&q));
    let fw_no = Compat { units: vec![], firmware: vec!["G19".into()] };
    assert!(!fw_no.matches(&q));
}

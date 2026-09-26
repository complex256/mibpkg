mod common;
use mibpkg::backups;
use mibpkg::state::{Installed, State};
use std::fs;

#[test]
fn state_roundtrip_and_default() {
    let f = common::fixture();
    assert_eq!(State::load(&f.layout).unwrap(), State::default());
    let mut s = State::default();
    s.packages.insert(
        "carmqtt".into(),
        Installed {
            collection: "complex256".into(),
            version: "1.2.0".into(),
            previous: Some("1.1.0".into()),
            services: vec!["carmqtt.carmqtt".into()],
            uses_scripts: false,
        },
    );
    s.results.insert("carmqtt".into(), "OK".into());
    s.save(&f.layout).unwrap();
    assert_eq!(State::load(&f.layout).unwrap(), s);
}

#[test]
fn corrupt_state_is_an_error() {
    let f = common::fixture();
    fs::create_dir_all(f.layout.state_dir()).unwrap();
    fs::write(f.layout.state_dir().join("installed.json"), "{").unwrap();
    assert!(State::load(&f.layout).is_err());
}

#[test]
fn backup_once_keeps_the_first_original() {
    let f = common::fixture();
    let unit = "/mnt/system/etc/boot/startup.sh";
    let b = backups::backup_once(&f.layout, unit).unwrap();
    assert_eq!(fs::read_to_string(&b).unwrap(), common::STARTUP);
    fs::write(f.layout.p(unit), "changed").unwrap();
    backups::backup_once(&f.layout, unit).unwrap();
    assert_eq!(fs::read_to_string(&b).unwrap(), common::STARTUP, "second backup must not overwrite");
    backups::restore(&f.layout, unit).unwrap();
    assert_eq!(fs::read_to_string(f.layout.p(unit)).unwrap(), common::STARTUP);
    assert_eq!(b.file_name().unwrap(), "mnt%system%etc%boot%startup.sh");
}

//! web.toml loading, PIN hashing and random ids.
mod common;
use mibpkg::web::settings::{self, check_pin, hash_pin, is_id, random_id, valid_pin, WebSettings};
use std::fs;

#[test]
fn defaults_when_the_file_is_missing() {
    let f = common::fixture();
    let (s, warning) = settings::load(&f.layout);
    assert_eq!(s, WebSettings::default());
    assert_eq!(s.port, 80);
    assert_eq!(s.listen, vec!["hotspot", "tailscale"]);
    assert!(warning.is_none());
}

#[test]
fn defaults_and_a_warning_when_the_file_is_broken() {
    let f = common::fixture();
    fs::create_dir_all(f.layout.home()).unwrap();
    for bad in ["port = \"eighty\"", "listen = [\"everywhere\"]", "listen = [\"any\", \"hotspot\"]", "pin = \"1234\"", "{{{"] {
        fs::write(settings::path(&f.layout), bad).unwrap();
        let (s, warning) = settings::load(&f.layout);
        assert_eq!(s, WebSettings::default(), "{bad}");
        assert!(warning.unwrap().contains("using the defaults"), "{bad}");
    }
}

#[test]
fn save_and_load_round_trip() {
    let f = common::fixture();
    let mut s = WebSettings::default();
    s.port = 8080;
    s.listen = vec!["any".into()];
    s.pin = Some(hash_pin("2468"));
    s.interfaces.insert("hotspot".into(), vec!["uap0".into()]);
    settings::save(&f.layout, &s).unwrap();
    assert_eq!(settings::load(&f.layout), (s, None));
}

#[test]
fn pins_are_salted_and_checked() {
    let a = hash_pin("2468");
    let b = hash_pin("2468");
    assert_ne!(a, b, "a fresh salt each time");
    assert!(check_pin(&a, "2468") && check_pin(&b, "2468"));
    assert!(!check_pin(&a, "2469") && !check_pin(&a, ""));
    assert!(!check_pin("garbage", "2468"));
    assert!(valid_pin("1234") && valid_pin("123456789012"));
    for bad in ["123", "1234567890123", "12a4", "", " 1234"] {
        assert!(!valid_pin(bad), "{bad:?}");
    }
}

#[test]
fn random_ids_are_32_hex_and_unique() {
    let a = random_id();
    let b = random_id();
    assert!(is_id(&a) && is_id(&b), "{a} {b}");
    assert_ne!(a, b);
}

#[test]
fn a_broken_file_keeps_a_well_formed_pin() {
    let f = common::fixture();
    fs::create_dir_all(f.layout.home()).unwrap();
    let pin = hash_pin("2468");
    // A typo elsewhere (or a field from a newer mibpkg) must not quietly remove the PIN.
    for broken in [format!("listen = [\"everywhere\"]\npin = \"{pin}\"\n"), format!("pin = \"{pin}\"\nfuture_field = 1\n")] {
        fs::write(settings::path(&f.layout), &broken).unwrap();
        let (s, warning) = settings::load(&f.layout);
        assert_eq!(s.pin.as_deref(), Some(pin.as_str()), "{broken}");
        assert_eq!(s.listen, WebSettings::default().listen);
        assert!(warning.unwrap().contains("kept the PIN"));
    }
}

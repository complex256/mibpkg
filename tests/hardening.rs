//! Input hardening shared by GEM, the CLI and the web interface.
mod common;
use mibpkg::index::{Collection, RESERVED_COLLECTION};
use mibpkg::manifest::valid_name;
use std::fs;

#[test]
fn names_must_start_with_a_letter_or_digit() {
    let long = "a".repeat(41);
    for bad in ["--root", "-x", "-", "", "a/b", "A", "x;y", "a b", long.as_str()] {
        assert!(!valid_name(bad), "{bad:?} should be rejected");
    }
    for good in ["carmqtt", "0day", "car-mqtt", "a", "complex256-carmqtt"] {
        assert!(valid_name(good), "{good:?} should be accepted");
    }
}

#[test]
fn global_flags_stop_at_the_command_word() {
    // "--root" after the command is the package argument, not a global flag: it must never
    // redirect mibpkg to another root.
    let f = common::fixture();
    let args: Vec<String> = ["--root", f.root().to_str().unwrap(), "--no-supervisor", "gem", "install", "--root", "/etc"]
        .iter()
        .map(|s| s.to_string())
        .collect();
    assert_eq!(mibpkg::cli::run(&args).unwrap(), 0);
    let st = mibpkg::state::State::load(&f.layout).unwrap();
    let res = &st.results["_"];
    assert!(res.contains("invalid package name"), "{res}");
}

#[test]
fn collection_id_uploaded_is_reserved() {
    assert_eq!(RESERVED_COLLECTION, "uploaded");
    let t = tempfile::tempdir().unwrap();
    fs::write(
        t.path().join("index.json"),
        r#"{"format":1,"collection":{"id":"uploaded","name":"x"},"packages":[]}"#,
    )
    .unwrap();
    let e = Collection::load(t.path()).unwrap_err();
    assert!(format!("{e:#}").contains("reserved"), "{e:#}");
    let idx: mibpkg::index::Index =
        serde_json::from_str(r#"{"format":1,"collection":{"id":"uploaded","name":"x"},"packages":[]}"#).unwrap();
    assert!(Collection::from_online(idx, "https://x/").is_err());
    let src = t.path().join("c");
    fs::create_dir_all(src.join("packages")).unwrap();
    assert!(mibpkg::index::build_index(&src, "uploaded", "x").is_err());
}

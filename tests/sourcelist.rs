//! The community source list (sources.json) and an SD card's sources.toml.
mod common;
use mibpkg::online;
use mibpkg::sourcelist::{self, ListEntry};
use std::fs;

const LIST: &str = r#"{ "format": 1, "sources": [
  { "name": "mibpkg", "description": "The package manager", "source": "github:complex256/mibpkg-packages" },
  { "name": "alice", "source": "github:alice/mib-tools", "key": "ed25519:ignored", "extra": 5 },
  { "name": "no source" },
  { "name": "plain http", "source": "http://example.com/index.json" },
  { "name": "XXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXX", "source": "github:a/b" },
  { "name": 7, "source": "github:c/d" }
] }"#;

#[test]
fn list_keeps_good_entries_and_explains_bad_ones() {
    let p = sourcelist::parse_list(LIST.as_bytes()).unwrap();
    assert_eq!(p.entries.len(), 2, "{:?}", p.skipped);
    assert_eq!(p.entries[1], ListEntry { name: "alice".into(), description: String::new(), source: "github:alice/mib-tools".into() });
    assert_eq!(p.skipped.len(), 4);
    assert!(p.skipped[0].starts_with("entry 3:"), "{:?}", p.skipped);
    assert!(p.skipped.iter().any(|s| s.contains("1-60")), "{:?}", p.skipped);
}

#[test]
fn list_with_unknown_format_or_garbage_is_an_error() {
    assert!(sourcelist::parse_list(br#"{"format":2,"sources":[]}"#).unwrap_err().to_string().contains("format 2"));
    assert!(sourcelist::parse_list(b"<html>").is_err());
    assert!(sourcelist::parse_list(br#"{"format":1}"#).is_err());
}

#[test]
fn card_sources_toml_is_parsed_per_entry() {
    let p = sourcelist::parse_card(
        "[[source]]\nname = \"alice's tools\"\ndescription = \"dashcam\"\nsource = \"github:alice/mib-tools\"\nkey = \"ed25519:x\"\n\n[[source]]\nname = \"bad\"\nsource = \"ftp://x\"\n",
    )
    .unwrap();
    assert_eq!(p.entries.len(), 1);
    assert_eq!(p.entries[0].description, "dashcam");
    assert_eq!(p.skipped.len(), 1);
    assert!(sourcelist::parse_card("not toml [").is_err());
}

#[test]
fn card_sources_are_read_from_every_card() {
    let f = common::fixture();
    fs::create_dir_all(f.sd().join("mibpkg")).unwrap();
    fs::write(f.sd().join("mibpkg/sources.toml"), "[[source]]\nname = \"alice\"\nsource = \"github:alice/mib-tools\"\n").unwrap();
    let p = sourcelist::card_sources(&f.layout);
    assert_eq!(p.entries.len(), 1);
    fs::write(f.sd().join("mibpkg/sources.toml"), "garbage [").unwrap();
    let p = sourcelist::card_sources(&f.layout);
    assert!(p.entries.is_empty());
    assert!(p.skipped[0].contains("sources.toml"), "{:?}", p.skipped);
}

#[test]
fn available_excludes_enabled_sources_and_prefers_card_labels() {
    let f = common::fixture();
    fs::create_dir_all(f.sd().join("mibpkg")).unwrap();
    fs::write(f.sd().join("mibpkg/sources.toml"), "[[source]]\nname = \"Alice (card)\"\nsource = \"github:alice/mib-tools\"\n\n[[source]]\nname = \"Bob\"\nsource = \"github:bob/x\"\n").unwrap();
    let list = sourcelist::parse_list(LIST.as_bytes()).unwrap().entries;
    let a = sourcelist::available(&f.layout, &list);
    // The default source is already enabled: not available.
    assert!(a.iter().all(|x| x.source != online::DEFAULT_SOURCE), "{a:?}");
    let alice = a.iter().find(|x| x.source == "github:alice/mib-tools").unwrap();
    assert_eq!((alice.name.as_str(), alice.origin), ("Alice (card)", "card"));
    assert!(a.iter().any(|x| x.source == "github:bob/x" && x.origin == "card"));
    online::add_source(&f.layout, "github:bob/x").unwrap();
    assert!(sourcelist::available(&f.layout, &list).iter().all(|x| x.source != "github:bob/x"));
}

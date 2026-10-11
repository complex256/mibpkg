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
    let a = sourcelist::available(&f.layout, &list, &sourcelist::card_sources(&f.layout).entries);
    // The default source is already enabled: not available.
    assert!(a.iter().all(|x| x.source != online::DEFAULT_SOURCE), "{a:?}");
    let alice = a.iter().find(|x| x.source == "github:alice/mib-tools").unwrap();
    assert_eq!((alice.name.as_str(), alice.origin), ("Alice (card)", "card"));
    assert!(a.iter().any(|x| x.source == "github:bob/x" && x.origin == "card"));
    online::add_source(&f.layout, "github:bob/x").unwrap();
    assert!(sourcelist::available(&f.layout, &list, &sourcelist::card_sources(&f.layout).entries).iter().all(|x| x.source != "github:bob/x"));
}

use mibpkg::ctx::Ctx;
use mibpkg::fetch::MemoryFetcher;
use mibpkg::sys::RecordingSys;

const LIST_URL: &str = "https://github.com/complex256/mibpkg-packages/releases/latest/download/sources.json";

#[test]
fn list_urls() {
    assert_eq!(sourcelist::list_url("github:o/r").unwrap().unwrap(), "https://github.com/o/r/releases/latest/download/sources.json");
    assert_eq!(sourcelist::list_url("https://x.org/s/sources.json").unwrap().unwrap(), "https://x.org/s/sources.json");
    assert_eq!(sourcelist::list_url("").unwrap(), None);
    assert!(sourcelist::list_url("http://x.org/sources.json").is_err());
}

fn list_fetcher() -> MemoryFetcher {
    let m = MemoryFetcher::default();
    m.files.borrow_mut().insert(LIST_URL.into(), LIST.as_bytes().to_vec());
    m
}

#[test]
fn the_check_fetches_and_caches_the_list() {
    let f = common::fixture();
    let sys = RecordingSys::default();
    let ctx = Ctx::new(f.layout.clone(), &sys);
    let m = list_fetcher();
    let r = online::check(&ctx, &m);
    assert_eq!(r.list_error, None);
    let c = sourcelist::load_cache(&f.layout).unwrap();
    assert_eq!(c.entries.len(), 2);
    assert_eq!(c.url, LIST_URL);
    // Fresh (fetched just now with a real host clock): not fetched again.
    let before = m.calls.borrow().iter().filter(|u| u.as_str() == LIST_URL).count();
    online::check(&ctx, &m);
    assert_eq!(m.calls.borrow().iter().filter(|u| u.as_str() == LIST_URL).count(), before);
}

#[test]
fn a_stale_or_future_cache_is_refetched() {
    let f = common::fixture();
    let now = sourcelist::real_now(&f.layout).expect("host clock is real");
    let m = list_fetcher();
    for fetched_at in [now - 25 * 3600, now + 3600] {
        let c = sourcelist::ListCache { fetched_at, url: LIST_URL.into(), entries: vec![], skipped: vec![] };
        fs::create_dir_all(f.layout.state_dir()).unwrap();
        fs::write(sourcelist::cache_path(&f.layout), serde_json::to_string(&c).unwrap()).unwrap();
        assert!(sourcelist::fetch_list(&f.layout, &m, Some(now)).is_some(), "fetched_at {fetched_at}");
    }
    // Unknown real time (unit at boot, no offset yet): always fetched.
    let c = sourcelist::ListCache { fetched_at: now, url: LIST_URL.into(), entries: vec![], skipped: vec![] };
    fs::write(sourcelist::cache_path(&f.layout), serde_json::to_string(&c).unwrap()).unwrap();
    assert!(sourcelist::fetch_list(&f.layout, &m, None).is_some());
    assert!(sourcelist::fetch_list(&f.layout, &m, Some(now)).is_none(), "fresh cache is used");
}

#[test]
fn a_failed_list_fetch_keeps_the_cache_and_does_not_fail_the_check() {
    let f = common::fixture();
    let sys = RecordingSys::default();
    let ctx = Ctx::new(f.layout.clone(), &sys);
    online::check(&ctx, &list_fetcher());
    let old = sourcelist::load_cache(&f.layout).unwrap();
    // Force a refetch against a server that no longer has the list.
    let mut stale = old.clone();
    stale.fetched_at = 0;
    fs::write(sourcelist::cache_path(&f.layout), serde_json::to_string(&stale).unwrap()).unwrap();
    let r = online::check(&ctx, &MemoryFetcher::default());
    assert!(r.list_error.as_deref().unwrap().contains("404"), "{:?}", r.list_error);
    assert_eq!(sourcelist::load_cache(&f.layout).unwrap().entries, old.entries);
    assert!(r.summary(100).contains("list:"), "{}", r.summary(100));
    assert!(online::load_errors(&f.layout).list.unwrap().contains("404"));
}

#[test]
fn an_empty_online_list_setting_turns_the_list_off() {
    let f = common::fixture();
    fs::create_dir_all(f.layout.home()).unwrap();
    fs::write(f.layout.home().join("settings.toml"), "online_list = \"\"\n").unwrap();
    let layout = mibpkg::paths::Layout::load(f.root()).unwrap();
    let m = list_fetcher();
    assert!(sourcelist::fetch_list(&layout, &m, None).is_none());
}

#[test]
fn an_oversized_card_sources_toml_is_refused_unread() {
    let f = common::fixture();
    fs::create_dir_all(f.sd().join("mibpkg")).unwrap();
    let mut big = String::from("[[source]]\nname = \"alice\"\nsource = \"github:alice/mib-tools\"\n");
    big.push_str(&"#".repeat(mibpkg::fetch::MAX_INDEX as usize + 1));
    fs::write(f.sd().join("mibpkg/sources.toml"), big).unwrap();
    let p = sourcelist::card_sources(&f.layout);
    assert!(p.entries.is_empty());
    assert!(p.skipped[0].contains("too large"), "{:?}", p.skipped);
}

//! Online sources added and removed on the unit (web interface), on top of settings.toml.
mod common;
use mibpkg::online::{self, EffectiveSource, SourceOrigin, DEFAULT_SOURCE};
use std::fs;

fn find<'a>(list: &'a [EffectiveSource], s: &str) -> Option<&'a EffectiveSource> {
    list.iter().find(|e| e.source == s)
}

#[test]
fn the_default_source_is_listed_and_enabled() {
    let f = common::fixture();
    let list = online::effective_sources(&f.layout);
    assert_eq!(list.len(), 1);
    assert_eq!(list[0].source, DEFAULT_SOURCE);
    assert_eq!(list[0].origin, SourceOrigin::Default);
    assert!(list[0].enabled);
}

#[test]
fn added_sources_appear_and_can_be_removed() {
    let f = common::fixture();
    online::add_source(&f.layout, "github:someone/carstuff").unwrap();
    let list = online::effective_sources(&f.layout);
    let e = find(&list, "github:someone/carstuff").unwrap();
    assert_eq!(e.origin, SourceOrigin::Added);
    assert!(e.enabled);
    assert!(online::add_source(&f.layout, "github:someone/carstuff").unwrap_err().to_string().contains("already"));
    online::remove_source(&f.layout, "github:someone/carstuff").unwrap();
    assert!(find(&online::effective_sources(&f.layout), "github:someone/carstuff").is_none());
}

#[test]
fn invalid_sources_are_refused() {
    let f = common::fixture();
    for bad in ["http://x/index.json", "github:a", "github:a/b/c", "$(id)", "", "github:a/b;reboot"] {
        assert!(online::add_source(&f.layout, bad).is_err(), "{bad:?}");
    }
    assert!(!online::state_path(&f.layout).exists(), "nothing written");
}

#[test]
fn removing_a_default_source_disables_it_and_drops_its_cache() {
    let f = common::fixture();
    let cache = online::cache_path(&f.layout, "complex256");
    fs::create_dir_all(cache.parent().unwrap()).unwrap();
    let cached = format!(
        r#"{{"source":"{DEFAULT_SOURCE}","base_url":"https://x/","index":{{"format":1,"collection":{{"id":"complex256","name":"c"}},"packages":[]}}}}"#
    );
    fs::write(&cache, cached).unwrap();
    online::remove_source(&f.layout, DEFAULT_SOURCE).unwrap();
    let list = online::effective_sources(&f.layout);
    assert!(!find(&list, DEFAULT_SOURCE).unwrap().enabled);
    assert!(!cache.exists());
    // Adding it back re-enables it.
    online::add_source(&f.layout, DEFAULT_SOURCE).unwrap();
    let list = online::effective_sources(&f.layout);
    let e = find(&list, DEFAULT_SOURCE).unwrap();
    assert!(e.enabled);
    assert_eq!(e.origin, SourceOrigin::Default);
}

#[test]
fn removing_an_unknown_source_is_an_error() {
    let f = common::fixture();
    assert!(online::remove_source(&f.layout, "github:no/body").is_err());
}

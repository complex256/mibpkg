mod common;
use mibpkg::ctx::Ctx;
use mibpkg::fetch::MemoryFetcher;
use mibpkg::index::{Collection, Location};
use mibpkg::install;
use mibpkg::online::{self, base_of, index_url};
use mibpkg::state::State;
use mibpkg::sys::RecordingSys;
use std::fs;

const SRC: &str = "github:complex256/mibpkg-packages";
const URL: &str = "https://github.com/complex256/mibpkg-packages/releases/latest/download/index.json";
const BASE: &str = "https://github.com/complex256/mibpkg-packages/releases/latest/download/";

#[test]
fn sources_resolve_to_urls() {
    assert_eq!(index_url(SRC).unwrap(), URL);
    assert_eq!(index_url("https://h.example/mib/index.json").unwrap(), "https://h.example/mib/index.json");
    for bad in ["http://h/index.json", "github:onlyowner", "gitlab:a/b", "github:a/b/c"] {
        assert!(index_url(bad).is_err(), "{bad}");
    }
    assert_eq!(base_of(URL), BASE);
}

/// An online index: flat file names, like release assets.
fn published(f: &common::Fixture, versions: &[&str]) -> (MemoryFetcher, Vec<Vec<u8>>) {
    let srcs: Vec<_> = versions.iter().map(|v| common::pkg_src(f.root(), "demo", v, "sleep 30", "")).collect();
    let dir = common::collection(&f.root().join("pub"), "complex256", &srcs);
    let mut index: serde_json::Value = serde_json::from_str(&fs::read_to_string(dir.join("index.json")).unwrap()).unwrap();
    let m = MemoryFetcher::default();
    let mut blobs = vec![];
    for e in index["packages"].as_array_mut().unwrap() {
        let file = e["file"].as_str().unwrap().to_string();
        let bytes = fs::read(dir.join(&file)).unwrap();
        let flat = file.trim_start_matches("packages/").to_string();
        m.files.borrow_mut().insert(format!("{BASE}{flat}"), bytes.clone());
        blobs.push(bytes);
        e["file"] = flat.into();
    }
    m.files.borrow_mut().insert(URL.into(), serde_json::to_vec(&index).unwrap());
    (m, blobs)
}

#[test]
fn check_caches_verified_indexes_and_reports_updates() {
    let f = common::fixture();
    let sys = RecordingSys::default();
    let ctx = Ctx::new(f.layout.clone(), &sys);
    let (m, _) = published(&f, &["1.0.0", "1.1.0"]);
    let mut st = State::default();
    st.packages.insert("demo".into(), mibpkg::state::Installed { collection: "complex256".into(), version: "1.0.0".into(), previous: None, services: vec![], uses_scripts: false });
    st.save(&f.layout).unwrap();
    let r = online::check(&ctx, &m);
    assert_eq!(r.errors, Vec::<String>::new());
    assert_eq!(r.updates, vec![("demo".to_string(), "1.1.0".to_string())]);
    assert!(r.summary(132).contains("1 update"), "{}", r.summary(132));
    let colls = online::collections(&f.layout);
    assert_eq!(colls.len(), 1);
    let e = colls[0].latest("demo").unwrap();
    assert_eq!(colls[0].location(e), Location::Remote(format!("{BASE}demo-1.1.0.mpk")));
    // Unchanged index: the cache file is not rewritten (flash).
    let calls_before = sys.calls.borrow().len();
    online::check(&ctx, &m);
    assert_eq!(sys.calls.borrow().len(), calls_before, "no rw remount when nothing changed");
}

#[test]
fn network_failures_keep_the_previous_cache() {
    let f = common::fixture();
    let sys = RecordingSys::default();
    let ctx = Ctx::new(f.layout.clone(), &sys);
    let (m, _) = published(&f, &["1.0.0"]);
    online::check(&ctx, &m);
    let empty = MemoryFetcher::default(); // everything 404s
    let r = online::check(&ctx, &empty);
    assert_eq!(r.errors.len(), 1);
    assert!(r.errors[0].contains("404"), "{:?}", r.errors);
    assert_eq!(online::collections(&f.layout).len(), 1, "previous cache still usable");
    let bad = MemoryFetcher::default();
    bad.files.borrow_mut().insert(URL.into(), b"{not json".to_vec());
    assert_eq!(online::check(&ctx, &bad).errors.len(), 1);
    assert_eq!(online::collections(&f.layout)[0].latest("demo").unwrap().version, "1.0.0");
}

#[test]
fn online_indexes_refuse_non_https_package_urls() {
    let mut idx: mibpkg::index::Index = serde_json::from_str(
        r#"{"format":1,"collection":{"id":"c","name":"c"},"packages":[{"name":"a","version":"1.0.0","file":"http://evil/a.mpk","sha256":"0","size":1}]}"#,
    ).unwrap();
    assert!(Collection::from_online(idx.clone(), BASE).is_err());
    idx.packages[0].file = "https://cdn.example/a.mpk".into();
    let c = Collection::from_online(idx.clone(), BASE).unwrap();
    assert_eq!(c.location(&c.index.packages[0]), Location::Remote("https://cdn.example/a.mpk".into()));
    idx.packages[0].file = "../a.mpk".into();
    assert!(Collection::from_online(idx, BASE).is_err());
}

#[test]
fn newest_compatible_across_card_and_online_wins() {
    let f = common::fixture();
    let sys = RecordingSys::default();
    let ctx = Ctx::new(f.layout.clone(), &sys);
    let card = Collection::load(&common::collection(&f.sd(), "complex256", &[common::pkg_src(f.root(), "demo", "1.0.0", "sleep 30", "")])).unwrap();
    let (m, _) = published(&f, &["1.2.0"]);
    online::check(&ctx, &m);
    let mut colls = vec![card.clone()];
    colls.extend(online::collections(&f.layout));
    let st = State::default();
    let picked = install::pick_collection(&st, &colls, "demo", Some("complex256"), &ctx.unit).unwrap();
    assert!(picked.base_url.is_some(), "online 1.2.0 beats card 1.0.0");
    // Offline, no cache: the card still works.
    let only_card = vec![card];
    assert!(install::pick_collection(&st, &only_card, "demo", None, &ctx.unit).unwrap().base_url.is_none());
}

#[test]
fn installing_from_online_downloads_then_cleans_up() {
    let f = common::fixture();
    let sys = RecordingSys::default();
    let mut ctx = Ctx::new(f.layout.clone(), &sys);
    let (m, _) = published(&f, &["1.1.0"]);
    online::check(&ctx, &m);
    ctx.fetcher = Box::new(m);
    let colls = online::collections(&f.layout);
    let msg = install::install(&ctx, &[], &colls[0], "demo").unwrap();
    assert_eq!(msg, "installed demo 1.1.0");
    assert!(f.layout.pkg_dir("demo").join("1.1.0/bin/demo").is_file());
    assert!(!f.layout.run("dl.demo-1.1.0.mpk").exists(), "download removed");
}

#[test]
fn a_corrupted_download_installs_nothing() {
    let f = common::fixture();
    let sys = RecordingSys::default();
    let mut ctx = Ctx::new(f.layout.clone(), &sys);
    let (m, _) = published(&f, &["1.1.0"]);
    online::check(&ctx, &m);
    m.files.borrow_mut().insert(format!("{BASE}demo-1.1.0.mpk"), b"truncated".to_vec());
    ctx.fetcher = Box::new(m);
    let colls = online::collections(&f.layout);
    let e = install::install(&ctx, &[], &colls[0], "demo").unwrap_err();
    assert!(format!("{e:#}").contains("corrupt"), "{e:#}");
    assert!(install::current_version(&f.layout, "demo").is_none());
    assert!(!f.layout.run("dl.demo-1.1.0.mpk").exists(), "download removed on failure too");
}

#[test]
fn equal_versions_prefer_the_card_copy() {
    // Offline with the same version cached online and on the card: the card must win.
    let f = common::fixture();
    let sys = RecordingSys::default();
    let ctx = Ctx::new(f.layout.clone(), &sys);
    let card = Collection::load(&common::collection(&f.sd(), "complex256", &[common::pkg_src(f.root(), "demo", "1.1.0", "sleep 30", "")])).unwrap();
    let (m, _) = published(&f, &["1.1.0"]);
    online::check(&ctx, &m);
    let mut colls = vec![card];
    colls.extend(online::collections(&f.layout));
    let st = State::default();
    assert!(install::pick_collection(&st, &colls, "demo", Some("complex256"), &ctx.unit).unwrap().base_url.is_none());
}

#[test]
fn fetching_writes_nothing_until_stored() {
    // The background check fetches without the action lock and only takes it to store.
    let f = common::fixture();
    let sys = RecordingSys::default();
    let ctx = Ctx::new(f.layout.clone(), &sys);
    let (m, _) = published(&f, &["1.0.0"]);
    let fetched = online::fetch_all(&ctx, &m);
    assert!(sys.calls.borrow().is_empty(), "no remount while fetching");
    assert!(online::collections(&f.layout).is_empty());
    let r = online::store(&ctx, fetched);
    assert_eq!(r.errors, Vec::<String>::new());
    assert_eq!(online::collections(&f.layout).len(), 1);
}

#[test]
fn unit_fetcher_finds_the_ca_bundle_in_the_installed_package() {
    // A car upgraded from 0.2.x by its old binary only has ca.pem inside pkgs/mibpkg/<ver>/bin.
    let f = common::fixture();
    let ver = f.layout.pkg_dir("mibpkg").join("0.3.0/bin");
    fs::create_dir_all(&ver).unwrap();
    fs::write(ver.join("ca.pem"), "x").unwrap();
    fs::write(f.layout.pkg_dir("mibpkg").join("current"), "0.3.0").unwrap();
    let c = mibpkg::fetch::CurlFetcher::for_unit(&f.layout, None);
    assert_eq!(c.ca_bundle.as_deref(), Some(ver.join("ca.pem").as_path()));
    fs::create_dir_all(f.layout.bin_dir()).unwrap();
    fs::remove_file(ver.join("ca.pem")).unwrap();
    fs::write(f.layout.bin_dir().join("ca.pem"), "y").unwrap();
    let c = mibpkg::fetch::CurlFetcher::for_unit(&f.layout, None);
    assert_eq!(c.ca_bundle.as_deref(), Some(f.layout.bin_dir().join("ca.pem").as_path()));
}

#[test]
fn unit_fetcher_measures_the_clock_itself_when_needed() {
    let f = common::fixture();
    let mut c = mibpkg::fetch::CurlFetcher::for_unit(&f.layout, None);
    c.time_servers = vec!["127.0.0.1:9".into()];
    assert!(c.verify_offset().is_err(), "no offset and no reachable server: no clock");
    fs::write(f.layout.run("clock_offset"), "77").unwrap();
    assert_eq!(c.verify_offset().unwrap(), Some(77), "uses the offset measured this boot");
}

#[test]
fn a_second_source_cannot_take_an_enabled_sources_collection_id() {
    let f = common::fixture();
    let sys = RecordingSys::default();
    let ctx = Ctx::new(f.layout.clone(), &sys);
    let (m, _) = published(&f, &["1.0.0"]);
    // The same index (collection id "complex256") also served by a second source.
    let index = m.files.borrow().get(URL).unwrap().clone();
    m.files.borrow_mut().insert("https://github.com/mallory/copy/releases/latest/download/index.json".into(), index);
    online::check(&ctx, &m);
    online::add_source(&f.layout, "github:mallory/copy").unwrap();
    let r = online::check(&ctx, &m);
    assert_eq!(r.errors.len(), 1, "{:?}", r.errors);
    assert!(r.errors[0].contains("already provided by"), "{:?}", r.errors);
    assert!(online::load_errors(&f.layout).sources["github:mallory/copy"].contains("already provided by"));
    // Removing the owner frees the id for the other source.
    online::remove_source(&f.layout, mibpkg::online::DEFAULT_SOURCE).unwrap();
    let r = online::check(&ctx, &m);
    assert!(r.errors.is_empty(), "{:?}", r.errors);
    let cached: online::Cached = serde_json::from_str(&fs::read_to_string(online::cache_path(&f.layout, "complex256")).unwrap()).unwrap();
    assert_eq!(cached.source, "github:mallory/copy");
}

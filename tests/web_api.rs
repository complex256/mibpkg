//! Read-only API handlers: everything the UI shows comes from these.
mod common;
use mibpkg::ctx::Ctx;
use mibpkg::index::Collection;
use mibpkg::install;
use mibpkg::sys::RecordingSys;
use mibpkg::web::{api, App};
use serde_json::Value;
use std::fs;

const CONF: &str = "[[config]]\npath = \"demo.conf\"\nexample = \"demo.conf.example\"\nrequired = true\n";

/// demo 1.0.0 installed from a card collection that also has 1.1.0; an online collection with
/// "extra"; a config the package requires is missing.
fn setup() -> (common::Fixture, App) {
    let f = common::fixture();
    let v1 = common::pkg_src(f.root(), "demo", "1.0.0", "sleep 30", CONF);
    fs::write(v1.join("demo.conf.example"), "broker = \"example\"\n").unwrap();
    let mut m = fs::read_to_string(v1.join("manifest.toml")).unwrap();
    m.push_str("\n[[file]]\nsrc = \"demo.conf.example\"\n");
    fs::write(v1.join("manifest.toml"), m).unwrap();
    let c1 = Collection::load(&common::collection(&f.sd(), "complex256", &[v1.clone()])).unwrap();
    let sys = RecordingSys::default();
    let ctx = Ctx::new(f.layout.clone(), &sys);
    install::install(&ctx, &[f.sd()], &c1, "demo").unwrap();
    let v2 = common::pkg_src(f.root(), "demo", "1.1.0", "sleep 30", "");
    common::collection(&f.sd(), "complex256", &[v1, v2]);

    let extra = common::pkg_src(f.root(), "extra", "0.2.0", "sleep 30", "");
    let pub_dir = common::collection(&f.root().join("pub"), "community", &[extra]);
    let index: Value = serde_json::from_str(&fs::read_to_string(pub_dir.join("index.json")).unwrap()).unwrap();
    let cached = serde_json::json!({"source": "github:someone/community", "base_url": "https://example.invalid/", "index": index});
    let cache = mibpkg::online::cache_path(&f.layout, "community");
    fs::create_dir_all(cache.parent().unwrap()).unwrap();
    fs::write(cache, cached.to_string()).unwrap();

    fs::write(f.layout.run("log"), "line one\nline two\nline three\n").unwrap();
    let app = App::new(f.layout.clone(), Box::new(|| None));
    (f, app)
}

fn body(r: mibpkg::web::http::Response) -> (u16, Value) {
    (r.status, serde_json::from_slice(&r.body).unwrap_or(Value::Null))
}

#[test]
fn packages_merge_installed_card_and_online() {
    let (_f, app) = setup();
    let (status, v) = body(api::packages(&app));
    assert_eq!(status, 200);
    let list = v.as_array().unwrap();
    let demo = list.iter().find(|p| p["name"] == "demo").unwrap();
    assert_eq!(demo["installed"]["version"], "1.0.0");
    assert_eq!(demo["installed"]["collection"], "complex256");
    assert_eq!(demo["update"], "1.1.0");
    assert_eq!(demo["installed"]["missing_config"][0], "demo.conf");
    assert_eq!(demo["installed"]["configs"][0]["path"], "demo.conf");
    assert_eq!(demo["available"][0]["origin"], "card");
    assert_eq!(demo["description"], "demo test package");
    let extra = list.iter().find(|p| p["name"] == "extra").unwrap();
    assert!(extra["installed"].is_null());
    assert_eq!(extra["available"][0]["origin"], "online");
    assert_eq!(extra["available"][0]["collection"], "community");
    assert_eq!(extra["available"][0]["compatible"], true);
}

#[test]
fn status_reports_version_and_not_busy() {
    let (_f, app) = setup();
    let (status, v) = body(api::status(&app));
    assert_eq!(status, 200);
    assert_eq!(v["version"], env!("CARGO_PKG_VERSION"));
    assert_eq!(v["busy"], false);
    assert_eq!(v["updates"], 1);
    assert!(v["unit"].as_str().unwrap().contains("MHI2Q"));
}

#[test]
fn logs_are_tailed_and_ids_are_checked() {
    let (_f, app) = setup();
    let r = api::log_tail(&app, "mibpkg", 64 * 1024);
    assert_eq!(r.status, 200);
    assert_eq!(String::from_utf8(r.body).unwrap(), "line one\nline two\nline three\n");
    let r = api::log_tail(&app, "mibpkg", 12);
    assert_eq!(String::from_utf8(r.body).unwrap(), "line three\n", "cut at a line start");
    for bad in ["../etc/passwd", "nope", "demo.demo/../../x", ""] {
        assert_eq!(api::log_tail(&app, bad, 100).status, 404, "{bad}");
    }
    // A known service whose log does not exist yet is an empty log, not an error.
    let r = api::log_tail(&app, "demo.demo", 100);
    assert_eq!(r.status, 200);
    let (_, logs) = body(api::logs(&app));
    let ids: Vec<&str> = logs.as_array().unwrap().iter().map(|l| l["id"].as_str().unwrap()).collect();
    assert!(ids.contains(&"mibpkg") && ids.contains(&"demo.demo") && ids.contains(&"mibpkg.web"), "{ids:?}");
}

#[test]
fn config_shows_the_example_until_a_file_exists() {
    let (f, app) = setup();
    let (status, v) = body(api::config_get(&app, "demo", 0));
    assert_eq!(status, 200);
    assert_eq!(v["exists"], false);
    assert_eq!(v["required"], true);
    assert_eq!(v["content"], "broker = \"example\"\n");
    fs::create_dir_all(f.layout.config_dir("demo")).unwrap();
    fs::write(f.layout.config_dir("demo").join("demo.conf"), "broker = \"real\"\n").unwrap();
    let (_, v) = body(api::config_get(&app, "demo", 0));
    assert_eq!(v["exists"], true);
    assert_eq!(v["content"], "broker = \"real\"\n");
    assert_eq!(v["card"], Value::Null);
    // In conflict: the card's copy is shown next to the car's.
    fs::write(mibpkg::config::pending_path(&f.layout, "demo", "demo.conf"), "broker = \"card\"\n").unwrap();
    let (_, v) = body(api::config_get(&app, "demo", 0));
    assert_eq!(v["content"], "broker = \"real\"\n");
    assert_eq!(v["card"], "broker = \"card\"\n");
    assert_eq!(api::config_get(&app, "demo", 1).status, 404);
    assert_eq!(api::config_get(&app, "nope", 0).status, 404);
}

#[test]
fn sources_and_settings_never_leak_the_pin() {
    let (f, _) = setup();
    let mut s = mibpkg::web::settings::WebSettings::default();
    s.pin = Some(mibpkg::web::settings::hash_pin("2468"));
    mibpkg::web::settings::save(&f.layout, &s).unwrap();
    let app = App::new(f.layout.clone(), Box::new(|| None));
    let r = api::settings(&app);
    let text = String::from_utf8(r.body.clone()).unwrap();
    assert!(!text.contains(s.pin.as_ref().unwrap().split(':').next().unwrap()), "{text}");
    let (_, v) = body(r);
    assert_eq!(v["pin_set"], true);
    assert_eq!(v["listen"][0], "hotspot");
    let (_, v) = body(api::sources(&app));
    assert_eq!(v["sources"][0]["source"], mibpkg::online::DEFAULT_SOURCE);
}

#[test]
fn moved_packages_name_where_their_update_comes_from() {
    let f = common::fixture();
    let sys = mibpkg::sys::RecordingSys::default();
    let v1 = common::pkg_src(f.root(), "demo", "1.0.0", "sleep 30", "");
    let old = common::collection(&f.sd(), "old", &[v1]);
    {
        let ctx = Ctx::new(f.layout.clone(), &sys);
        let colls = mibpkg::media::loaded(&mibpkg::media::scan(&f.layout));
        install::install(&ctx, &mibpkg::media::roots(&f.layout), &colls[0], "demo").unwrap();
    }
    fs::remove_dir_all(old).unwrap();
    let v2 = common::pkg_src(f.root(), "demo", "1.1.0", "sleep 30", "");
    common::collection(&f.sd(), "new", &[v2]);
    let app = App::new(f.layout.clone(), Box::new(|| None));
    let (_, v) = body(api::packages(&app));
    let demo = v.as_array().unwrap().iter().find(|p| p["name"] == "demo").unwrap();
    assert_eq!(demo["update"], "1.1.0");
    assert_eq!(demo["update_from"], "new");
}

#[test]
fn sources_lists_what_could_be_enabled() {
    let (f, _) = setup();
    fs::create_dir_all(f.sd().join("mibpkg")).unwrap();
    fs::write(f.sd().join("mibpkg/sources.toml"), "[[source]]\nname = \"Alice\"\nsource = \"github:alice/mib-tools\"\n").unwrap();
    let cache = mibpkg::sourcelist::ListCache {
        fetched_at: 1_800_000_000,
        url: "https://github.com/complex256/mibpkg-packages/releases/latest/download/sources.json".into(),
        entries: vec![mibpkg::sourcelist::ListEntry { name: "Bob".into(), description: "tools".into(), source: "github:bob/x".into() }],
        skipped: vec!["entry 2: name must be 1-60 characters".into()],
    };
    mibpkg::sourcelist::store_cache(&f.layout, &cache).unwrap();
    fs::write(f.layout.run("online-errors.json"), r#"{"sources":{"github:complex256/mibpkg-packages":"HTTP 404"},"list":null}"#).unwrap();
    let app = App::new(f.layout.clone(), Box::new(|| None));
    let (_, v) = body(api::sources(&app));
    let avail = v["available"].as_array().unwrap();
    assert_eq!(avail.len(), 2, "{v}");
    assert_eq!(avail[0]["origin"], "card");
    assert_eq!(avail[1]["source"], "github:bob/x");
    assert_eq!(v["list"]["fetched_at"], 1_800_000_000u64);
    assert_eq!(v["list"]["skipped"][0], "entry 2: name must be 1-60 characters");
    assert_eq!(v["sources"][0]["error"], "HTTP 404");
}

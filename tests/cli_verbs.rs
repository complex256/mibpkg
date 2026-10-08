//! Action verbs the web interface runs through `mibpkg gem <verb>` (one write path with GEM).
mod common;
use mibpkg::cli::run;
use mibpkg::state::State;
use std::fs;

const ID: &str = "0123456789abcdef0123456789abcdef";

fn gem(f: &common::Fixture, rest: &[&str]) -> String {
    let mut v = vec!["--root".to_string(), f.root().display().to_string(), "--no-supervisor".to_string(), "gem".to_string()];
    v.extend(rest.iter().map(|s| s.to_string()));
    assert_eq!(run(&v).unwrap(), 0, "gem never fails");
    let st = State::load(&f.layout).unwrap();
    let key = match rest[0] {
        "install-file" | "config-set" => rest[1],
        "self-update" => "mibpkg",
        _ => "_",
    };
    st.results.get(key).cloned().unwrap_or_else(|| st.results.get("_").cloned().unwrap_or_default())
}

fn upload(f: &common::Fixture, name: &str, version: &str, extra: &str) {
    let src = common::pkg_src(f.root(), name, version, "sleep 30", extra);
    mibpkg::archive::pack(&src, &f.layout.run(&format!("upload.{ID}.mpk"))).unwrap();
}

#[test]
fn install_file_installs_the_upload_and_removes_it() {
    let f = common::fixture();
    upload(&f, "demo", "1.0.0", "");
    let r = gem(&f, &["install-file", "demo", ID, "-"]);
    assert!(r.starts_with("OK: installed demo 1.0.0"), "{r}");
    assert_eq!(State::load(&f.layout).unwrap().packages["demo"].collection, "uploaded");
    assert!(!f.layout.run(&format!("upload.{ID}.mpk")).exists());
}

#[test]
fn install_file_rejects_bad_ids_and_collections() {
    let f = common::fixture();
    upload(&f, "demo", "1.0.0", "");
    for bad in ["../x", "ZZZZ", "0123456789ABCDEF0123456789ABCDEF", ""] {
        let r = gem(&f, &["install-file", "demo", bad, "-"]);
        assert!(r.starts_with("FAILED: invalid id"), "{bad:?}: {r}");
    }
    let r = gem(&f, &["install-file", "demo", ID, "--root"]);
    assert!(r.starts_with("FAILED: invalid collection"), "{r}");
    assert!(State::load(&f.layout).unwrap().packages.is_empty());
}

#[test]
fn config_set_saves_the_file_and_requests_a_restart() {
    let f = common::fixture();
    upload(&f, "demo", "1.0.0", "[[config]]\npath = \"demo.conf\"\nrequired = true\n");
    gem(&f, &["install-file", "demo", ID, "-"]);
    fs::write(f.layout.run(&format!("text.{ID}")), "broker = \"x\"\n").unwrap();
    let r = gem(&f, &["config-set", "demo", "0", ID]);
    assert!(r.starts_with("OK: saved demo.conf"), "{r}");
    assert_eq!(fs::read_to_string(f.layout.config_dir("demo").join("demo.conf")).unwrap(), "broker = \"x\"\n");
    assert!(f.layout.run("restart.demo.demo").exists());
    assert!(!f.layout.run(&format!("text.{ID}")).exists());
    fs::write(f.layout.run(&format!("text.{ID}")), "x").unwrap();
    assert!(gem(&f, &["config-set", "demo", "1", ID]).starts_with("FAILED"));
    assert!(gem(&f, &["config-set", "demo", "-1", ID]).starts_with("FAILED"));
}

#[test]
fn web_settings_are_validated_saved_and_reset() {
    let f = common::fixture();
    let web_toml = f.layout.home().join("web.toml");
    fs::write(f.layout.run(&format!("text.{ID}")), r#"{"listen":["any","hotspot"]}"#).unwrap();
    assert!(gem(&f, &["web-settings", ID]).starts_with("FAILED"));
    assert!(!web_toml.exists());
    fs::write(f.layout.run(&format!("text.{ID}")), r#"{"listen":["tailscale"],"port":8080}"#).unwrap();
    assert!(gem(&f, &["web-settings", ID]).starts_with("OK"));
    let text = fs::read_to_string(&web_toml).unwrap();
    assert!(text.contains("port = 8080") && text.contains("tailscale"), "{text}");
    assert!(gem(&f, &["web-reset"]).starts_with("OK"));
    assert!(!web_toml.exists());
    assert!(f.layout.run("restart.mibpkg.web").exists());
    // A reload makes the supervisor start the web server again if web.toml had disabled it.
    assert!(f.layout.run("reload").exists());
    std::fs::remove_file(f.layout.run("reload")).unwrap();
    fs::write(f.layout.run(&format!("text.{ID}")), r#"{"listen":["hotspot"]}"#).unwrap();
    assert!(gem(&f, &["web-settings", ID]).starts_with("OK"));
    assert!(f.layout.run("reload").exists(), "web-settings reloads too (enabled may change)");
}

#[test]
fn sources_can_be_added_and_removed() {
    let f = common::fixture();
    let r = gem(&f, &["source-add", "github:someone/carstuff"]);
    assert!(r.starts_with("OK: added github:someone/carstuff"), "{r}");
    assert!(gem(&f, &["source-add", "http://bad"]).starts_with("FAILED"));
    assert!(gem(&f, &["source-remove", "github:someone/carstuff"]).starts_with("OK: removed"));
}

#[test]
fn web_clear_pin_removes_only_the_pin() {
    let f = common::fixture();
    let mut s = mibpkg::web::settings::WebSettings::default();
    s.port = 8080;
    s.listen = vec!["tailscale".into()];
    s.pin = Some(mibpkg::web::settings::hash_pin("2468"));
    mibpkg::web::settings::save(&f.layout, &s).unwrap();
    assert!(gem(&f, &["web-clear-pin"]).starts_with("OK: PIN removed"));
    let (now, warning) = mibpkg::web::settings::load(&f.layout);
    assert!(warning.is_none());
    assert_eq!(now.pin, None);
    assert_eq!((now.port, now.listen.clone()), (8080, vec!["tailscale".to_string()]), "everything else kept");
    assert!(f.layout.run("restart.mibpkg.web").exists(), "the web server restarts without the PIN");
    assert!(gem(&f, &["web-clear-pin"]).starts_with("OK: no PIN was set"));
}

#[test]
fn self_update_checks_online_before_installing() {
    // The fixture has no internet and no mibpkg anywhere: the check runs (and says so), then the
    // install fails plainly.
    let f = common::fixture();
    let r = gem(&f, &["self-update"]);
    assert!(r.starts_with("FAILED") && r.contains("online:"), "{r}");
}

#[test]
fn gem_screens_redraws_the_page_without_recording_a_result() {
    let f = common::fixture();
    fs::write(f.layout.run("web.url"), "port 80\nhotspot http://10.255.1.1\n").unwrap();
    let v: Vec<String> = ["--root", f.root().to_str().unwrap(), "--no-supervisor", "gem-screens"].iter().map(|s| s.to_string()).collect();
    assert_eq!(run(&v).unwrap(), 0);
    let esd = fs::read_to_string(f.layout.engdefs().join("mibpkg.esd")).unwrap();
    assert!(esd.contains("hotspot: http://10.255.1.1"), "{esd}");
    assert!(State::load(&f.layout).unwrap().results.is_empty(), "no result row churn");
}

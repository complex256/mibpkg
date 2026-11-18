//! The config form API: GET carries a form description, POST /form applies edits server-side.
mod common;
use mibpkg::ctx::Ctx;
use mibpkg::index::Collection;
use mibpkg::install;
use mibpkg::sys::RecordingSys;
use mibpkg::web::http::Request;
use mibpkg::web::jobs::RecordingRunner;
use mibpkg::web::{api, routes, App};
use serde_json::Value;
use std::fs;

const CONF: &str = "[[config]]\npath = \"demo.toml\"\n\n[[config]]\npath = \"demo.conf\"\n";

fn setup() -> (common::Fixture, App) {
    let f = common::fixture();
    let v1 = common::pkg_src(f.root(), "demo", "1.0.0", "sleep 30", CONF);
    let c1 = Collection::load(&common::collection(&f.sd(), "complex256", &[v1])).unwrap();
    let sys = RecordingSys::default();
    let ctx = Ctx::new(f.layout.clone(), &sys);
    install::install(&ctx, &[f.sd()], &c1, "demo").unwrap();
    fs::create_dir_all(f.layout.config_dir("demo")).unwrap();
    let app = App::new(f.layout.clone(), Box::new(|| None)).with_runner(Box::new(RecordingRunner::default()));
    (f, app)
}

/// A package whose first config is `broker.toml`, optionally with an example and a schema shipped in it.
fn setup_shipped(example: Option<&str>, schema: Option<&str>) -> (common::Fixture, App) {
    let f = common::fixture();
    let mut conf = String::from("[[config]]\npath = \"broker.toml\"\n");
    if example.is_some() {
        conf.push_str("example = \"broker.toml.example\"\n");
    }
    if schema.is_some() {
        conf.push_str("schema = \"broker.schema.toml\"\n");
    }
    let v1 = common::pkg_src(f.root(), "demo", "1.0.0", "sleep 30", &conf);
    if let Some(t) = example {
        fs::write(v1.join("broker.toml.example"), t).unwrap();
    }
    if let Some(t) = schema {
        fs::write(v1.join("broker.schema.toml"), t).unwrap();
    }
    let c1 = Collection::load(&common::collection(&f.sd(), "complex256", &[v1])).unwrap();
    let sys = RecordingSys::default();
    let ctx = Ctx::new(f.layout.clone(), &sys);
    install::install(&ctx, &[f.sd()], &c1, "demo").unwrap();
    fs::create_dir_all(f.layout.config_dir("demo")).unwrap();
    let app = App::new(f.layout.clone(), Box::new(|| None)).with_runner(Box::new(RecordingRunner::default()));
    (f, app)
}

const SCHEMA: &str = "[[field]]\nkey = \"port\"\nlabel = \"Port number\"\ntype = \"int\"\nmax = 100\nsection = \"Net\"\n";

fn put_file(f: &common::Fixture, name: &str, bytes: &[u8]) {
    fs::write(f.layout.config_dir("demo").join(name), bytes).unwrap();
}

fn body(r: mibpkg::web::http::Response) -> (u16, Value) {
    (r.status, serde_json::from_slice(&r.body).unwrap_or(Value::Null))
}

fn post(app: &App, name: &str, n: usize, json: &str) -> (u16, Value) {
    let req = Request {
        method: "POST".into(),
        path: format!("/api/packages/{name}/config/{n}/form"),
        query: vec![],
        headers: vec![("host".into(), "10.0.0.1".into()), ("x-mibpkg".into(), "1".into())],
        body: json.as_bytes().to_vec(),
    };
    body(routes::handle(app, &req, "10.0.0.1".parse().unwrap()))
}

/// The text a started save job was handed.
fn saved_text(f: &common::Fixture) -> String {
    let run = f.layout.run("x");
    let dir = run.parent().unwrap();
    for e in fs::read_dir(dir).unwrap().flatten() {
        if e.file_name().to_string_lossy().starts_with("mibpkg.text.") {
            return fs::read_to_string(e.path()).unwrap();
        }
    }
    panic!("no text file was staged");
}

const EDIT: &str = r#"{"edits":[{"key":"port","value":9}]}"#;

fn too_big() -> String {
    format!("port = 1\n# {}\n", "x".repeat(api::MAX_CONFIG as usize))
}

#[test]
fn get_includes_form_for_toml() {
    let (f, app) = setup();
    put_file(&f, "demo.toml", b"port = 1\n");
    let (s, v) = body(api::config_get(&app, "demo", 0));
    assert_eq!(s, 200);
    assert_eq!(v["content"], "port = 1\n");
    assert_eq!(v["form"]["mode"], "inferred");
    assert_eq!(v["form"]["fields"][0]["key"], "port");
}

#[test]
fn get_form_is_null_for_non_toml() {
    let (f, app) = setup();
    put_file(&f, "demo.conf", b"a=1\n");
    let (_, v) = body(api::config_get(&app, "demo", 1));
    assert!(v["form"].is_null());
    assert_eq!(v["content"], "a=1\n");
}

#[test]
fn get_form_is_raw_mode_over_size_cap() {
    let (f, app) = setup();
    put_file(&f, "demo.toml", too_big().as_bytes());
    let (s, v) = body(api::config_get(&app, "demo", 0));
    assert_eq!(s, 200);
    assert_eq!(v["form"]["mode"], "raw");
    assert!(v["form"]["reason"].as_str().unwrap().contains("large"));
    assert_eq!(v["form"]["fields"].as_array().unwrap().len(), 0);
}

#[test]
fn get_form_is_raw_mode_for_non_utf8() {
    let (f, app) = setup();
    put_file(&f, "demo.toml", b"name = \"\xff\xfe\"\n");
    let (_, v) = body(api::config_get(&app, "demo", 0));
    assert_eq!(v["form"]["mode"], "raw");
    assert!(v["form"]["reason"].as_str().unwrap().contains("UTF-8"));
}

#[test]
fn post_refused_over_size_cap_and_non_utf8() {
    let (f, app) = setup();
    put_file(&f, "demo.toml", too_big().as_bytes());
    let (s, v) = post(&app, "demo", 0, EDIT);
    assert!(s == 400 || s == 409, "{s}");
    assert!(v["error"].as_str().unwrap().contains("large"));
    put_file(&f, "demo.toml", b"port = \"\xff\"\n");
    let (s, v) = post(&app, "demo", 0, EDIT);
    assert!(s == 400 || s == 409, "{s}");
    assert!(v["error"].as_str().unwrap().contains("UTF-8"));
}

#[test]
fn post_form_starts_job_and_saves_edited_text() {
    let (f, app) = setup();
    put_file(&f, "demo.toml", b"# keep\nport = 1\n");
    let (s, v) = post(&app, "demo", 0, EDIT);
    assert_eq!(s, 202, "{v}");
    assert!(v["job"].is_string());
    assert_eq!(saved_text(&f), "# keep\nport = 9\n");
}

#[test]
fn post_form_validation_error_is_400_with_fields() {
    let (f, app) = setup();
    put_file(&f, "demo.toml", b"port = 1\n");
    let (s, v) = post(&app, "demo", 0, r#"{"edits":[{"key":"port","value":{"a":1}}]}"#);
    assert_eq!(s, 400);
    assert!(v["error"].is_string());
    assert_eq!(v["fields"][0]["key"], "port");
    assert!(v["fields"][0]["message"].is_string());
}

#[test]
fn post_form_refused_while_card_conflict_pending() {
    let (f, app) = setup();
    put_file(&f, "demo.toml", b"port = 1\n");
    fs::write(mibpkg::config::pending_path(&f.layout, "demo", "demo.toml"), "port = 2\n").unwrap();
    let (s, _) = post(&app, "demo", 0, EDIT);
    assert_eq!(s, 409);
}

#[test]
fn post_form_rejections() {
    let (f, app) = setup();
    put_file(&f, "demo.toml", b"port = 1\n");
    assert_eq!(post(&app, "nope", 0, EDIT).0, 404);
    assert_eq!(post(&app, "demo", 7, EDIT).0, 404);
    assert_eq!(post(&app, "demo", 1, EDIT).0, 400);
    assert_eq!(post(&app, "demo", 0, r#"{"edits":[],"x":1}"#).0, 400);
}

#[test]
fn unreadable_existing_file_is_raw_only_and_refused() {
    use std::os::unix::fs::PermissionsExt;
    let (f, app) = setup();
    put_file(&f, "demo.toml", b"# important\nport = 1\nhost = \"a\"\n");
    let p = f.layout.config_dir("demo").join("demo.toml");
    fs::set_permissions(&p, fs::Permissions::from_mode(0o000)).unwrap();
    if fs::File::open(&p).is_ok() {
        return; // running as root: the file stays readable, nothing to test
    }
    let (_, v) = body(api::config_get(&app, "demo", 0));
    assert_eq!(v["form"]["mode"], "raw");
    assert!(v["form"]["reason"].as_str().unwrap().contains("could not read"));
    let (s, v) = post(&app, "demo", 0, EDIT);
    assert!(s >= 400, "{s}");
    assert!(v["error"].as_str().unwrap().contains("could not read"));
    let dir = f.layout.run("x");
    let staged = fs::read_dir(dir.parent().unwrap()).unwrap().flatten().any(|e| e.file_name().to_string_lossy().starts_with("mibpkg.text."));
    assert!(!staged, "a job was staged");
}

#[test]
fn get_with_a_shipped_schema_is_schema_mode_with_its_labels() {
    let (f, app) = setup_shipped(None, Some(SCHEMA));
    put_file(&f, "broker.toml", b"port = 1\n");
    let (s, v) = body(api::config_get(&app, "demo", 0));
    assert_eq!(s, 200);
    assert_eq!(v["form"]["mode"], "schema", "{v}");
    assert_eq!(v["form"]["fields"][0]["label"], "Port number");
    assert_eq!(v["form"]["fields"][0]["section"], "Net");
}

#[test]
fn post_violating_the_shipped_schema_is_400_and_starts_no_job() {
    let (f, app) = setup_shipped(None, Some(SCHEMA));
    put_file(&f, "broker.toml", b"port = 1\n");
    let (s, v) = post(&app, "demo", 0, r#"{"edits":[{"key":"port","value":500}]}"#);
    assert_eq!(s, 400, "{v}");
    assert_eq!(v["fields"][0]["key"], "port");
    assert!(v["job"].is_null());
    let dir = f.layout.run("x");
    let staged = fs::read_dir(dir.parent().unwrap()).unwrap().flatten().any(|e| e.file_name().to_string_lossy().starts_with("mibpkg.text."));
    assert!(!staged, "a rejected edit must not stage a file");
}

#[test]
fn empty_edits_on_a_missing_file_save_the_example() {
    let example = "# the example\nport = 1883\n";
    let (f, app) = setup_shipped(Some(example), None);
    let (s, v) = post(&app, "demo", 0, r#"{"edits":[]}"#);
    assert_eq!(s, 202, "{v}");
    assert_eq!(saved_text(&f), example);
}

#[test]
fn output_larger_than_the_cap_is_refused_with_409() {
    let (f, app) = setup();
    let pad = "x".repeat(api::MAX_CONFIG as usize - 40);
    put_file(&f, "demo.toml", format!("# {pad}\nport = 1\n").as_bytes());
    let (s, v) = post(&app, "demo", 0, r#"{"edits":[{"key":"name","value":"this makes the file longer than the cap allows it to be"}]}"#);
    assert_eq!(s, 409, "{v}");
    assert!(v["error"].as_str().unwrap().contains("larger than the editor can safely open"));
}

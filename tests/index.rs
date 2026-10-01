mod common;
use mibpkg::index::{cmp_versions, Collection};
use std::cmp::Ordering;
use std::fs;

#[test]
fn build_and_load_collection() {
    let t = tempfile::tempdir().unwrap();
    let a1 = common::pkg_src(t.path(), "alpha", "1.0.0", "sleep 1", "");
    let a2 = common::pkg_src(t.path(), "alpha", "1.10.0", "sleep 1", "");
    let b = common::pkg_src(t.path(), "beta", "0.1.0", "sleep 1", "[compat]\nunits = [\"MHI2Q\"]\n");
    let dir = common::collection(&t.path().join("sd"), "complex256", &[a1, a2, b]);
    let c = Collection::load(&dir).unwrap();
    assert_eq!(c.id(), "complex256");
    assert_eq!(c.index.packages.len(), 3);
    assert_eq!(c.latest("alpha").unwrap().version, "1.10.0");
    let beta = c.latest("beta").unwrap();
    assert_eq!(beta.compat.units, vec!["MHI2Q"]);
    assert_eq!(beta.sha256.len(), 64);
    assert!(c.archive_path(beta).is_file());
    assert!(c.index.signatures.is_empty());
    assert!(c.latest("gamma").is_none());
}

#[test]
fn load_rejects_future_format_and_escaping_paths() {
    let t = tempfile::tempdir().unwrap();
    let d = t.path();
    fs::write(d.join("index.json"), r#"{"format":2,"collection":{"id":"x","name":"x"},"packages":[]}"#).unwrap();
    assert!(format!("{:#}", Collection::load(d).unwrap_err()).contains("format 2"));
    fs::write(
        d.join("index.json"),
        r#"{"format":1,"collection":{"id":"x","name":"x"},"packages":[{"name":"a","version":"1.0.0","file":"../a.mpk","sha256":"0","size":1}]}"#,
    )
    .unwrap();
    assert!(Collection::load(d).is_err());
}

#[test]
fn version_order_is_semver() {
    assert_eq!(cmp_versions("1.10.0", "1.9.0"), Ordering::Greater);
    assert_eq!(cmp_versions("1.0.0", "1.0.0"), Ordering::Equal);
    assert_eq!(cmp_versions("1.0.0-beta", "1.0.0"), Ordering::Less);
}

#[test]
fn build_index_rejects_misnamed_archive() {
    let t = tempfile::tempdir().unwrap();
    let a = common::pkg_src(t.path(), "alpha", "1.0.0", "sleep 1", "");
    let dir = common::collection(&t.path().join("sd"), "c", &[a]);
    fs::rename(dir.join("packages/alpha-1.0.0.mpk"), dir.join("packages/wrong.mpk")).unwrap();
    assert!(mibpkg::index::build_index(&dir, "c", "c").is_err());
}

#[test]
fn load_rejects_bad_entry_names_and_versions() {
    let t = tempfile::tempdir().unwrap();
    let d = t.path();
    for (name, ver) in [("x/../Main", "1.0.0"), ("ok", "1.0")] {
        fs::write(
            d.join("index.json"),
            format!(r#"{{"format":1,"collection":{{"id":"x","name":"x"}},"packages":[{{"name":"{name}","version":"{ver}","file":"packages/a.mpk","sha256":"0","size":1}}]}}"#),
        )
        .unwrap();
        assert!(Collection::load(d).is_err(), "{name} {ver}");
    }
}

#[test]
fn flat_index_lists_packages_next_to_it() {
    let t = tempfile::tempdir().unwrap();
    let a = common::pkg_src(t.path(), "alpha", "1.0.0", "sleep 1", "");
    let out = t.path().join("rel");
    fs::create_dir_all(&out).unwrap();
    mibpkg::archive::pack(&a, &out.join("alpha-1.0.0.mpk")).unwrap();
    let idx = mibpkg::index::build_index_flat(&out, "complex256", "complex256 packages").unwrap();
    assert_eq!(idx.packages[0].file, "alpha-1.0.0.mpk");
}

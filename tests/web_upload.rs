//! Uploaded .mpk files: storage, inspection, and which collection updates should come from.
mod common;
use mibpkg::ctx::Ctx;
use mibpkg::index::Collection;
use mibpkg::install;
use mibpkg::sys::RecordingSys;
use mibpkg::web::upload::{inspect, Inspection, Uploads};
use std::fs;
use std::time::{Duration, Instant};

fn mpk_bytes(f: &common::Fixture, name: &str, version: &str, extra: &str) -> Vec<u8> {
    let src = common::pkg_src(f.root(), name, version, "sleep 30", extra);
    let out = f.root().join(format!("built-{name}-{version}.mpk"));
    mibpkg::archive::pack(&src, &out).unwrap();
    fs::read(out).unwrap()
}

/// A card collection with demo at `version`, built from `bytes` exactly when given.
fn coll(f: &common::Fixture, id: &str, version: &str) -> Collection {
    let src = common::pkg_src(&f.root().join(format!("srcs-{id}")), "demo", version, &format!("echo {id}"), "");
    Collection::load(&common::collection(&f.sd(), id, &[src])).unwrap()
}

fn look(f: &common::Fixture, colls: &[Collection], bytes: &[u8]) -> Inspection {
    let up = Uploads::default();
    let id = up.store(&f.layout, bytes).unwrap();
    inspect(&f.layout, &mibpkg::unit::detect(&f.layout), colls, &id, &up.path(&id).unwrap())
}

#[test]
fn a_file_identical_to_an_index_entry_preselects_that_collection() {
    let f = common::fixture();
    let c = coll(&f, "community", "1.0.0");
    let other = coll(&f, "complex256", "2.0.0");
    let bytes = fs::read(c.archive_path(c.latest("demo").unwrap())).unwrap();
    let i = look(&f, &[c, other], &bytes);
    assert_eq!(i.kind.as_deref(), Some("fresh"));
    assert_eq!(i.preselected.as_deref(), Some("community"));
    assert!(!i.ask);
    assert!(i.candidates.iter().any(|c| c.collection == "community" && c.matches_file));
    assert!(i.problems.is_empty(), "{:?}", i.problems);
}

#[test]
fn one_collection_with_the_name_is_preselected() {
    let f = common::fixture();
    let i = look(&f, &[coll(&f, "community", "0.9.0")], &mpk_bytes(&f, "demo", "1.0.0", ""));
    assert_eq!(i.preselected.as_deref(), Some("community"));
    assert!(!i.ask);
}

#[test]
fn several_collections_and_no_match_asks_and_prefers_the_newest() {
    let f = common::fixture();
    let colls = [coll(&f, "community", "0.9.0"), coll(&f, "complex256", "0.9.0"), coll(&f, "other", "0.5.0")];
    let i = look(&f, &colls, &mpk_bytes(&f, "demo", "1.0.0", ""));
    assert!(i.ask);
    assert_eq!(i.candidates.len(), 3);
    assert_eq!(i.preselected.as_deref(), Some("complex256"), "newest, official wins the tie");
}

#[test]
fn no_collection_has_it_so_none_is_preselected() {
    let f = common::fixture();
    let i = look(&f, &[], &mpk_bytes(&f, "demo", "1.0.0", ""));
    assert_eq!(i.preselected.as_deref(), Some("-"));
    assert!(!i.ask);
    assert_eq!(i.name.as_deref(), Some("demo"));
    assert_eq!(i.services, vec!["demo.demo"]);
}

#[test]
fn upgrades_same_and_older_versions() {
    let f = common::fixture();
    let c = coll(&f, "complex256", "1.0.0");
    let sys = RecordingSys::default();
    install::install(&Ctx::new(f.layout.clone(), &sys), &[f.sd()], &c, "demo").unwrap();
    let up = look(&f, &[c.clone()], &mpk_bytes(&f, "demo", "1.1.0", ""));
    assert_eq!(up.kind.as_deref(), Some("upgrade"));
    assert_eq!(up.installed_collection.as_deref(), Some("complex256"));
    assert!(up.candidates.is_empty() && !up.ask);
    assert!(up.problems.is_empty());
    let same = look(&f, &[c.clone()], &mpk_bytes(&f, "demo", "1.0.0", ""));
    assert_eq!(same.kind.as_deref(), Some("same"));
    assert!(same.problems[0].contains("already installed"));
    let older = look(&f, &[c], &mpk_bytes(&f, "demo", "0.9.0", ""));
    assert_eq!(older.kind.as_deref(), Some("older"));
    assert!(older.problems[0].contains("Roll back"));
}

#[test]
fn incompatible_and_script_using_packages_are_flagged() {
    let f = common::fixture();
    let i = look(&f, &[], &mpk_bytes(&f, "demo", "1.0.0", "[compat]\nunits = [\"MHI2\"]\n"));
    assert!(!i.compatible);
    assert!(i.problems.iter().any(|p| p.contains("not compatible")), "{:?}", i.problems);
    let src = common::pkg_src(f.root(), "scripted", "1.0.0", "sleep 1", "[scripts]\npost_install = \"post.sh\"\n");
    fs::write(src.join("post.sh"), "true\n").unwrap();
    let out = f.root().join("s.mpk");
    mibpkg::archive::pack(&src, &out).unwrap();
    let i = look(&f, &[], &fs::read(out).unwrap());
    assert!(i.uses_scripts);
}

#[test]
fn files_that_are_not_packages_are_problems_not_errors() {
    let f = common::fixture();
    for junk in [&b""[..], b"<html>login</html>", b"PK\x03\x04zipfile", &[0x1f, 0x8b, 0x08, 0, 0, 0]] {
        let i = look(&f, &[], junk);
        assert!(i.name.is_none());
        assert!(i.problems[0].contains("not a valid mibpkg package"), "{:?}", i.problems);
        assert_eq!(i.size, junk.len() as u64);
    }
}

#[test]
fn one_pending_upload_and_expiry() {
    let f = common::fixture();
    let up = Uploads::default();
    let a = up.store(&f.layout, b"one").unwrap();
    let pa = up.path(&a).unwrap();
    let b = up.store(&f.layout, b"two").unwrap();
    assert!(!pa.exists(), "the earlier upload is replaced");
    assert!(up.path(&a).is_none());
    let pb = up.path(&b).unwrap();
    up.sweep_at(&f.layout, Instant::now() + Duration::from_secs(9 * 60));
    assert!(pb.exists());
    up.sweep_at(&f.layout, Instant::now() + Duration::from_secs(11 * 60));
    assert!(!pb.exists() && up.path(&b).is_none());
    // Leftovers from an earlier web process are removed too.
    let stray = f.layout.run("upload.0123456789abcdef0123456789abcdef.mpk");
    fs::write(&stray, "x").unwrap();
    up.sweep_at(&f.layout, Instant::now());
    assert!(!stray.exists());
}

#[test]
fn an_archive_claiming_a_huge_entry_is_refused_before_unpacking() {
    use std::io::{Read, Write};
    let f = common::fixture();
    // Take a real package, claim its first entry is 1 GB (a "gzip bomb" shape), fix the checksum.
    let real = mpk_bytes(&f, "demo", "1.0.0", "");
    let mut tar = Vec::new();
    flate2::read::GzDecoder::new(&real[..]).read_to_end(&mut tar).unwrap();
    // Skip leading directory entries (header only) to the first file entry.
    let mut at = 0;
    while tar[at + 156] != b'0' {
        at += 512;
    }
    let size = format!("{:011o}\0", 1u64 << 30);
    tar[at + 124..at + 136].copy_from_slice(size.as_bytes());
    tar[at + 148..at + 156].copy_from_slice(b"        ");
    let sum: u32 = tar[at..at + 512].iter().map(|b| *b as u32).sum();
    tar[at + 148..at + 156].copy_from_slice(format!("{sum:06o}\0 ").as_bytes());
    let mut gz = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::default());
    gz.write_all(&tar).unwrap();
    let i = look(&f, &[], &gz.finish().unwrap());
    assert!(i.problems[0].contains("too large"), "{:?}", i.problems);
}

mod common;
use mibpkg::gempatch::{self, Status};
use mibpkg::{delta, sha1::sha1_hex};
use std::fs;
use std::path::PathBuf;

fn seed_jar(f: &common::Fixture, bytes: &[u8]) -> PathBuf {
    let dir = f.root().join("mnt/app/eso/hmi/lsd/jars");
    fs::create_dir_all(&dir).unwrap();
    let p = dir.join("AppDevelopment.jar");
    fs::write(&p, bytes).unwrap();
    p
}
fn patch_dir(f: &common::Fixture, stock: &[u8], patched: &[u8]) -> PathBuf {
    let d = f.root().join("patchsrc");
    fs::create_dir_all(&d).unwrap();
    let stock_sha = sha1_hex(stock);
    fs::write(d.join(format!("{stock_sha}.patch")), delta::encode(stock, patched)).unwrap();
    fs::write(
        d.join("patches.toml"),
        format!(
            "[[patch]]\nstock_sha1 = \"{stock_sha}\"\nfile = \"{stock_sha}.patch\"\nresult_sha1 = \"{}\"\n",
            sha1_hex(patched)
        ),
    )
    .unwrap();
    d
}

#[test]
fn apply_patches_a_known_jar_and_verifies() {
    let f = common::fixture();
    let (stock, patched) = (b"STOCK JAR BYTES vXYZ".as_ref(), b"PATCHED JAR BYTES vXYZ+mib".as_ref());
    seed_jar(&f, stock);
    let d = patch_dir(&f, stock, patched);
    let msg = gempatch::apply(&f.layout, &d).unwrap();
    assert!(msg.contains("patched"), "{msg}");
    assert_eq!(fs::read(f.root().join("mnt/app/eso/hmi/lsd/jars/AppDevelopment.jar")).unwrap(), patched);
    gempatch::revert(&f.layout).unwrap();
    assert_eq!(fs::read(f.root().join("mnt/app/eso/hmi/lsd/jars/AppDevelopment.jar")).unwrap(), stock);
}

#[test]
fn unknown_jar_is_a_clean_skip_not_an_error() {
    let f = common::fixture();
    seed_jar(&f, b"SOME OTHER JAR");
    let d = patch_dir(&f, b"STOCK", b"PATCHED");
    let msg = gempatch::apply(&f.layout, &d).unwrap();
    assert!(msg.contains("no patch"), "{msg}");
    assert_eq!(fs::read(f.root().join("mnt/app/eso/hmi/lsd/jars/AppDevelopment.jar")).unwrap(), b"SOME OTHER JAR");
}

#[test]
fn verify_mismatch_leaves_stock_jar_intact() {
    let f = common::fixture();
    let stock = b"STOCK JAR BYTES".as_ref();
    seed_jar(&f, stock);
    let d = patch_dir(&f, stock, b"GOOD PATCHED");
    fs::write(
        d.join("patches.toml"),
        format!(
            "[[patch]]\nstock_sha1 = \"{}\"\nfile = \"{}.patch\"\nresult_sha1 = \"{}\"\n",
            sha1_hex(stock),
            sha1_hex(stock),
            "0".repeat(40)
        ),
    )
    .unwrap();
    assert!(gempatch::apply(&f.layout, &d).is_err());
    assert_eq!(fs::read(f.root().join("mnt/app/eso/hmi/lsd/jars/AppDevelopment.jar")).unwrap(), stock);
}

#[test]
fn status_reports_no_jar() {
    let f = common::fixture();
    let d = f.root().join("empty");
    fs::create_dir_all(&d).unwrap();
    fs::write(d.join("patches.toml"), "").unwrap();
    assert!(matches!(gempatch::status(&f.layout, &d).unwrap(), Status::NoJar));
}

mod common;
use std::fs;

#[test]
fn build_emits_patch_and_toml_that_apply_reads() {
    let d = tempfile::tempdir().unwrap();
    let (stock, patched) = (d.path().join("s.jar"), d.path().join("p.jar"));
    fs::write(&stock, b"STOCK JAR vA").unwrap();
    fs::write(&patched, b"PATCHED JAR vA").unwrap();
    let out = d.path().join("gem-patch");
    let code = mibpkg::cli::run(&[
        "gem-patch".into(),
        "build".into(),
        "--stock".into(),
        stock.display().to_string(),
        "--patched".into(),
        patched.display().to_string(),
        "--out".into(),
        out.display().to_string(),
    ])
    .unwrap();
    assert_eq!(code, 0);
    let toml = fs::read_to_string(out.join("patches.toml")).unwrap();
    assert!(toml.contains("stock_sha1"));
    let stock_sha = mibpkg::sha1::sha1_file(&stock).unwrap();
    assert!(out.join(format!("{stock_sha}.patch")).exists());
    let patch = fs::read(out.join(format!("{stock_sha}.patch"))).unwrap();
    assert_eq!(mibpkg::delta::apply(b"STOCK JAR vA", &patch).unwrap(), b"PATCHED JAR vA");
}

#[test]
fn apply_from_with_no_value_errors_not_panics() {
    // A structured root so Layout::load succeeds and we actually reach the gem-patch arm.
    let f = common::fixture();
    let r = mibpkg::cli::run(&[
        "--root".into(),
        f.root().display().to_string(),
        "--no-supervisor".into(),
        "gem-patch".into(),
        "apply".into(),
        "--from".into(),
    ]);
    assert!(r.is_err(), "expected a clean error, got {r:?}");
}

#[test]
fn rebuilding_same_stock_upserts_so_apply_still_works() {
    let d = tempfile::tempdir().unwrap();
    let (stock, p1, p2) = (d.path().join("s.jar"), d.path().join("p1.jar"), d.path().join("p2.jar"));
    fs::write(&stock, b"STOCK JAR vZ").unwrap();
    fs::write(&p1, b"PATCHED ONE").unwrap();
    fs::write(&p2, b"PATCHED TWO is different").unwrap();
    let out = d.path().join("gp");
    let build = |patched: &std::path::Path| {
        mibpkg::cli::run(&[
            "gem-patch".into(),
            "build".into(),
            "--stock".into(),
            stock.display().to_string(),
            "--patched".into(),
            patched.display().to_string(),
            "--out".into(),
            out.display().to_string(),
        ])
        .unwrap()
    };
    build(&p1);
    build(&p2); // rebuild for the same stock with a different patched jar
    let stock_sha = mibpkg::sha1::sha1_file(&stock).unwrap();
    let toml = fs::read_to_string(out.join("patches.toml")).unwrap();
    assert_eq!(toml.matches("[[patch]]").count(), 1, "stock jar has more than one entry:\n{toml}");
    assert!(toml.contains(&stock_sha));
    // The patch now applies to the latest patched jar, not the stale first entry.
    let patch = fs::read(out.join(format!("{stock_sha}.patch"))).unwrap();
    assert_eq!(mibpkg::delta::apply(b"STOCK JAR vZ", &patch).unwrap(), b"PATCHED TWO is different");
    assert!(toml.contains(&mibpkg::sha1::sha1_hex(b"PATCHED TWO is different")));
}

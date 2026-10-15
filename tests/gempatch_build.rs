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

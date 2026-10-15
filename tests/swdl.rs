use std::fs;

#[test]
fn card_has_matching_hash_and_layout() {
    let d = tempfile::tempdir().unwrap();
    let payload = d.path().join("payload");
    fs::create_dir_all(&payload).unwrap();
    for f in ["mibpkg", "launch.sh", "gem.sh", "ca.pem"] {
        fs::write(payload.join(f), b"x").unwrap();
    }
    let out = d.path().join("card");
    let code = mibpkg::cli::run(&[
        "swdl-card".into(),
        out.display().to_string(),
        "--payload".into(),
        payload.display().to_string(),
        "--version".into(),
        "1.0".into(),
    ])
    .unwrap();
    assert_eq!(code, 0);
    let meta = fs::read_to_string(out.join("metainfo2.txt")).unwrap();
    assert!(meta.contains("skipCheckSignatureAndVariant = \"true\""));
    assert!(meta.contains("FinalScript = \"./mibpkg/final/install.sh\""));
    let script = fs::read(out.join("mibpkg/final/install.sh")).unwrap();
    let want = mibpkg::sha1::sha1_hex(&script);
    assert!(meta.contains(&format!("FinalScriptChecksum = \"{want}\"")), "hash drift");
    assert!(out.join("mibpkg/payload/mibpkg").exists());
    assert!(out.join("_Swdlautorun.txt").exists());
    assert!(!out.join("Swdlautorun.txt").exists());
}

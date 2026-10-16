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

#[test]
fn signed_header_is_prepended_before_our_block() {
    let d = tempfile::tempdir().unwrap();
    let payload = d.path().join("payload");
    std::fs::create_dir_all(&payload).unwrap();
    for f in ["mibpkg", "launch.sh", "gem.sh", "ca.pem"] {
        std::fs::write(payload.join(f), b"x").unwrap();
    }
    // A stand-in for a genuine Audi-signed metainfo header (a signed document).
    let header = "[common]\nvendor = \"ESO\"\nMetafileChecksum = \"37259e4758d7c843f316aaaa306cced5211049cf\"\n\n[Signature]\nsignature1 = \"11583e2be1780d5ee04eb62c71e0d2f1\"\n";
    let hf = d.path().join("header.txt");
    std::fs::write(&hf, header).unwrap();
    let out = d.path().join("card");
    let code = mibpkg::cli::run(&[
        "swdl-card".into(),
        out.display().to_string(),
        "--payload".into(),
        payload.display().to_string(),
        "--version".into(),
        "1.0".into(),
        "--signed-header".into(),
        hf.display().to_string(),
    ])
    .unwrap();
    assert_eq!(code, 0);
    let meta = std::fs::read_to_string(out.join("metainfo2.txt")).unwrap();
    // The signed document comes first, intact, then our own [common] block.
    assert!(meta.starts_with("[common]\nvendor = \"ESO\""), "signed header not first:\n{meta}");
    assert!(meta.contains("[Signature]"));
    assert!(meta.contains("release = \"mibpkg installer v1.0\""));
    // Our final-script hash is still present and matches the shipped install.sh.
    let script = std::fs::read(out.join("mibpkg/final/install.sh")).unwrap();
    assert!(meta.contains(&format!("FinalScriptChecksum = \"{}\"", mibpkg::sha1::sha1_hex(&script))));
}

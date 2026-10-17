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

#[test]
fn download_group_has_a_real_file_entry_so_swdl_lists_it() {
    let d = tempfile::tempdir().unwrap();
    let payload = d.path().join("payload");
    std::fs::create_dir_all(&payload).unwrap();
    for f in ["mibpkg", "launch.sh", "gem.sh", "ca.pem"] {
        std::fs::write(payload.join(f), b"x").unwrap();
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
    // The marker file the File entry copies must exist at version/MARKER.
    let marker = out.join("mibpkg/version/MARKER");
    assert!(marker.exists(), "marker file missing");
    let meta = std::fs::read_to_string(out.join("metainfo2.txt")).unwrap();
    // A real version/0/default/File entry (what makes SWDL offer the update).
    assert!(meta.contains("[mibpkg\\version\\0\\default\\File]"), "no File entry:\n{meta}");
    let msha = mibpkg::sha1::sha1_file(&marker).unwrap();
    let mlen = std::fs::metadata(&marker).unwrap().len();
    assert!(meta.contains(&format!("CheckSum = \"{msha}\"")), "File CheckSum missing/wrong");
    assert!(meta.contains(&format!("FileSize = \"{mlen}\"")), "File FileSize missing/wrong");
    assert!(meta.contains("Source = \"../../MARKER\""));
    assert!(meta.contains("Destination = \"/HBpersistence/SWDL/Log/service/mibpkg\""));
}

fn mib_payload(d: &std::path::Path) -> std::path::PathBuf {
    let p = d.join("payload");
    std::fs::create_dir_all(&p).unwrap();
    for f in ["mibpkg", "launch.sh", "gem.sh", "ca.pem"] {
        std::fs::write(p.join(f), b"x").unwrap();
    }
    p
}

#[test]
fn with_mib_is_the_default_and_emits_both_entry_points() {
    let d = tempfile::tempdir().unwrap();
    let payload = mib_payload(d.path());
    let out = d.path().join("card");
    let code = mibpkg::cli::run(&[
        "swdl-card".into(), out.display().to_string(),
        "--payload".into(), payload.display().to_string(),
        "--version".into(), "1.0".into(),
    ]).unwrap();
    assert_eq!(code, 0);
    // SWDL entry still present
    assert!(out.join("metainfo2.txt").exists());
    assert!(out.join("mibpkg/final/install.sh").exists());
    assert!(out.join("mibpkg/payload/mibpkg").exists());
    // M.I.B. entry present by default
    assert!(out.join("mod/custom.sh").exists(), "no mod/custom.sh");
    assert!(out.join("mod/command.sh").exists(), "no mod/command.sh");
    assert!(out.join("mod/mibpkg/mibpkg").exists(), "no mod/mibpkg payload");
}

#[test]
fn no_mib_flag_omits_the_mib_entry() {
    let d = tempfile::tempdir().unwrap();
    let payload = mib_payload(d.path());
    let out = d.path().join("card");
    let code = mibpkg::cli::run(&[
        "swdl-card".into(), out.display().to_string(),
        "--payload".into(), payload.display().to_string(),
        "--version".into(), "1.0".into(),
        "--no-mib".into(),
    ]).unwrap();
    assert_eq!(code, 0);
    assert!(out.join("metainfo2.txt").exists());        // SWDL still there
    assert!(!out.join("mod").exists(), "mod/ should be absent with --no-mib");
}

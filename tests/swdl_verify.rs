use std::fs;
use std::path::PathBuf;

fn good_card() -> (tempfile::TempDir, PathBuf) {
    let d = tempfile::tempdir().unwrap();
    let payload = d.path().join("p");
    fs::create_dir_all(&payload).unwrap();
    for f in ["mibpkg", "launch.sh", "gem.sh", "ca.pem"] {
        fs::write(payload.join(f), b"x").unwrap();
    }
    let out = d.path().join("card");
    mibpkg::swdl::build_card(&out, &payload, None, "1.0").unwrap();
    (d, out)
}

#[test]
fn clean_card_has_no_problems() {
    let (_d, out) = good_card();
    assert!(mibpkg::swdl::verify_card(&out).unwrap().is_empty());
}

#[test]
fn tampered_script_is_caught() {
    let (_d, out) = good_card();
    fs::write(out.join("mibpkg/final/install.sh"), "#!/bin/sh\necho tampered\n").unwrap();
    let probs = mibpkg::swdl::verify_card(&out).unwrap();
    assert!(probs.iter().any(|p| p.contains("hash")), "{probs:?}");
}

#[test]
fn missing_payload_is_caught() {
    let (_d, out) = good_card();
    fs::remove_file(out.join("mibpkg/payload/mibpkg")).unwrap();
    let probs = mibpkg::swdl::verify_card(&out).unwrap();
    assert!(probs.iter().any(|p| p.contains("mibpkg")), "{probs:?}");
}

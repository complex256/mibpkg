use mibpkg::sha1::{sha1_hex, sha1_file};

#[test]
fn known_vectors() {
    assert_eq!(sha1_hex(b""), "da39a3ee5e6b4b0d3255bfef95601890afd80709");
    assert_eq!(sha1_hex(b"abc"), "a9993e364706816aba3e25717850c26c9cd0d89d");
    // > 64 bytes to exercise multi-block padding
    assert_eq!(
        sha1_hex(&b"a".repeat(1000)),
        "291e9a6c66994949b57ba5e650361e98fc36b1ba"
    );
}

#[test]
fn file_matches_bytes() {
    let d = tempfile::tempdir().unwrap();
    let p = d.path().join("x");
    std::fs::write(&p, b"abc").unwrap();
    assert_eq!(sha1_file(&p).unwrap(), sha1_hex(b"abc"));
}

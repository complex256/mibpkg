use mibpkg::delta::{encode, apply};

#[test]
fn roundtrip_middle_change() {
    let old = b"the quick brown fox jumps over the lazy dog".to_vec();
    let new = b"the quick RED fox leaps over the lazy dog".to_vec();
    let p = encode(&old, &new);
    assert!(p.len() <= new.len() + 64, "patch not larger than full new + header");
    assert_eq!(apply(&old, &p).unwrap(), new);
}

#[test]
fn roundtrip_identity_and_empty() {
    assert_eq!(apply(b"abc", &encode(b"abc", b"abc")).unwrap(), b"abc");
    assert_eq!(apply(b"", &encode(b"", b"xyz")).unwrap(), b"xyz");
}

#[test]
fn wrong_base_is_rejected() {
    let p = encode(b"original base", b"patched output");
    let e = apply(b"different base!", &p).unwrap_err();
    assert!(format!("{e}").contains("base"), "{e}");
}

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

#[test]
fn huge_out_len_errors_not_aborts() {
    // A patch whose recorded out_len is enormous must return Err, never panic/abort
    // (Vec::with_capacity on a multi-GB/overflowing value).
    let mut p = encode(b"abc", b"abc");
    // out_len is the 8 bytes right after MAGIC(4) + base_sha1(20).
    for b in p.iter_mut().skip(24).take(8) {
        *b = 0xFF;
    }
    let e = apply(b"abc", &p).unwrap_err();
    let s = format!("{e}");
    assert!(s.contains("length") || s.contains("too large") || s.contains("range"), "{s}");
}

#[test]
fn copy_overrunning_out_len_errors() {
    // Many COPYs that exceed the declared out_len must bail, not grow unbounded.
    let old = b"abcdefgh";
    // header: MAGIC + sha1(old) + out_len=1, then two COPY (0,8) ops = 16 bytes > 1.
    let mut p = encode(old, &old[..1]); // valid patch producing 1 byte
    // append an extra COPY(0,8) after the legit ops.
    p.push(0x01);
    p.extend_from_slice(&0u64.to_le_bytes());
    p.extend_from_slice(&8u64.to_le_bytes());
    assert!(apply(old, &p).is_err());
}

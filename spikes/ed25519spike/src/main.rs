//! THROWAWAY spike: does a pure-Rust ed25519 verifier build and run on QNX 6.5?
use ed25519_dalek::{Signature, Verifier, VerifyingKey};

fn hex(s: &str) -> Vec<u8> {
    (0..s.len()).step_by(2).map(|i| u8::from_str_radix(&s[i..i + 2], 16).unwrap()).collect()
}

fn main() {
    let pk: [u8; 32] = hex("d75a980182b10ab7d54bfed3c964073a0ee172f3daa62325af021a68f707511a").try_into().unwrap();
    let sig: [u8; 64] = hex("e5564300c360ac729086e2cc806e828a84877f1eb8e5d974d873e065224901555fb8821590a33bacc61e39701cf9b46bd25bf5f0595bbe24655141438e7a100b").try_into().unwrap();
    let key = VerifyingKey::from_bytes(&pk).expect("key");
    let ok = key.verify(b"", &Signature::from_bytes(&sig)).is_ok();
    let bad = key.verify(b"x", &Signature::from_bytes(&sig)).is_ok();
    println!("valid={ok} tampered={bad}");
}

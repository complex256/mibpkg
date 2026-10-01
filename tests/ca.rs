const ROOTS: &[&str] = &[
    "ISRG Root X1", "DigiCert Global Root G2", "DigiCert Global Root CA",
    "DigiCert High Assurance EV Root CA", "USERTrust RSA Certification Authority",
    "USERTrust ECC Certification Authority", "GTS Root R1", "GTS Root R2", "GTS Root R3", "GTS Root R4",
    "Amazon Root CA 1",
];

#[test]
fn ca_bundle_has_exactly_the_listed_roots() {
    let pem = std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/packaging/ca.pem")).unwrap();
    assert_eq!(pem.matches("-----BEGIN CERTIFICATE-----").count(), ROOTS.len());
    for r in ROOTS {
        assert!(pem.contains(&format!("# {r}\n")), "missing {r}");
    }
}

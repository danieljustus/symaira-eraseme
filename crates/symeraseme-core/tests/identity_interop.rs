use std::io::Write;
use std::path::Path;
use std::process::{Command, Stdio};

use symeraseme_core::identity::{
    Profile, ProfileAddress, ProfileError, canonical_json, decrypt_profile_with_key,
    encrypt_profile_with_nonce, hash_profile,
};

fn go_oracle(request: &[u8]) -> Vec<u8> {
    let oracle =
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../rust-tests/parity/oracle/identity");
    let mut child = Command::new("go")
        .args(["run", "."])
        .current_dir(oracle)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("Go identity oracle must start");
    child
        .stdin
        .as_mut()
        .expect("oracle stdin")
        .write_all(request)
        .expect("write oracle request");
    let output = child.wait_with_output().expect("wait for Go oracle");
    assert!(
        output.status.success(),
        "Go oracle failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    output.stdout
}

fn profile() -> Profile {
    Profile {
        full_name: "Jörg Example 漢".to_owned(),
        name_variants: vec!["J. Example".to_owned()],
        date_of_birth: Some("1985-12-31".to_owned()),
        addresses: vec![ProfileAddress {
            street: "Hauptstraße 1".to_owned(),
            city: "Köln".to_owned(),
            postal_code: "50667".to_owned(),
            country: "DE".to_owned(),
            state: Some("NRW".to_owned()),
            valid_from: Some("2020-01-01".to_owned()),
            valid_to: None,
        }],
        email_addresses: vec!["joerg@example.invalid".to_owned()],
        phone_numbers: vec!["+49-221-1234".to_owned()],
        jurisdictions: vec!["DE".to_owned(), "EU".to_owned()],
    }
}

#[test]
fn rust_writer_is_readable_by_go_and_hashes_match() {
    let key = [0x5a; 32];
    let nonce = [0x07; 12];
    let value = profile();
    let plaintext = serde_json::to_vec(&value).expect("serialize profile");
    let envelope = encrypt_profile_with_nonce(&plaintext, &key, &nonce).expect("encrypt");

    let mut request = vec![b'd'];
    request.extend_from_slice(&key);
    request.extend_from_slice(&envelope);
    let go_plaintext = go_oracle(&request);

    let mut canonical_request = vec![b'c'];
    canonical_request.extend_from_slice(&go_plaintext);
    assert_eq!(
        canonical_json(&value).as_bytes(),
        go_oracle(&canonical_request)
    );

    let mut hash_request = vec![b'h'];
    hash_request.extend_from_slice(&go_plaintext);
    assert_eq!(hash_profile(&value).as_bytes(), go_oracle(&hash_request));
}

#[test]
fn go_writer_is_readable_by_rust_and_hashes_match() {
    let key = [0x7b; 32];
    let value = profile();
    let plaintext = serde_json::to_vec(&value).expect("serialize profile");
    let mut request = vec![b'e'];
    request.extend_from_slice(&key);
    request.extend_from_slice(&plaintext);
    let envelope = go_oracle(&request);

    let (decoded, header) = decrypt_profile_with_key(&envelope, &key).expect("decrypt Go profile");
    assert_eq!(header.version, 2);
    assert_eq!(header.algorithm, "AES-256-GCM");
    let decoded: Profile = serde_json::from_slice(&decoded).expect("decode profile");

    let mut canonical_request = vec![b'c'];
    canonical_request.extend_from_slice(&plaintext);
    assert_eq!(
        canonical_json(&decoded).as_bytes(),
        go_oracle(&canonical_request)
    );

    let mut hash_request = vec![b'h'];
    hash_request.extend_from_slice(&plaintext);
    assert_eq!(hash_profile(&decoded).as_bytes(), go_oracle(&hash_request));
}

#[test]
fn envelope_rejects_tampering_malformed_inputs_and_bad_keys() {
    let key = [0x42; 32];
    let nonce = [0x09; 12];
    let envelope =
        encrypt_profile_with_nonce(br#"{"full_name":"test"}"#, &key, &nonce).expect("encrypt");

    let mut ciphertext_tampered = envelope.clone();
    *ciphertext_tampered.last_mut().unwrap() ^= 1;
    assert_eq!(
        decrypt_profile_with_key(&ciphertext_tampered, &key),
        Err(ProfileError::Authentication)
    );

    let mut header_tampered = envelope.clone();
    header_tampered[10] ^= 1;
    assert!(decrypt_profile_with_key(&header_tampered, &key).is_err());

    assert_eq!(
        decrypt_profile_with_key(b"{\"version\":2,\"nonce\":\"00\"}\n", &key),
        Err(ProfileError::Nonce)
    );
    assert_eq!(
        decrypt_profile_with_key(b"not-an-envelope", &key),
        Err(ProfileError::NoSeparator)
    );
    assert!(matches!(
        decrypt_profile_with_key(&envelope, &[0; 31]),
        Err(ProfileError::Key(_))
    ));
}

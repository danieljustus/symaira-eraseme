#[path = "support/frozen_go_oracle.rs"]
mod frozen_go_oracle;
#[path = "support/go_oracle.rs"]
mod go_oracle;

use symeraseme_core::identity::{
    Profile, ProfileAddress, ProfileError, canonical_generic_json, canonical_json,
    decrypt_profile_with_key, encrypt_profile, hash_profile,
};

fn go_oracle(request: &[u8]) -> Vec<u8> {
    if !frozen_go_oracle::live_mode() {
        return frozen_go_oracle::run("identity", request);
    }
    let run = go_oracle::run_oracle("identity", Some(request));
    assert!(
        run.status.success(),
        "Go oracle failed: {}",
        String::from_utf8_lossy(&run.stderr)
    );
    run.stdout
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
    let value = profile();
    let plaintext = serde_json::to_vec(&value).expect("serialize profile");
    let fresh = encrypt_profile(&plaintext, &key).expect("encrypt");
    assert_eq!(decrypt_profile_with_key(&fresh, &key).unwrap().0, plaintext);
    let envelope = if frozen_go_oracle::live_mode() {
        fresh
    } else {
        let recorded = frozen_go_oracle::case("identity-original-rust-writer-go-decrypt");
        assert_eq!(&recorded.request[1..33], &key);
        assert_eq!(recorded.stdout, plaintext);
        assert_ne!(
            fresh,
            recorded.request[33..],
            "production nonce must remain fresh"
        );
        // Only this actual recorded Rust envelope is replayed to its measured
        // Go reader result. The private writer unit test reproduces all bytes.
        recorded.request[33..].to_vec()
    };

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
    let envelope = encrypt_profile(br#"{"full_name":"test"}"#, &key).expect("encrypt");

    let mut ciphertext_tampered = envelope.clone();
    *ciphertext_tampered.last_mut().unwrap() ^= 1;
    assert_eq!(
        decrypt_profile_with_key(&ciphertext_tampered, &key),
        Err(ProfileError::Authentication)
    );

    let mut header_tampered = envelope.clone();
    let algorithm_offset = header_tampered
        .windows(b"AES-256-GCM".len())
        .position(|window| window == b"AES-256-GCM")
        .expect("algorithm header");
    header_tampered[algorithm_offset] = b"B"[0];
    assert_eq!(
        decrypt_profile_with_key(&header_tampered, &key),
        Err(ProfileError::Authentication)
    );

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

#[test]
fn canonical_generic_numbers_match_go_float64_rules() {
    let raw = br#"{"fraction":1.25,"integer":1.0,"large_fraction":1000000.5,"scientific":1e-7}"#;
    let value: serde_json::Value = serde_json::from_slice(raw).expect("JSON numbers");
    let mut request = vec![b'g'];
    request.extend_from_slice(raw);
    assert_eq!(
        canonical_generic_json(&value).as_bytes(),
        go_oracle(&request)
    );
    assert_eq!(
        canonical_generic_json(&value),
        r#"{"fraction": 1.25, "integer": 1, "large_fraction": 1.0000005e+06, "scientific": 1e-07}"#
    );
}

#[test]
fn frozen_go_corpus_rejects_all_recorded_tampering_and_unknown_requests() {
    let mut rejected = 0;
    let mut observations = 0;
    for recorded in frozen_go_oracle::all_cases() {
        observations += 1;
        if recorded.exit_status == 0 {
            assert_eq!(
                frozen_go_oracle::run(recorded.package, recorded.request),
                recorded.stdout
            );
            continue;
        }
        rejected += 1;
        assert!(recorded.stdout.is_empty());
        assert!(!recorded.stderr.is_empty());
        assert_eq!(recorded.request[0], b'd');
        let key = &recorded.request[1..33];
        let envelope = &recorded.request[33..];
        match recorded.package {
            "identity" => assert_eq!(
                decrypt_profile_with_key(envelope, key),
                Err(ProfileError::Authentication)
            ),
            "crypto" => assert!(
                symeraseme_core::storage::encryption::decrypt_v3(envelope, key).is_err(),
                "Rust must reject the same complete tampered envelope Go rejected"
            ),
            _ => panic!("unexpected recorded package"),
        }
        assert!(
            std::panic::catch_unwind(|| {
                frozen_go_oracle::run(recorded.package, recorded.request)
            })
            .is_err()
        );
    }
    assert_eq!(observations, 15);
    assert_eq!(rejected, 5);
    // A valid recorded reader result must not answer a different ciphertext.
    let recorded = frozen_go_oracle::case("identity-original-rust-writer-go-decrypt");
    let mut changed = recorded.request.to_vec();
    changed[1] ^= 1;
    assert!(std::panic::catch_unwind(|| frozen_go_oracle::run("identity", &changed)).is_err());
}

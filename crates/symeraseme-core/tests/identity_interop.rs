use serde::Deserialize;
use std::collections::BTreeMap;
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use symeraseme_core::identity::{
    FakeKeyring, MasterKey, MasterKeyResolver, Profile, ProfileAddress, ProfileError, ProfilePaths,
    canonical_json, decrypt_profile_with_key, delete_profile, encrypt_profile,
    encrypt_profile_with_nonce, hash_profile, load_profile, profile_exists, save_profile,
};

#[derive(Deserialize)]
struct ProvenanceFixture {
    key_hex: String,
    nonce_hex: String,
    profiles: BTreeMap<String, ProvenanceProfile>,
}

#[derive(Deserialize)]
#[allow(dead_code)]
struct ProvenanceProfile {
    file_enc: String,
    canonical_json: String,
    canonical_hash: String,
    header_json: String,
    ciphertext_hex: String,
    payload_json: String,
}

fn fixtures_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tests/fixtures/identity-contract")
}

fn oracle_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../rust-tests/parity/oracle/identity")
}

fn run_go_oracle(request: &[u8]) -> Vec<u8> {
    let mut child = Command::new("go")
        .args(["run", "."])
        .current_dir(oracle_dir())
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
        .expect("write request to oracle");

    let output = child.wait_with_output().expect("wait on Go oracle");
    assert!(
        output.status.success(),
        "Go oracle failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    output.stdout
}

#[test]
fn python_final_provenance_vectors_decrypt_and_match_canonical_hash() {
    let prov_bytes = fs::read(fixtures_root().join("provenance.json"))
        .expect("provenance.json must exist in identity-contract fixtures");
    let prov: ProvenanceFixture =
        serde_json::from_slice(&prov_bytes).expect("parse provenance.json");
    let key = hex::decode(&prov.key_hex).expect("decode key_hex");

    for (name, prof_data) in &prov.profiles {
        let file_path = fixtures_root().join(&prof_data.file_enc);
        let raw = fs::read(&file_path)
            .unwrap_or_else(|_| panic!("read fixture {} ({name})", prof_data.file_enc));

        let (plain, header) = decrypt_profile_with_key(&raw, &key)
            .unwrap_or_else(|err| panic!("decrypt fixture {name}: {err}"));

        assert_eq!(header.version, 2, "envelope version for {name}");
        assert_eq!(header.nonce, prov.nonce_hex, "envelope nonce for {name}");
        assert_eq!(header.algorithm, "AES-256-GCM", "envelope algo for {name}");

        // Parse into Profile model
        let profile: Profile = serde_json::from_slice(&plain)
            .unwrap_or_else(|err| panic!("deserialize {name}: {err}"));

        // Compare canonical JSON string
        let canon = canonical_json(&profile);
        assert_eq!(
            canon, prof_data.canonical_json,
            "canonical JSON mismatch for {name}"
        );

        // Compare deterministic SHA-256 hash
        let hash = hash_profile(&profile);
        assert_eq!(
            hash, prof_data.canonical_hash,
            "profile hash mismatch for {name}"
        );

        // Test deterministic encryption with known nonce
        let nonce_bytes: [u8; 12] = hex::decode(&prov.nonce_hex).unwrap().try_into().unwrap();
        let re_encrypted =
            encrypt_profile_with_nonce(prof_data.payload_json.as_bytes(), &key, &nonce_bytes)
                .expect("encrypt_profile_with_nonce");
        let (re_decrypted, re_hdr) =
            decrypt_profile_with_key(&re_encrypted, &key).expect("re-decrypt");
        assert_eq!(re_decrypted, prof_data.payload_json.as_bytes());
        assert_eq!(re_hdr.nonce, prov.nonce_hex);
    }
}

#[test]
fn rust_writer_is_consumed_by_go_oracle_and_matches_canonical_hash() {
    let key = [0x5au8; 32];
    let profile = Profile {
        full_name: "Rust Test User".to_string(),
        name_variants: vec!["R. User".to_string(), "Tester".to_string()],
        date_of_birth: Some("1992-06-15".to_string()),
        addresses: vec![ProfileAddress {
            street: "Rustway 42".to_string(),
            city: "Berlin".to_string(),
            postal_code: "10117".to_string(),
            country: "DE".to_string(),
            state: Some("Berlin".to_string()),
            valid_from: Some("2021-01-01".to_string()),
            valid_to: None,
        }],
        email_addresses: vec!["rust-user@example.com".to_string()],
        phone_numbers: vec!["+49-30-99887766".to_string()],
        jurisdictions: vec!["DE".to_string(), "EU".to_string()],
    };

    let rust_canon = canonical_json(&profile);
    let rust_hash = hash_profile(&profile);

    // Encrypt with Rust
    let plain_bytes = serde_json::to_vec(&profile).expect("serialize profile");
    let encrypted = encrypt_profile(&plain_bytes, &key).expect("Rust encrypt_profile");

    // Decrypt with Go oracle: op 'd' + 32-byte key + envelope
    let mut decrypt_req = Vec::with_capacity(1 + 32 + encrypted.len());
    decrypt_req.push(b'd');
    decrypt_req.extend_from_slice(&key);
    decrypt_req.extend_from_slice(&encrypted);
    let go_decrypted_plain = run_go_oracle(&decrypt_req);

    // Go computes canonical JSON and hash
    let mut canon_req = Vec::with_capacity(1 + go_decrypted_plain.len());
    canon_req.push(b'c');
    canon_req.extend_from_slice(&go_decrypted_plain);
    let go_canon = run_go_oracle(&canon_req);

    let mut hash_req = Vec::with_capacity(1 + go_decrypted_plain.len());
    hash_req.push(b'h');
    hash_req.extend_from_slice(&go_decrypted_plain);
    let go_hash = run_go_oracle(&hash_req);

    assert_eq!(
        rust_canon,
        String::from_utf8_lossy(&go_canon),
        "Rust and Go canonical JSON must match"
    );
    assert_eq!(
        rust_hash,
        String::from_utf8_lossy(&go_hash),
        "Rust and Go hash must match"
    );
}

#[test]
fn go_writer_is_consumed_by_rust_reader_and_matches_canonical_hash() {
    let key = [0x7bu8; 32];
    let profile = Profile {
        full_name: "Go Created Subject".to_string(),
        name_variants: vec![],
        date_of_birth: None,
        addresses: vec![],
        email_addresses: vec!["go-sub@example.org".to_string()],
        phone_numbers: vec![],
        jurisdictions: vec!["US-CA".to_string()],
    };

    let profile_json = serde_json::to_vec(&profile).expect("serialize profile");

    // Encrypt via Go oracle: op 'e' + 32-byte key + plaintext
    let mut encrypt_req = Vec::with_capacity(1 + 32 + profile_json.len());
    encrypt_req.push(b'e');
    encrypt_req.extend_from_slice(&key);
    encrypt_req.extend_from_slice(&profile_json);
    let go_envelope = run_go_oracle(&encrypt_req);

    // Decrypt in Rust
    let (plain, header) = decrypt_profile_with_key(&go_envelope, &key)
        .expect("Rust must decrypt Go encrypted profile");
    assert_eq!(header.version, 2);
    assert_eq!(header.algorithm, "AES-256-GCM");

    let decoded: Profile = serde_json::from_slice(&plain).expect("deserialize profile");
    assert_eq!(decoded.full_name, profile.full_name);
    assert_eq!(decoded.email_addresses, profile.email_addresses);

    let rust_canon = canonical_json(&decoded);
    let rust_hash = hash_profile(&decoded);

    let mut go_canon_req = Vec::with_capacity(1 + profile_json.len());
    go_canon_req.push(b'c');
    go_canon_req.extend_from_slice(&profile_json);
    let go_canon = run_go_oracle(&go_canon_req);
    assert_eq!(
        rust_canon,
        String::from_utf8_lossy(&go_canon),
        "canonical JSON must match"
    );

    let mut go_hash_req = Vec::with_capacity(1 + profile_json.len());
    go_hash_req.push(b'h');
    go_hash_req.extend_from_slice(&profile_json);
    let go_hash = run_go_oracle(&go_hash_req);

    assert_eq!(
        rust_hash,
        String::from_utf8_lossy(&go_hash),
        "hashes must match"
    );
}

#[test]
fn full_disk_roundtrip_go_to_rust_to_go() {
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join("identity.encrypted");
    let key_bytes = [0x33u8; 32];
    let master_key = MasterKey::from_bytes(&key_bytes).unwrap();

    let original = Profile {
        full_name: "Roundtrip User".to_string(),
        name_variants: vec!["RT User".to_string()],
        date_of_birth: Some("1988-08-08".to_string()),
        addresses: vec![ProfileAddress {
            street: "Test St 1".to_string(),
            city: "Hamburg".to_string(),
            postal_code: "20095".to_string(),
            country: "DE".to_string(),
            state: None,
            valid_from: None,
            valid_to: None,
        }],
        email_addresses: vec!["rt@example.de".to_string()],
        phone_numbers: vec!["+49-40-1234567".to_string()],
        jurisdictions: vec!["DE".to_string()],
    };

    let original_json = serde_json::to_vec(&original).expect("serialize");

    // 1. Go saves profile to disk
    let path_str = path.to_str().unwrap();
    let mut save_req = Vec::new();
    save_req.push(b's');
    save_req.extend_from_slice(&key_bytes);
    let path_len = path_str.len() as u32;
    save_req.extend_from_slice(&path_len.to_be_bytes());
    save_req.extend_from_slice(path_str.as_bytes());
    save_req.extend_from_slice(&original_json);
    let go_saved_path = run_go_oracle(&save_req);
    assert_eq!(String::from_utf8_lossy(&go_saved_path), path_str);

    // 2. Rust loads profile from disk
    let paths = ProfilePaths::new(Some(dir.path().to_owned()), BTreeMap::new());
    let keyring = FakeKeyring::new();
    let mut resolver = MasterKeyResolver::new(keyring);
    resolver.set_cached(master_key.clone());

    assert!(profile_exists(&path, &paths));
    let rust_loaded =
        load_profile(&path, &paths, &mut resolver).expect("Rust must load profile written by Go");

    assert_eq!(rust_loaded.full_name, original.full_name);
    assert_eq!(rust_loaded.addresses.len(), 1);
    assert_eq!(rust_loaded.addresses[0].city, "Hamburg");
    assert_eq!(hash_profile(&rust_loaded), hash_profile(&original));

    // 3. Rust modifies and saves back to disk
    let mut updated = rust_loaded;
    updated.name_variants.push("Second Nick".to_string());
    let saved_target =
        save_profile(&updated, &path, &paths, &master_key).expect("Rust save_profile must succeed");
    assert_eq!(saved_target, path);

    // 4. Go loads profile from disk
    let mut load_req = Vec::new();
    load_req.push(b'l');
    load_req.extend_from_slice(&key_bytes);
    load_req.extend_from_slice(path_str.as_bytes());
    let go_loaded_canon = run_go_oracle(&load_req);

    assert_eq!(
        canonical_json(&updated),
        String::from_utf8_lossy(&go_loaded_canon),
        "Go must load profile re-saved by Rust with identical canonical JSON"
    );

    // 5. Delete profile and verify gone
    delete_profile(&path, &paths).expect("delete profile");
    assert!(!profile_exists(&path, &paths));
    assert_eq!(
        delete_profile(&path, &paths).unwrap_err(),
        ProfileError::NotFound
    );
}

#[test]
fn filesystem_permissions_and_directory_modes() {
    let dir = tempfile::tempdir().expect("tempdir");
    let target_dir = dir.path().join("sub").join("data");
    let file_path = target_dir.join("identity.encrypted");
    let key = MasterKey::from_bytes(&[0x11u8; 32]).unwrap();
    let profile = Profile {
        full_name: "Perm User".to_string(),
        ..Default::default()
    };
    let paths = ProfilePaths::new(Some(dir.path().to_owned()), BTreeMap::new());

    save_profile(&profile, &file_path, &paths, &key).expect("save_profile");

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let file_meta = fs::metadata(&file_path).expect("file metadata");
        assert_eq!(
            file_meta.permissions().mode() & 0o777,
            0o600,
            "profile file must have 0600 mode"
        );

        let dir_meta = fs::metadata(&target_dir).expect("dir metadata");
        assert_eq!(
            dir_meta.permissions().mode() & 0o777,
            0o700,
            "profile directory must have 0700 mode"
        );
    }

    // Verify no temporary files remain
    let entries = fs::read_dir(&target_dir).expect("read_dir");
    let mut names = Vec::new();
    for entry in entries {
        names.push(entry.unwrap().file_name().into_string().unwrap());
    }
    assert_eq!(names, vec!["identity.encrypted"]);
}

#[test]
fn tampering_truncation_and_bad_keys_fail_closed() {
    let key = [0x42u8; 32];
    let wrong_key = [0x43u8; 32];
    let plain = b"{\"full_name\":\"Tamper Test\"}";
    let envelope = encrypt_profile(plain, &key).expect("encrypt");

    // 1. Wrong key fails
    assert_eq!(
        decrypt_profile_with_key(&envelope, &wrong_key).unwrap_err(),
        ProfileError::Authentication
    );

    // 2. Tampered ciphertext fails
    let mut tampered = envelope.clone();
    let last = tampered.len() - 1;
    tampered[last] ^= 0x01;
    assert_eq!(
        decrypt_profile_with_key(&tampered, &key).unwrap_err(),
        ProfileError::Authentication
    );

    // 3. Tampered header fails (AAD mismatch)
    let sep = envelope.iter().position(|&v| v == b'\n').unwrap();
    let mut tampered_header = envelope.clone();
    tampered_header[2] ^= 0x01; // inside {"version": 2...}
    assert!(decrypt_profile_with_key(&tampered_header, &key).is_err());

    // 4. Truncated envelope fails
    assert_eq!(
        decrypt_profile_with_key(&envelope[..sep], &key).unwrap_err(),
        ProfileError::NoSeparator
    );
    assert_eq!(
        decrypt_profile_with_key(&envelope[..sep + 5], &key).unwrap_err(),
        ProfileError::Authentication
    );

    // 5. Invalid key length fails
    assert!(matches!(
        decrypt_profile_with_key(&envelope, &[0u8; 31]),
        Err(ProfileError::Key(_))
    ));
}

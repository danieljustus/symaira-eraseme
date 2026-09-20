#![no_main]

use libfuzzer_sys::fuzz_target;
use symeraseme_core::storage::encryption::{
    decrypt_any, decrypt_v1, decrypt_v2, decrypt_v3, detect_version, encrypt_v3, is_encrypted,
    is_legacy_go_envelope,
};

const MASTER_KEY: &[u8; 32] = b"symaira-eraseme-golden-master-32";

fuzz_target!(|input: &[u8]| {
    // 1. Structural detection must never panic
    let _ = detect_version(input);
    let _ = is_encrypted(input);
    let _ = is_legacy_go_envelope(input);

    // 2. Targeted version decryptors must fail closed without panic
    let _ = decrypt_v1(input, MASTER_KEY);
    let _ = decrypt_v2(input, MASTER_KEY);
    let _ = decrypt_v3(input, MASTER_KEY);

    // 3. decrypt_any parsing with standard master key
    if let Ok(plaintext) = decrypt_any(input, MASTER_KEY) {
        // Roundtrip property: if decrypt_any accepts the envelope, re-encrypting as V3
        // and decrypting must produce identical plaintext bytes.
        let re_encrypted = encrypt_v3(&plaintext, MASTER_KEY)
            .expect("re-encrypting valid plaintext as V3 must succeed");
        let re_decrypted = decrypt_v3(&re_encrypted, MASTER_KEY)
            .expect("decrypting re-encrypted V3 ciphertext must succeed");
        assert_eq!(re_decrypted, plaintext);
    }

    // 4. Test arbitrary master key if input provides at least 32 bytes
    if input.len() >= 32 {
        let (key_slice, payload) = input.split_at(32);
        let _ = decrypt_any(payload, key_slice);
    }
});

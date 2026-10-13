//! Reader controls for auxiliary HTTP records, in their own binary so the
//! count-gated comparators gain no test.
//!
//! Mutate an immutable native record instead of recapturing the current tree.
//! Reader validation is portable; this is not a new native runtime observation.
#[path = "support/frozen_http_aux.rs"]
#[allow(dead_code)]
mod frozen_http_aux;

use serde_json::Value;
use sha2::{Digest, Sha256};
use std::panic::catch_unwind;
const FILE: &str = "crates/symeraseme-cli/tests/mcp_http_native_contract.rs";
const TEST: &str = "native_bind_failures_match_checked_out_go";
const REVISION: &str = "13ddc6a43036d6b34706db2b82bbb5a13c40d6de";
const RECORD_PIN: &str = "5ccae33a51528f5a1f286bb521ade37361dfcd7076bccf936d7391fd2f382280";
const RECORD: &[u8] = include_bytes!(
    "../../../tests/fixtures/go-frozen/refresh-13ddc6a4/http-aux/darwin-arm64/native_bind_failures_match_checked_out_go.json"
);

fn pin(raw: &[u8]) -> String {
    Sha256::digest(raw)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

#[test]
fn frozen_http_aux_rejects_corrupt_truncated_and_fabricated_records() {
    let revision = REVISION.to_owned();
    let target = "darwin/arm64";
    let raw = RECORD.to_vec();
    let record: Value = serde_json::from_slice(&raw).unwrap();
    let verify = |raw: &[u8], pin: &str, revision: &str| {
        frozen_http_aux::verify(raw, pin, revision, target, FILE, TEST)
    };
    // Positive control: the negatives below fail for their mutation alone.
    verify(&raw, RECORD_PIN, &revision);

    let mut corrupt = raw.clone();
    corrupt.push(b'!');
    assert!(catch_unwind(|| verify(&corrupt, &pin(&raw), &revision)).is_err());
    // Captured files end in a newline: remove JSON content, not just whitespace.
    let last_content = raw
        .iter()
        .rposition(|byte| !byte.is_ascii_whitespace())
        .unwrap();
    assert_eq!(raw[last_content], b'}');
    let truncated = &raw[..last_content];
    assert!(catch_unwind(|| verify(truncated, &pin(&raw), &revision)).is_err());
    assert!(catch_unwind(|| verify(truncated, &pin(truncated), &revision)).is_err());

    let mut fabricated = record.clone();
    fabricated["source_revision"] = "fabricated-source".into();
    let fabricated = serde_json::to_vec_pretty(&fabricated).unwrap();
    assert!(catch_unwind(|| verify(&fabricated, &pin(&fabricated), &revision)).is_err());
    assert!(catch_unwind(|| verify(&fabricated, &pin(&fabricated), "fabricated-source")).is_err());

    // Re-pinning does not launder a changed source, generator or omitted input.
    for (field, name) in [
        ("source_files", "go.mod"),
        ("archived_generators", "Cargo.lock"),
    ] {
        let mut changed = record.clone();
        changed[field][name]["sha256"] = "0".repeat(64).into();
        let changed = serde_json::to_vec_pretty(&changed).unwrap();
        assert!(catch_unwind(|| verify(&changed, &pin(&changed), &revision)).is_err());
        let mut omitted = record.clone();
        omitted[field].as_object_mut().unwrap().remove(name);
        let omitted = serde_json::to_vec_pretty(&omitted).unwrap();
        assert!(catch_unwind(|| verify(&omitted, &pin(&omitted), &revision)).is_err());
    }
    let mut modified = record.clone();
    modified["embedded_build_info"] = "\tbuild\tvcs.modified=true\n".into();
    let modified = serde_json::to_vec_pretty(&modified).unwrap();
    assert!(catch_unwind(|| verify(&modified, &pin(&modified), &revision)).is_err());
}

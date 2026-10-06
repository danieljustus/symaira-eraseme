//! Reader controls for auxiliary HTTP records, in their own binary so the
//! count-gated comparators gain no test.
//!
//! The synthetic record below is a reader control only: it carries no Go
//! observation and is never selected by a comparator.
#[path = "support/capture_http_aux.rs"]
#[allow(dead_code)]
mod capture_http_aux;
#[path = "support/frozen_http_aux.rs"]
#[allow(dead_code)]
mod frozen_http_aux;

use serde_json::json;
use sha2::{Digest, Sha256};
use std::panic::catch_unwind;
use std::process::Command;

const ROOT: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../..");
const FILE: &str = "crates/symeraseme-cli/tests/mcp_http_process.rs";
const TEST: &str = "reader_control";

fn pin(raw: &[u8]) -> String {
    Sha256::digest(raw)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

#[test]
fn frozen_http_aux_rejects_corrupt_truncated_and_fabricated_records() {
    // Needs the committed tree: the reader binds the archived and current inputs.
    let head = Command::new("git")
        .args(["rev-parse", "HEAD"])
        .current_dir(ROOT)
        .output()
        .unwrap();
    assert!(head.status.success());
    let revision = String::from_utf8(head.stdout).unwrap().trim().to_owned();
    let target = frozen_http_aux::native_target().expect("recordable native target");
    let (os, arch) = target.split_once('/').unwrap();
    let (sources, generators) = capture_http_aux::current_inputs(FILE);
    let record = json!({
        "schema": "symeraseme.actual-go.http-aux.v1",
        "source_revision": revision, "source_files": sources,
        "archived_generators": generators,
        "checkout_autocrlf": capture_http_aux::checkout_autocrlf(),
        "native_target": target, "go_version": "go1.26.6",
        "embedded_build_info": format!(
            "\tbuild\tGOOS={os}\n\tbuild\tGOARCH={arch}\n\tbuild\tvcs.revision={revision}\n\tbuild\tvcs.modified=false\n"
        ),
        "test_file": FILE, "test": TEST,
        "observation": {"reader_control": true},
    });
    let raw = serde_json::to_vec_pretty(&record).unwrap();
    let verify = |raw: &[u8], pin: &str, revision: &str| {
        frozen_http_aux::verify(raw, pin, revision, target, FILE, TEST)
    };
    // Positive control: the negatives below fail for their mutation alone.
    verify(&raw, &pin(&raw), &revision);

    let mut corrupt = raw.clone();
    corrupt.push(b'!');
    assert!(catch_unwind(|| verify(&corrupt, &pin(&raw), &revision)).is_err());
    let truncated = &raw[..raw.len() - 1];
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

//! Exact published binary identity for the real explicit Go rollback test.
//! Metadata provenance was independently read from all six actual archives.
use sha2::{Digest, Sha256};
use std::path::Path;

const MANIFEST: &[u8] = include_bytes!("../../../../tests/fixtures/go-rollback/v0.12.1.json");
const MANIFEST_SHA256: &str = "98cc9e2b0ec1e9c9d363363cef8302e806235294048d611b2271a3d5538b162f";

fn sha256(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

pub fn stage(retained: &Path, destination: &Path) {
    assert_eq!(
        sha256(MANIFEST),
        MANIFEST_SHA256,
        "published manifest changed"
    );
    let manifest: serde_json::Value = serde_json::from_slice(MANIFEST).unwrap();
    assert_eq!(manifest["tag"], "v0.12.1");
    assert_eq!(
        manifest["source_revision"],
        "240bf67cefa05e643e32611a02e6e7ed87a033ea"
    );
    let os = match std::env::consts::OS {
        "macos" => "darwin",
        other => other,
    };
    let arch = match std::env::consts::ARCH {
        "x86_64" => "amd64",
        "aarch64" => "arm64",
        _ => panic!("unrecorded native rollback target"),
    };
    let target = format!("{os}/{arch}");
    let records = manifest["native_archives"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|record| record["native_target"] == target)
        .collect::<Vec<_>>();
    assert_eq!(records.len(), 1, "exact native published record required");
    assert!(retained.is_absolute(), "retained binary must be absolute");
    let metadata = std::fs::symlink_metadata(retained).unwrap();
    assert!(metadata.is_file() && !metadata.is_symlink());
    let bytes = records[0]["binary_bytes"].as_u64().unwrap();
    assert!(bytes <= 32 * 1024 * 1024);
    assert_eq!(metadata.len(), bytes);
    let raw = std::fs::read(retained).unwrap();
    assert_eq!(raw.len() as u64, bytes);
    assert_eq!(sha256(&raw), records[0]["binary_sha256"].as_str().unwrap());
    assert!(!destination.exists(), "fresh rollback sibling required");
    std::fs::copy(retained, destination).unwrap();
    assert_eq!(std::fs::read(destination).unwrap(), raw);
}

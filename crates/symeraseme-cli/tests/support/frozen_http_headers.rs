//! Original ten complete HTTP response comparisons backed only by measured native Go responses.
use base64::Engine;
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::path::{Component, Path};
use std::process::Command;

#[path = "go_source_pin.rs"]
mod go_source_pin;

const ROOT: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../..");
// Populate only after retaining and independently verifying actual native captures.
const RECORDS: &[(&str, &str, &str, &[u8])] = &[
    (
        "darwin/amd64",
        "411b5c10eb041f0233e0e86710a3dd733b0012e2",
        "bc023c7f4918d0af629d4bdd686067a92e81c0a1ae44c459fc3f14c73bbd798d",
        include_bytes!(
            "../../../../tests/fixtures/go-frozen/refresh-411b5c10/http-headers/darwin-amd64/headers.json"
        ),
    ),
    (
        "darwin/arm64",
        "411b5c10eb041f0233e0e86710a3dd733b0012e2",
        "0165c741ec62d9b4f3a4a5746d5d64bc3dbd71fb6295d5276d17193126b04637",
        include_bytes!(
            "../../../../tests/fixtures/go-frozen/refresh-411b5c10/http-headers/darwin-arm64/headers.json"
        ),
    ),
    (
        "linux/amd64",
        "411b5c10eb041f0233e0e86710a3dd733b0012e2",
        "a5fa9553dd22590c3145ca611dbc394d5f109a9dd727a67c8c226e33bccbea2c",
        include_bytes!(
            "../../../../tests/fixtures/go-frozen/refresh-411b5c10/http-headers/linux-amd64/headers.json"
        ),
    ),
    (
        "linux/arm64",
        "411b5c10eb041f0233e0e86710a3dd733b0012e2",
        "2bb0d6ca7dea12aed15df78f85bbb625b6ed2cd1d91321277112702e5c391300",
        include_bytes!(
            "../../../../tests/fixtures/go-frozen/refresh-411b5c10/http-headers/linux-arm64/headers.json"
        ),
    ),
    (
        "windows/amd64",
        "411b5c10eb041f0233e0e86710a3dd733b0012e2",
        "9570876cb08bd8eb727398c28606b2f0046f6a8b1b3fd88d4fd0f82e01a3bc86",
        include_bytes!(
            "../../../../tests/fixtures/go-frozen/refresh-411b5c10/http-headers/windows-amd64/headers.json"
        ),
    ),
    (
        "windows/arm64",
        "411b5c10eb041f0233e0e86710a3dd733b0012e2",
        "2300b45e989db9141afe30a9f8731c812345eb4d263a4f63cd0adb72bd5a1110",
        include_bytes!(
            "../../../../tests/fixtures/go-frozen/refresh-411b5c10/http-headers/windows-arm64/headers.json"
        ),
    ),
];

fn digest(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

fn verify(raw: &[u8], pin: &str, revision: &str, target: &str) -> Value {
    assert!(raw.len() <= 1024 * 1024);
    assert_eq!(digest(raw), pin, "whole measured HTTP record");
    let record: Value = serde_json::from_slice(raw).unwrap();
    assert_eq!(record["schema"], "symeraseme.actual-go.http-headers.v1");
    assert_eq!(record["source_revision"], revision);
    assert_eq!(record["native_target"], target);
    assert_eq!(record["go_version"], "go1.26.6");
    let build = record["embedded_build_info"].as_str().unwrap();
    let (os, arch) = target.split_once('/').unwrap();
    for expected in [
        format!("vcs.revision={revision}"),
        "vcs.modified=false".into(),
        format!("GOOS={os}"),
        format!("GOARCH={arch}"),
    ] {
        assert!(build.contains(&expected));
    }
    for (field, count) in [("source_files", 171), ("archived_generators", 3)] {
        let files = record[field].as_object().unwrap();
        assert_eq!(files.len(), count);
        for (name, recorded) in files {
            let path = Path::new(name);
            assert!(
                !path.is_absolute() && path.components().all(|c| matches!(c, Component::Normal(_)))
            );
            let archive = Command::new("git")
                .args(["show", &format!("{revision}:{name}")])
                .current_dir(ROOT)
                .output()
                .unwrap();
            assert!(
                archive.status.success(),
                "immutable HTTP producer must remain verifiable"
            );
            let archived_bytes = if field == "archived_generators" && target.starts_with("windows/")
            {
                assert!(!archive.stdout.windows(2).any(|bytes| bytes == b"\r\n"));
                let mut transformed = Vec::new();
                for byte in archive.stdout {
                    if byte == b'\n' {
                        transformed.push(b'\r');
                    }
                    transformed.push(byte);
                }
                transformed
            } else {
                archive.stdout
            };
            assert_eq!(recorded["bytes"], archived_bytes.len(), "{name}");
            assert_eq!(recorded["sha256"], digest(&archived_bytes), "{name}");
            if go_source_pin::current_tree_bound(name) {
                let current = std::fs::read(Path::new(ROOT).join(path)).unwrap();
                go_source_pin::assert_current_matches_archive(name, &archived_bytes, &current);
            }
        }
    }
    let cases = record["cases"].as_array().unwrap();
    assert_eq!(cases.len(), 10);
    for (index, case) in cases.iter().enumerate() {
        assert_eq!(case["index"], index);
        let bytes = base64::engine::general_purpose::STANDARD
            .decode(case["request"]["body_base64"].as_str().unwrap())
            .unwrap();
        assert_eq!(case["request"]["body"]["bytes"], bytes.len());
        assert_eq!(case["request"]["body"]["sha256"], digest(&bytes));
        let raw = base64::engine::general_purpose::STANDARD
            .decode(case["raw_response_base64"].as_str().unwrap())
            .unwrap();
        assert!(raw.len() <= 1024 * 1024);
        assert_eq!(case["raw_response"]["bytes"], raw.len());
        assert_eq!(case["raw_response"]["sha256"], digest(&raw));
    }
    record
}

pub struct Corpus(Value);

impl Corpus {
    pub fn reply(&self, index: usize, request: &Value) -> Vec<u8> {
        let case = &self.0["cases"][index];
        assert_eq!(
            &case["request"], request,
            "original complete HTTP input changed"
        );
        base64::engine::general_purpose::STANDARD
            .decode(case["raw_response_base64"].as_str().unwrap())
            .unwrap()
    }
}

pub fn observations() -> Option<Corpus> {
    if std::env::var_os("SYMERASEME_CAPTURE_HTTP_HEADERS").is_some_and(|value| !value.is_empty()) {
        return None;
    }
    match std::env::var("SYMERASEME_PARITY_LIVE_GO").as_deref() {
        Ok("1") => return None,
        Ok("0") | Err(std::env::VarError::NotPresent) => {}
        _ => panic!("SYMERASEME_PARITY_LIVE_GO must be 0 or 1"),
    }
    let target = match (std::env::consts::OS, std::env::consts::ARCH) {
        ("linux", "x86_64") => "linux/amd64",
        ("linux", "aarch64") => "linux/arm64",
        ("windows", "x86_64") => "windows/amd64",
        ("windows", "aarch64") => "windows/arm64",
        ("macos", "x86_64") => "darwin/amd64",
        ("macos", "aarch64") => "darwin/arm64",
        _ => return None,
    };
    let (_, revision, pin, raw) = RECORDS.iter().find(|record| record.0 == target)?;
    let record = verify(raw, pin, revision, target);
    let mut corrupt = raw.to_vec();
    corrupt.push(b'!');
    assert!(std::panic::catch_unwind(|| verify(&corrupt, pin, revision, target)).is_err());
    assert!(
        std::panic::catch_unwind(|| verify(&raw[..raw.len() - 1], pin, revision, target)).is_err()
    );
    let mut changed = record.clone();
    changed["source_revision"] = "fabricated-source".into();
    assert!(
        std::panic::catch_unwind(|| verify(
            &serde_json::to_vec(&changed).unwrap(),
            pin,
            revision,
            target
        ))
        .is_err()
    );
    Some(Corpus(record))
}

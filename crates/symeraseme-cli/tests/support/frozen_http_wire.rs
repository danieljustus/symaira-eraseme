//! Original ten HTTP comparisons backed only by measured native Go responses.
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
        "67d3b61f8c8cb943a6830e7310fc2c018c7bf081",
        "0408e0af288a9070c97a34a148ea2cd5749ac74ce0c87c57227f053a12d5e31e",
        include_bytes!("../../../../tests/fixtures/go-frozen/http-wire/darwin-amd64/wire.json"),
    ),
    (
        "darwin/arm64",
        "67d3b61f8c8cb943a6830e7310fc2c018c7bf081",
        "f7fd04a723d6660f10c74ac57c97f7f7f44640d71d36e44181c80799223216c1",
        include_bytes!("../../../../tests/fixtures/go-frozen/http-wire/darwin-arm64/wire.json"),
    ),
    (
        "linux/amd64",
        "67d3b61f8c8cb943a6830e7310fc2c018c7bf081",
        "d65e7c935372d9b41f0bfbe82dc2a47907e9a1233a09fac083825d1d3304b7a9",
        include_bytes!("../../../../tests/fixtures/go-frozen/http-wire/linux-amd64/wire.json"),
    ),
    (
        "linux/arm64",
        "67d3b61f8c8cb943a6830e7310fc2c018c7bf081",
        "e1f5b89291b2594c80c55722e1495737e4920c17065c4c54e59a8f140509fc20",
        include_bytes!("../../../../tests/fixtures/go-frozen/http-wire/linux-arm64/wire.json"),
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
    assert_eq!(record["schema"], "symeraseme.actual-go.http-wire.v1");
    assert_eq!(record["source_revision"], revision);
    assert_eq!(record["native_target"], target);
    assert_eq!(record["go_version"], "go1.26.6");
    assert_eq!(record["server_exit_status"], 0);
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
    for (field, count) in [("source_files", 171), ("archived_generators", 4)] {
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
            let current = go_source_pin::current_tree_bound(name)
                .then(|| std::fs::read(Path::new(ROOT).join(path)).unwrap());
            for bytes in std::iter::once(&archive.stdout).chain(&current) {
                assert_eq!(recorded["bytes"], bytes.len(), "{name}");
                assert_eq!(recorded["sha256"], digest(bytes), "{name}");
            }
        }
    }
    let cases = record["cases"].as_array().unwrap();
    assert_eq!(cases.len(), 10);
    for (index, case) in cases.iter().enumerate() {
        assert_eq!(case["index"], index);
        for field in ["request", "response"] {
            let bytes = base64::engine::general_purpose::STANDARD
                .decode(case[field]["body_base64"].as_str().unwrap())
                .unwrap();
            assert!(bytes.len() <= 1024 * 1024);
            assert_eq!(case[field]["body"]["bytes"], bytes.len());
            assert_eq!(case[field]["body"]["sha256"], digest(&bytes));
        }
    }
    record
}

pub struct Corpus(Value);

impl Corpus {
    pub fn reply(&self, index: usize, request: &Value) -> (u16, String, Vec<u8>) {
        let case = &self.0["cases"][index];
        assert_eq!(&case["request"], request, "original HTTP input changed");
        let response = &case["response"];
        (
            u16::try_from(response["status"].as_u64().unwrap()).unwrap(),
            response["content_type"].as_str().unwrap().to_owned(),
            base64::engine::general_purpose::STANDARD
                .decode(response["body_base64"].as_str().unwrap())
                .unwrap(),
        )
    }
}

pub fn observations() -> Option<Corpus> {
    if std::env::var_os("SYMERASEME_CAPTURE_HTTP_WIRE").is_some_and(|value| !value.is_empty()) {
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

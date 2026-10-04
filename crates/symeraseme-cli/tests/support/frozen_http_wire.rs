//! Original ten HTTP comparisons backed only by measured native Go responses.
use base64::Engine;
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::path::{Component, Path};
use std::process::Command;

const ROOT: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../..");
// Populate only after retaining and independently verifying actual native captures.
const RECORDS: &[(&str, &str, &str, &[u8])] = &[(
    "linux/amd64",
    "b518d619f297754970ac7b44f86497aafd1072ad",
    "afc81fa177dc03ee70c6f26ed74334304671d2ea20e6106f9a629876c16e7d32",
    include_bytes!("../../../../tests/fixtures/go-frozen/http-wire/linux-amd64/wire.json"),
)];

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
            let current = std::fs::read(Path::new(ROOT).join(path)).unwrap();
            for bytes in [&archive.stdout, &current] {
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

//! Original ten complete HTTP response comparisons backed only by measured native Go responses.
use base64::Engine;
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::path::{Component, Path};
use std::process::Command;

const ROOT: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../..");
// Populate only after retaining and independently verifying actual native captures.
const RECORDS: &[(&str, &str, &str, &[u8])] = &[
    (
        "darwin/amd64",
        "e8bb6643cbc2a05dbc19f3ad3749513887095ac9",
        "1da4fc4e7ea2325bde7e1e6be27a39b08d3ae9615a1ea0128e42703868aba474",
        include_bytes!(
            "../../../../tests/fixtures/go-frozen/http-headers/darwin-amd64/headers.json"
        ),
    ),
    (
        "darwin/arm64",
        "e8bb6643cbc2a05dbc19f3ad3749513887095ac9",
        "06c30abb149761b020b96921a477a198e1cff56107d6e80a904a7aff9b76cff6",
        include_bytes!(
            "../../../../tests/fixtures/go-frozen/http-headers/darwin-arm64/headers.json"
        ),
    ),
    (
        "linux/amd64",
        "e8bb6643cbc2a05dbc19f3ad3749513887095ac9",
        "fe919a2b65f3a88a06447f49519945969185c9e3763c952f5c5e6d60edc8f594",
        include_bytes!(
            "../../../../tests/fixtures/go-frozen/http-headers/linux-amd64/headers.json"
        ),
    ),
    (
        "linux/arm64",
        "e8bb6643cbc2a05dbc19f3ad3749513887095ac9",
        "c4f59543084535a381c82eac1890591575d888b9628f674c39cdce19a3db2c49",
        include_bytes!(
            "../../../../tests/fixtures/go-frozen/http-headers/linux-arm64/headers.json"
        ),
    ),
    (
        "windows/amd64",
        "e8bb6643cbc2a05dbc19f3ad3749513887095ac9",
        "e0c360ca217c23855db783083d0709f8353d7ec115f7dfbaee3f56d5c675cea4",
        include_bytes!(
            "../../../../tests/fixtures/go-frozen/http-headers/windows-amd64/headers.json"
        ),
    ),
    (
        "windows/arm64",
        "e8bb6643cbc2a05dbc19f3ad3749513887095ac9",
        "f2ab4598427c2f6b2af43e7ad1b7f20070fd65f3782b78832b7fa68e266c24a0",
        include_bytes!(
            "../../../../tests/fixtures/go-frozen/http-headers/windows-arm64/headers.json"
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
            let current = std::fs::read(Path::new(ROOT).join(path)).unwrap();
            for bytes in [&archived_bytes, &current] {
                assert_eq!(recorded["bytes"], bytes.len(), "{name}");
                assert_eq!(recorded["sha256"], digest(bytes), "{name}");
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

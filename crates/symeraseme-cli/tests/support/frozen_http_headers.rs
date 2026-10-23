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
        "e2cf176337f6f90e64f76b5f39d3a859e87774ea",
        "cb1e16218992c3b5dea66afc52b998277f37f30c958143a244044643824228ee",
        include_bytes!(
            "../../../../tests/fixtures/go-frozen/refresh-e2cf1763/http-headers/darwin-amd64/headers.json"
        ),
    ),
    (
        "darwin/arm64",
        "e2cf176337f6f90e64f76b5f39d3a859e87774ea",
        "3174148062186619fa149fb716dbd24b0bd8ec12b8e76774c505e1d1260fa46c",
        include_bytes!(
            "../../../../tests/fixtures/go-frozen/refresh-e2cf1763/http-headers/darwin-arm64/headers.json"
        ),
    ),
    (
        "linux/amd64",
        "e2cf176337f6f90e64f76b5f39d3a859e87774ea",
        "f6a6283272673e87c412cffbea20cb8bcbeff9fb0132de87440baa59ae21e6fe",
        include_bytes!(
            "../../../../tests/fixtures/go-frozen/refresh-e2cf1763/http-headers/linux-amd64/headers.json"
        ),
    ),
    (
        "linux/arm64",
        "e2cf176337f6f90e64f76b5f39d3a859e87774ea",
        "86f7ab6b6f59ea8b4469e64ab92c6d4875b1b2216e92cc155fbce192f579d335",
        include_bytes!(
            "../../../../tests/fixtures/go-frozen/refresh-e2cf1763/http-headers/linux-arm64/headers.json"
        ),
    ),
    (
        "windows/amd64",
        "e2cf176337f6f90e64f76b5f39d3a859e87774ea",
        "a871e99f64e891f77478ea16b9669da06a2b5b62d88cf93567871c4b9f14bc41",
        include_bytes!(
            "../../../../tests/fixtures/go-frozen/refresh-e2cf1763/http-headers/windows-amd64/headers.json"
        ),
    ),
    (
        "windows/arm64",
        "e2cf176337f6f90e64f76b5f39d3a859e87774ea",
        "cfa0c2a1d55d18891bafcc7e16653502041147e17cf57b9cdb702cee494b9bb4",
        include_bytes!(
            "../../../../tests/fixtures/go-frozen/refresh-e2cf1763/http-headers/windows-arm64/headers.json"
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
            // Retired producers remain bound to the independent Go-era pin.
            let retired_producer = field == "archived_generators"
                && matches!(
                    name.as_str(),
                    "crates/symeraseme-cli/tests/mcp_http_headers_process.rs"
                        | "crates/symeraseme-cli/tests/support/capture_http_headers.rs"
                );
            if retired_producer {
                go_source_pin::assert_retired_generator_matches_archive(
                    ROOT,
                    name,
                    &archive.stdout,
                );
            }
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
            if !retired_producer && go_source_pin::current_tree_bound(name) {
                let current = go_source_pin::current_source(ROOT, name);
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

pub fn observations() -> Corpus {
    assert!(
        !std::env::var_os("SYMERASEME_CAPTURE_HTTP_HEADERS").is_some_and(|value| !value.is_empty()),
        "live Go capture is retired; use the immutable historical capture producer"
    );
    match std::env::var("SYMERASEME_PARITY_LIVE_GO").as_deref() {
        Ok("0") | Err(std::env::VarError::NotPresent) => {}
        _ => panic!("live Go execution is retired; SYMERASEME_PARITY_LIVE_GO must be 0 or unset"),
    }
    let target = match (std::env::consts::OS, std::env::consts::ARCH) {
        ("linux", "x86_64") => "linux/amd64",
        ("linux", "aarch64") => "linux/arm64",
        ("windows", "x86_64") => "windows/amd64",
        ("windows", "aarch64") => "windows/arm64",
        ("macos", "x86_64") => "darwin/amd64",
        ("macos", "aarch64") => "darwin/arm64",
        _ => panic!("no recorded native HTTP header capture for this target; live Go is retired"),
    };
    let (_, revision, pin, raw) = RECORDS
        .iter()
        .find(|record| record.0 == target)
        .expect("no recorded HTTP headers for the native target");
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
    Corpus(record)
}

// Same request encoding as the historical capture producer, including token roles.
pub fn request(method: &str, body: &[u8], headers: &[(&str, String)]) -> Value {
    serde_json::json!({
        "method": method,
        "body_base64": base64::engine::general_purpose::STANDARD.encode(body),
        "body": {"bytes": body.len(), "sha256": digest(body)},
        "headers": headers.iter().map(|(name, value)| {
            if name.eq_ignore_ascii_case("authorization") {
                assert!(value.starts_with("Bearer "));
                serde_json::json!({"name": name, "role": "private-native-bearer-token"})
            } else { serde_json::json!({"name": name, "value": value}) }
        }).collect::<Vec<_>>()
    })
}

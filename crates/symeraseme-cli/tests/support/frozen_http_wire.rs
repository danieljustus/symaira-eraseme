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
        "e61132f85b89580c96d95d098289f7238e810255",
        "220f244608254653ee412644d1cff4e0a7a1951de90d5f9ad8b381f6615bb67e",
        include_bytes!(
            "../../../../tests/fixtures/go-frozen/refresh-e61132f8/http-wire/darwin-amd64/wire.json"
        ),
    ),
    (
        "darwin/arm64",
        "e61132f85b89580c96d95d098289f7238e810255",
        "7de3681bbcc17ba01834bfc57376be7e433424d5f7cb4c5e6b3bf771ee0c9c29",
        include_bytes!(
            "../../../../tests/fixtures/go-frozen/refresh-e61132f8/http-wire/darwin-arm64/wire.json"
        ),
    ),
    (
        "linux/amd64",
        "e61132f85b89580c96d95d098289f7238e810255",
        "79d2b9120a4091fdb081a7f725abcbd51b6da6fc82728a3409d1af0a91d37eb6",
        include_bytes!(
            "../../../../tests/fixtures/go-frozen/refresh-e61132f8/http-wire/linux-amd64/wire.json"
        ),
    ),
    (
        "linux/arm64",
        "e61132f85b89580c96d95d098289f7238e810255",
        "f0bc1edb309826fb57dcb0325971309222dcda668596d230942c5a6c125f10b2",
        include_bytes!(
            "../../../../tests/fixtures/go-frozen/refresh-e61132f8/http-wire/linux-arm64/wire.json"
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
            assert_eq!(recorded["bytes"], archive.stdout.len(), "{name}");
            assert_eq!(recorded["sha256"], digest(&archive.stdout), "{name}");
            let retired_producer = field == "archived_generators"
                && matches!(
                    name.as_str(),
                    "crates/symeraseme-cli/tests/mcp_http_process.rs"
                        | "crates/symeraseme-cli/tests/support/capture_http_wire.rs"
                );
            if retired_producer {
                go_source_pin::assert_retired_generator_matches_archive(
                    ROOT,
                    name,
                    &archive.stdout,
                );
            } else if go_source_pin::current_tree_bound(name) {
                let current = go_source_pin::current_source(ROOT, name);
                go_source_pin::assert_current_matches_archive(name, &archive.stdout, &current);
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

pub fn observations() -> Corpus {
    assert!(
        !std::env::var_os("SYMERASEME_CAPTURE_HTTP_WIRE").is_some_and(|value| !value.is_empty()),
        "live Go capture is retired; use the immutable historical capture producer"
    );
    match std::env::var("SYMERASEME_PARITY_LIVE_GO").as_deref() {
        Ok("0") | Err(std::env::VarError::NotPresent) => {}
        _ => panic!("live Go execution is retired; SYMERASEME_PARITY_LIVE_GO must be 0 or unset"),
    }
    let target = match (std::env::consts::OS, std::env::consts::ARCH) {
        ("linux", "x86_64") => "linux/amd64",
        ("linux", "aarch64") => "linux/arm64",
        ("macos", "x86_64") => "darwin/amd64",
        ("macos", "aarch64") => "darwin/arm64",
        _ => panic!("no recorded native HTTP wire capture for this target; live Go is retired"),
    };
    let (_, revision, pin, raw) = RECORDS
        .iter()
        .find(|record| record.0 == target)
        .expect("no recorded HTTP wire capture for the native target");
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

// Preserve the request representation of the historical capture producer.
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

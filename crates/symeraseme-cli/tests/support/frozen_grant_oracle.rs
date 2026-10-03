use super::frozen_review_oracle::{digest, valid_bytes};
use serde_json::Value;
use std::path::Path;
use std::sync::OnceLock;

const ROOT: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../..");
const DIRECTORY: &str = "tests/fixtures/go-frozen/cli-grant";

fn manifest() -> &'static Value {
    static MANIFEST: OnceLock<Value> = OnceLock::new();
    MANIFEST.get_or_init(|| {
        let manifest: Value = serde_json::from_slice(include_bytes!(
            "../../../../tests/fixtures/go-frozen/cli-grant/manifest.json"
        ))
        .unwrap();
        assert_eq!(
            manifest["source_revision"],
            "aa684787641be9c92a8b0c5a30fa75d804c1c48d"
        );
        assert_eq!(manifest["go_version"], "go1.26.6");
        assert_eq!(manifest["native_target"], "linux/amd64");
        let info = manifest["binary_build_info"].as_str().unwrap();
        assert!(info.contains("vcs.revision=aa684787641be9c92a8b0c5a30fa75d804c1c48d"));
        assert!(info.contains("vcs.modified=false"));
        assert_eq!(manifest["build"]["exit_status"], 0);
        for name in ["stdout", "stderr"] {
            let bytes = std::fs::read(
                Path::new(ROOT)
                    .join(DIRECTORY)
                    .join(format!("build.{name}")),
            )
            .unwrap();
            assert!(valid_bytes(&manifest["build"][name], &bytes));
            assert!(bytes.is_empty());
        }
        let sources = manifest["source_files"].as_object().unwrap();
        assert_eq!(sources.len(), 171);
        for (name, expected) in sources {
            assert_eq!(
                expected,
                &digest(&std::fs::read(Path::new(ROOT).join(name)).unwrap()),
                "Go source drift: {name}"
            );
        }
        assert_eq!(manifest["cases"].as_array().unwrap().len(), 6);
        manifest
    })
}

pub struct ExpectedOutput {
    pub status: i32,
    pub stdout: Vec<u8>,
    pub stderr: Vec<u8>,
}

pub fn observation(index: usize, argv: &[&str]) -> ExpectedOutput {
    let case = &manifest()["cases"][index];
    assert_eq!(
        serde_json::to_value(argv).unwrap(),
        case["argv"],
        "actual captured grant arguments"
    );
    let read = |name: &str| {
        let bytes = std::fs::read(
            Path::new(ROOT)
                .join(DIRECTORY)
                .join(format!("case-{index}.{name}")),
        )
        .unwrap();
        assert!(
            valid_bytes(&case[name], &bytes),
            "grant {index} {name} drift"
        );
        bytes
    };
    ExpectedOutput {
        status: case["exit_status"].as_i64().unwrap().try_into().unwrap(),
        stdout: read("stdout"),
        stderr: read("stderr"),
    }
}

pub fn valid_record(metadata: &Value, bytes: &[u8]) -> bool {
    if !valid_bytes(metadata, bytes) || metadata["mode"] != "0600" {
        return false;
    }
    let Ok(record) = serde_json::from_slice::<Value>(bytes) else {
        return false;
    };
    let Some(token) = record["token"].as_str() else {
        return false;
    };
    metadata["name"] == format!("consent_{}.json", &digest(token.as_bytes())[..16])
}

pub fn records(index: usize) -> Vec<Vec<u8>> {
    let records = manifest()["cases"][index]["consent_records"]
        .as_array()
        .unwrap();
    assert_eq!(records.len(), [1, 1, 2, 1, 0, 0][index]);
    records
        .iter()
        .map(|metadata| {
            let bytes = std::fs::read(
                Path::new(ROOT)
                    .join(DIRECTORY)
                    .join(metadata["file"].as_str().unwrap()),
            )
            .unwrap();
            assert!(
                valid_record(metadata, &bytes),
                "actual grant file/hash/mode drift"
            );
            bytes
        })
        .collect()
}

pub fn snapshot(index: usize) -> Vec<Vec<u8>> {
    let mut snapshot = records(index)
        .into_iter()
        .map(|bytes| {
            let record: Value = serde_json::from_slice(&bytes).unwrap();
            super::normalize_grant_bytes(&bytes, &[record["token"].as_str().unwrap()])
        })
        .collect::<Vec<_>>();
    snapshot.sort();
    snapshot
}

pub fn first_record_metadata() -> &'static Value {
    &manifest()["cases"][0]["consent_records"][0]
}

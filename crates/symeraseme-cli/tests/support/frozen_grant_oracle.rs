//! Actual four-target grant captures. Unrecorded runtime targets use real Go.
use super::frozen_review_oracle::{digest, valid_bytes};
use serde_json::Value;
use std::path::{Component, Path};
use std::sync::OnceLock;

const ROOT: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../..");
const DIRECTORY: &str = "tests/fixtures/go-frozen/cli-grant-native-344";
const REVISION: &str = "344dfaae9c51a9ae4dabe8fc50bd5f04f0b379d0";
const TARGETS: [&str; 4] = [
    "linux-amd64",
    "linux-arm64",
    "windows-amd64",
    "windows-arm64",
];

fn recorded_target(os: &str, arch: &str) -> Option<&'static str> {
    match (os, arch) {
        ("linux", "x86_64") => Some("linux-amd64"),
        ("linux", "aarch64") => Some("linux-arm64"),
        ("windows", "x86_64") => Some("windows-amd64"),
        ("windows", "aarch64") => Some("windows-arm64"),
        _ => None,
    }
}

pub fn live_required() -> bool {
    match std::env::var("SYMERASEME_PARITY_LIVE_GO").as_deref() {
        Ok("1") => true,
        Ok("0") | Err(std::env::VarError::NotPresent) => {
            recorded_target(std::env::consts::OS, std::env::consts::ARCH).is_none()
        }
        _ => panic!("SYMERASEME_PARITY_LIVE_GO must be 0 or 1"),
    }
}

fn control_target() -> &'static str {
    // Corruption controls inspect recorded files even on unrecorded hosts.
    // live_required() keeps their runtime parity comparator on actual Go.
    recorded_target(std::env::consts::OS, std::env::consts::ARCH).unwrap_or("linux-amd64")
}

fn read(target: &str, name: &str) -> Vec<u8> {
    assert!(TARGETS.contains(&target));
    assert!(
        Path::new(name)
            .components()
            .all(|c| matches!(c, Component::Normal(_)))
    );
    std::fs::read(Path::new(ROOT).join(DIRECTORY).join(target).join(name)).unwrap()
}

fn expected_mode(target: &str) -> &'static str {
    if target.starts_with("windows-") {
        "0666"
    } else {
        "0600"
    }
}

fn valid_record_for_target(target: &str, metadata: &Value, bytes: &[u8]) -> bool {
    if !valid_bytes(metadata, bytes) || metadata["mode"] != expected_mode(target) {
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

fn verify(manifest: &Value, target: &str) {
    assert!(TARGETS.contains(&target));
    let (os, arch) = target.split_once('-').unwrap();
    assert_eq!(manifest["source_revision"], REVISION);
    assert_eq!(manifest["go_version"], "go1.26.6");
    assert_eq!(manifest["native_target"], format!("{os}/{arch}"));
    let sources = manifest["source_files"].as_object().unwrap();
    assert_eq!(sources.len(), 194);
    for (name, metadata) in sources {
        let path = Path::new(name);
        assert!(
            !path.is_absolute() && path.components().all(|c| matches!(c, Component::Normal(_)))
        );
        let source = std::fs::read(Path::new(ROOT).join(path)).unwrap();
        assert!(valid_bytes(metadata, &source), "grant source drift: {name}");
    }
    let info = manifest["cli_grants"]["embedded_build_info"]
        .as_str()
        .unwrap();
    for expected in [
        format!("vcs.revision={REVISION}"),
        "vcs.modified=false".into(),
        format!("GOOS={os}"),
        format!("GOARCH={arch}"),
    ] {
        assert!(info.contains(&expected), "native grant binary provenance");
    }
    let cases = manifest["cli_grants"]["cases"].as_array().unwrap();
    assert_eq!(cases.len(), 6);
    for (index, case) in cases.iter().enumerate() {
        assert_eq!(case["exit_status"], 0);
        for name in ["stdout", "stderr"] {
            let bytes = read(target, &format!("cli-grant-case-{index}.{name}"));
            assert!(valid_bytes(&case[name], &bytes), "native grant frame drift");
        }
        let records = case["consent_records"].as_array().unwrap();
        assert_eq!(records.len(), [1, 1, 2, 1, 0, 0][index]);
        for metadata in records {
            let bytes = read(target, metadata["file"].as_str().unwrap());
            assert!(
                valid_record_for_target(target, metadata, &bytes),
                "native consent record drift"
            );
        }
    }
}

fn manifest() -> &'static Value {
    static MANIFEST: OnceLock<Value> = OnceLock::new();
    MANIFEST.get_or_init(|| {
        let manifest = serde_json::from_slice(&read(control_target(), "manifest.json")).unwrap();
        verify(&manifest, control_target());
        manifest
    })
}

pub struct ExpectedOutput {
    pub status: i32,
    pub stdout: Vec<u8>,
    pub stderr: Vec<u8>,
}

pub fn observation(index: usize, argv: &[&str]) -> ExpectedOutput {
    assert!(
        !live_required(),
        "unrecorded target must execute the live Go producer"
    );
    let case = &manifest()["cli_grants"]["cases"][index];
    assert_eq!(
        serde_json::to_value(argv).unwrap(),
        case["argv"],
        "actual captured grant arguments"
    );
    ExpectedOutput {
        status: case["exit_status"].as_i64().unwrap().try_into().unwrap(),
        stdout: read(control_target(), &format!("cli-grant-case-{index}.stdout")),
        stderr: read(control_target(), &format!("cli-grant-case-{index}.stderr")),
    }
}

pub fn valid_record(metadata: &Value, bytes: &[u8]) -> bool {
    valid_record_for_target(control_target(), metadata, bytes)
}

pub fn records(index: usize) -> Vec<Vec<u8>> {
    manifest()["cli_grants"]["cases"][index]["consent_records"]
        .as_array()
        .unwrap()
        .iter()
        .map(|metadata| {
            let bytes = read(control_target(), metadata["file"].as_str().unwrap());
            assert!(valid_record(metadata, &bytes));
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
    &manifest()["cli_grants"]["cases"][0]["consent_records"][0]
}

pub fn verify_all_native_records_and_reject_changed_provenance() {
    assert!(recorded_target("macos", "aarch64").is_none());
    assert!(recorded_target("macos", "x86_64").is_none());
    for target in TARGETS {
        let manifest: Value = serde_json::from_slice(&read(target, "manifest.json")).unwrap();
        verify(&manifest, target);
        for (field, value) in [
            ("source_revision", "fabricated-revision"),
            ("native_target", "fabricated/target"),
        ] {
            let mut changed = manifest.clone();
            changed[field] = value.into();
            assert!(std::panic::catch_unwind(|| verify(&changed, target)).is_err());
        }
        let metadata = &manifest["cli_grants"]["cases"][0]["consent_records"][0];
        let bytes = read(target, metadata["file"].as_str().unwrap());
        let mut changed = bytes.clone();
        changed[0] ^= 1;
        assert!(!valid_record_for_target(target, metadata, &changed));
        let mut wrong_mode = metadata.clone();
        wrong_mode["mode"] = "0644".into();
        assert!(!valid_record_for_target(target, &wrong_mode, &bytes));
        let mut wrong_name = metadata.clone();
        wrong_name["name"] = "consent_wrong.json".into();
        assert!(!valid_record_for_target(target, &wrong_name, &bytes));
    }
}

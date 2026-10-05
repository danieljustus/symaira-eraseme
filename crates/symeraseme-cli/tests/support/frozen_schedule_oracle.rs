//! Whole CLI schedule frames from six actual native Go producers.
use super::frozen_review_oracle::{digest, go_source_pin, valid_bytes};
use base64::Engine;
use serde_json::Value;
use std::path::{Component, Path};

const ROOT: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../..");
const REVISION: &str = "e8bb6643cbc2a05dbc19f3ad3749513887095ac9";
const TARGETS: [&str; 6] = [
    "linux-amd64",
    "linux-arm64",
    "windows-amd64",
    "windows-arm64",
    "darwin-amd64",
    "darwin-arm64",
];

fn target(os: &str, arch: &str) -> Option<&'static str> {
    match (os, arch) {
        ("linux", "x86_64") => Some("linux-amd64"),
        ("linux", "aarch64") => Some("linux-arm64"),
        ("windows", "x86_64") => Some("windows-amd64"),
        ("windows", "aarch64") => Some("windows-arm64"),
        ("macos", "x86_64") => Some("darwin-amd64"),
        ("macos", "aarch64") => Some("darwin-arm64"),
        _ => None,
    }
}

pub fn live_requested() -> bool {
    match std::env::var("SYMERASEME_PARITY_LIVE_GO").as_deref() {
        Ok("1") => true,
        Ok("0") | Err(std::env::VarError::NotPresent) => false,
        _ => panic!("SYMERASEME_PARITY_LIVE_GO must be 0 or 1"),
    }
}

fn read(directory: &str, target: &str, name: &str) -> Vec<u8> {
    assert!(TARGETS.contains(&target));
    std::fs::read(
        Path::new(ROOT)
            .join("tests/fixtures/go-frozen")
            .join(directory)
            .join(target)
            .join(name),
    )
    .unwrap()
}

fn verify(target: &str, manifest: &Value, bytes: &[u8]) -> Value {
    assert!(TARGETS.contains(&target));
    let (os, arch) = target.split_once('-').unwrap();
    assert_eq!(manifest["source_revision"], REVISION);
    assert_eq!(manifest["go_version"], "go1.26.6");
    assert_eq!(manifest["native_target"], format!("{os}/{arch}"));
    let sources = manifest["source_files"].as_object().unwrap();
    assert_eq!(sources.len(), 1481);
    for (name, metadata) in sources {
        let path = Path::new(name);
        assert!(
            !path.is_absolute() && path.components().all(|c| matches!(c, Component::Normal(_)))
        );
        if !go_source_pin::current_tree_bound(name) {
            continue;
        }
        assert!(
            valid_bytes(
                metadata,
                &std::fs::read(Path::new(ROOT).join(path)).unwrap()
            ),
            "schedule source drift: {name}"
        );
    }
    let recorded = manifest["runtime_oracles"]
        .as_array()
        .unwrap()
        .iter()
        .find(|record| record["package"] == "cli-schedule")
        .unwrap();
    assert_eq!(recorded["exit_status"], 0);
    assert_eq!(recorded["stdin"]["bytes"], 0);
    assert_eq!(recorded["stdin"]["sha256"], digest(&[]));
    assert!(valid_bytes(&recorded["stdout"], bytes));
    let stderr = read("native-0d/e8", target, "cli-schedule.stderr");
    assert!(valid_bytes(&recorded["stderr"], &stderr));
    let info = recorded["embedded_build_info"].as_str().unwrap();
    for expected in [
        format!("vcs.revision={REVISION}"),
        "vcs.modified=false".into(),
        format!("GOOS={os}"),
        format!("GOARCH={arch}"),
    ] {
        assert!(info.contains(&expected), "native schedule build provenance");
    }
    let fixture: Value = serde_json::from_slice(bytes).unwrap();
    assert_eq!(fixture["schema"], "symeraseme.go-oracle.cli.v1");
    let cases = fixture["cases"].as_array().unwrap();
    assert_eq!(cases.len(), 13);
    let ids = cases
        .iter()
        .map(|case| case["id"].clone())
        .collect::<Vec<_>>();
    assert_eq!(serde_json::to_value(ids).unwrap(), recorded["cases"]);
    for case in cases {
        for name in ["stdout", "stderr"] {
            let bytes = base64::engine::general_purpose::STANDARD
                .decode(case[format!("{name}_base64")].as_str().unwrap())
                .unwrap();
            assert_eq!(case[format!("{name}_bytes")], bytes.len());
        }
        assert!(case["files"].is_object());
        assert!([0, 1].contains(&case["exit_code"].as_i64().unwrap()));
    }
    fixture
}

fn manifest(target: &str) -> Value {
    serde_json::from_slice(&read("native-0d/e8", target, "manifest.json")).unwrap()
}

pub fn fixture() -> Option<Value> {
    if live_requested() {
        return None;
    }
    let target = target(std::env::consts::OS, std::env::consts::ARCH)?;
    let bytes = read("native-0d/e8", target, "cli-schedule.stdout");
    Some(verify(target, &manifest(target), &bytes))
}

pub fn verify_all_native_frames_and_reject_corruption() {
    for target in TARGETS {
        let manifest = manifest(target);
        let bytes = read("native-0d/e8", target, "cli-schedule.stdout");
        verify(target, &manifest, &bytes);
        let mut changed = bytes.clone();
        changed[0] ^= 1;
        assert!(std::panic::catch_unwind(|| verify(target, &manifest, &changed)).is_err());
        let mut incomplete: Value = serde_json::from_slice(&bytes).unwrap();
        incomplete["cases"].as_array_mut().unwrap().pop().unwrap();
        assert!(
            std::panic::catch_unwind(|| verify(
                target,
                &manifest,
                &serde_json::to_vec(&incomplete).unwrap()
            ))
            .is_err()
        );
        let mut wrong_target = manifest.clone();
        wrong_target["native_target"] = "fabricated/target".into();
        assert!(std::panic::catch_unwind(|| verify(target, &wrong_target, &bytes)).is_err());
        let mut wrong_source = manifest.clone();
        wrong_source["source_revision"] = "fabricated-revision".into();
        assert!(std::panic::catch_unwind(|| verify(target, &wrong_source, &bytes)).is_err());
    }
}

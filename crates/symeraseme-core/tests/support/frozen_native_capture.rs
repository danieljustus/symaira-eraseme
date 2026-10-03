//! Reviewed actual Go captures; a missing target must use the live producer.

use serde_json::Value;
use sha2::{Digest, Sha256};
use std::path::{Component, Path};

const REVISION: &str = "0d1be28288594354941a07d19f834f2553928bc4";
const ROOT: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../..");

pub fn manifest_bytes(os: &str, arch: &str) -> Option<&'static [u8]> {
    macro_rules! recorded {
        ($target:literal) => {
            include_bytes!(concat!(
                "../../../../tests/fixtures/go-frozen/native-0d/",
                $target,
                "/manifest.json"
            ))
            .as_slice()
        };
    }
    match (os, arch) {
        ("linux", "x86_64") => Some(recorded!("linux-amd64")),
        ("linux", "aarch64") => Some(recorded!("linux-arm64")),
        ("windows", "x86_64") => Some(recorded!("windows-amd64")),
        ("windows", "aarch64") => Some(recorded!("windows-arm64")),
        ("macos", "x86_64") => Some(recorded!("darwin-amd64")),
        _ => None,
    }
}

pub fn verify(bytes: &[u8], os: &str, arch: &str) -> Value {
    let manifest: Value = serde_json::from_slice(bytes).expect("actual native capture manifest");
    let go_os = if os == "macos" { "darwin" } else { os };
    let go_arch = match arch {
        "x86_64" => "amd64",
        "aarch64" => "arm64",
        _ => panic!("unrecorded native architecture"),
    };
    let target = format!("{go_os}/{go_arch}");
    assert_eq!(manifest["source_revision"], REVISION);
    assert_eq!(manifest["go_version"], "go1.26.6");
    assert_eq!(manifest["native_target"], target);
    let sources = manifest["source_files"]
        .as_object()
        .expect("source inventory");
    assert_eq!(sources.len(), 194);
    for (path, metadata) in sources {
        let relative = Path::new(path);
        assert!(!relative.is_absolute());
        assert!(
            relative
                .components()
                .all(|c| matches!(c, Component::Normal(_)))
        );
        let source = std::fs::read(Path::new(ROOT).join(relative)).expect("captured source file");
        assert_eq!(metadata["bytes"], source.len(), "{path}");
        assert_eq!(
            metadata["sha256"],
            hex::encode(Sha256::digest(&source)),
            "{path}"
        );
    }
    for family in ["llm_failures", "cli_review"] {
        let info = manifest[family]["embedded_build_info"].as_str().unwrap();
        for expected in [
            format!("vcs.revision={REVISION}"),
            "vcs.modified=false".to_owned(),
            format!("GOOS={go_os}"),
            format!("GOARCH={go_arch}"),
        ] {
            assert!(info.contains(&expected), "native {family} build provenance");
        }
    }
    assert_eq!(
        manifest["llm_failures"]["cases"].as_array().unwrap().len(),
        7
    );
    assert_eq!(manifest["cli_review"]["cases"].as_array().unwrap().len(), 8);
    manifest
}

pub fn verify_llm(manifest: &Value, fixture: &[u8], args: &[&str]) {
    let case = manifest["llm_failures"]["cases"]
        .as_array()
        .unwrap()
        .iter()
        .find(|case| case["args"] == serde_json::to_value(args).unwrap())
        .expect("unobserved native LLM arguments cannot receive a cached answer");
    assert_eq!(case["exit_status"], 0);
    assert_eq!(case["stdout"]["bytes"], fixture.len());
    assert_eq!(
        case["stdout"]["sha256"],
        hex::encode(Sha256::digest(fixture))
    );
    assert_eq!(case["stderr"]["bytes"], 0);
    assert_eq!(case["stderr"]["sha256"], hex::encode(Sha256::digest([])));
}

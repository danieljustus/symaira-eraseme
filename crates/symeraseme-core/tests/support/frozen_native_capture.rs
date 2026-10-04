//! Reviewed actual Go captures; a missing target must use the live producer.

use serde_json::Value;
use sha2::{Digest, Sha256};
use std::path::{Component, Path};

const REVISION: &str = "e8bb6643cbc2a05dbc19f3ad3749513887095ac9";
const ROOT: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../..");

pub fn manifest_bytes(os: &str, arch: &str) -> Option<&'static [u8]> {
    macro_rules! recorded {
        ($target:literal) => {
            include_bytes!(concat!(
                "../../../../tests/fixtures/go-frozen/native-0d/e8/",
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
        ("macos", "aarch64") => Some(recorded!("darwin-arm64")),
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
    assert_eq!(sources.len(), 1481);
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
        assert_eq!(metadata["sha256"], digest(&source), "{path}");
    }
    let check_build = |info: &str| {
        for expected in [
            format!("vcs.revision={REVISION}"),
            "vcs.modified=false".to_owned(),
            format!("GOOS={go_os}"),
            format!("GOARCH={go_arch}"),
        ] {
            assert!(info.contains(&expected), "native build provenance");
        }
    };
    for family in ["llm_failures", "cli_review"] {
        check_build(manifest[family]["embedded_build_info"].as_str().unwrap());
    }
    let observations = manifest["observations"].as_array().unwrap();
    assert_eq!(observations.len(), 6);
    for observed in observations {
        assert_eq!(observed["exit_status"], 0);
        assert_eq!(observed["stderr"]["bytes"], 0);
        assert_eq!(observed["stderr"]["sha256"], digest(&[]));
        check_build(observed["embedded_build_info"].as_str().unwrap());
    }
    assert_eq!(
        manifest["llm_failures"]["cases"].as_array().unwrap().len(),
        7
    );
    assert_eq!(manifest["cli_review"]["cases"].as_array().unwrap().len(), 8);
    manifest
}

fn digest(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

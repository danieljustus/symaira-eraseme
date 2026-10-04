//! Immutable actual Linux-amd64 Go runtime observations; other hosts remain live.
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::path::{Component, Path};

const ROOT: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../..");
const REVISION: &str = "f0a91ab986387f3e7b96a55388452bd56346f4e0";
const MANIFEST: &[u8] =
    include_bytes!("../../../../tests/fixtures/go-frozen/mcp-runtime/manifest.json");

pub fn live_required() -> bool {
    match std::env::var("SYMERASEME_PARITY_LIVE_GO").as_deref() {
        Ok("1") => true,
        Ok("0") | Err(std::env::VarError::NotPresent) => {
            !cfg!(all(target_os = "linux", target_arch = "x86_64"))
        }
        _ => panic!("SYMERASEME_PARITY_LIVE_GO must be 0 or 1"),
    }
}

fn digest(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

pub fn observation(family: &str) -> Vec<u8> {
    macro_rules! captured {
        ($name:literal) => {
            include_bytes!(concat!(
                "../../../../tests/fixtures/go-frozen/mcp-runtime/",
                $name,
                ".stdout"
            ))
            .as_slice()
        };
    }
    let bytes = match family {
        "mcp-clock" => captured!("mcp-clock"),
        "mcp-auto-confirm" => captured!("mcp-auto-confirm"),
        "mcp-tool-gaps" => captured!("mcp-tool-gaps"),
        _ => panic!("unobserved MCP family cannot receive a cached answer"),
    };
    verify(MANIFEST, family, bytes);
    bytes.to_vec()
}

fn verify(manifest_bytes: &[u8], family: &str, bytes: &[u8]) {
    let manifest: Value = serde_json::from_slice(manifest_bytes).unwrap();
    assert_eq!(manifest["source_revision"], REVISION);
    assert_eq!(manifest["go_version"], "go1.26.6");
    assert_eq!(manifest["native_target"], "linux/amd64");
    let sources = manifest["source_files"].as_object().unwrap();
    assert_eq!(sources.len(), 1476);
    for (name, record) in sources {
        let path = Path::new(name);
        assert!(
            !path.is_absolute() && path.components().all(|c| matches!(c, Component::Normal(_)))
        );
        let actual = std::fs::read(Path::new(ROOT).join(path)).unwrap();
        assert_eq!(record["bytes"], actual.len(), "{name}");
        assert_eq!(record["sha256"], digest(&actual), "{name}");
    }
    let record = manifest["runtime_oracles"]
        .as_array()
        .unwrap()
        .iter()
        .find(|record| record["package"] == family)
        .expect("actual captured family");
    assert_eq!(record["exit_status"], 0);
    assert_eq!(record["stdin"]["bytes"], 0);
    assert_eq!(record["stdin"]["sha256"], digest(&[]));
    assert_eq!(record["stdout"]["bytes"], bytes.len());
    assert_eq!(record["stdout"]["sha256"], digest(bytes));
    assert_eq!(record["stderr"]["bytes"], 0);
    assert_eq!(record["stderr"]["sha256"], digest(&[]));
    let info = record["embedded_build_info"].as_str().unwrap();
    for expected in [
        format!("vcs.revision={REVISION}"),
        "vcs.modified=false".to_owned(),
        "GOOS=linux".to_owned(),
        "GOARCH=amd64".to_owned(),
    ] {
        assert!(info.contains(&expected), "native MCP build provenance");
    }
    let expected_count = match family {
        "mcp-clock" => 6,
        "mcp-auto-confirm" => 4,
        "mcp-tool-gaps" => 25,
        _ => panic!("unknown family"),
    };
    assert_eq!(record["cases"].as_array().unwrap().len(), expected_count);
    if family == "mcp-clock" {
        assert_eq!(record["hostile_project_db_absent"], true);
        assert_eq!(record["hostile_inherited_data_absent"], true);
    }
}

#[test]
fn changed_frames_unknown_families_and_source_identity_are_rejected() {
    for family in ["mcp-clock", "mcp-auto-confirm", "mcp-tool-gaps"] {
        let bytes = observation(family);
        let mut changed = bytes.clone();
        changed.push(b'!');
        assert!(std::panic::catch_unwind(|| verify(MANIFEST, family, &changed)).is_err());
        assert!(
            std::panic::catch_unwind(|| verify(MANIFEST, family, &bytes[..bytes.len() - 1]))
                .is_err()
        );
    }
    assert!(std::panic::catch_unwind(|| observation("unobserved-family")).is_err());
    let mut changed: Value = serde_json::from_slice(MANIFEST).unwrap();
    changed["source_revision"] = "fabricated-revision".into();
    assert!(
        std::panic::catch_unwind(|| verify(
            &serde_json::to_vec(&changed).unwrap(),
            "mcp-clock",
            &observation("mcp-clock")
        ))
        .is_err()
    );
}

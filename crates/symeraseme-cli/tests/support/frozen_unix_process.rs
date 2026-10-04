//! Complete actual Linux-amd64 Go process observations. Other hosts remain live.
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::path::{Component, Path};

const ROOT: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../..");
const REVISION: &str = "8febd4dfa3498065b464ccde79aaaceafaba44c6";
const MANIFEST: &[u8] =
    include_bytes!("../../../../tests/fixtures/go-frozen/unix-process-8feb/manifest.json");

fn digest(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

fn streams(family: &str) -> (&'static [u8], &'static [u8]) {
    macro_rules! recorded {
        ($name:literal) => {
            (
                include_bytes!(concat!(
                    "../../../../tests/fixtures/go-frozen/unix-process-8feb/",
                    $name,
                    ".stdout"
                ))
                .as_slice(),
                include_bytes!(concat!(
                    "../../../../tests/fixtures/go-frozen/unix-process-8feb/",
                    $name,
                    ".stderr"
                ))
                .as_slice(),
            )
        };
    }
    match family {
        "cli-triage" => recorded!("cli-triage"),
        "mcp-triage" => recorded!("mcp-triage"),
        "mcp-agent-error-json" => recorded!("mcp-agent-error-json"),
        _ => panic!("unrecorded process cannot receive a cached answer"),
    }
}

fn verify(manifest: &[u8], family: &str, stdout: &[u8], stderr: &[u8]) {
    let manifest: Value = serde_json::from_slice(manifest).unwrap();
    assert_eq!(manifest["source_revision"], REVISION);
    assert_eq!(manifest["go_version"], "go1.26.6");
    assert_eq!(manifest["native_target"], "linux/amd64");
    let sources = manifest["source_files"].as_object().unwrap();
    assert_eq!(sources.len(), 1481);
    for (name, record) in sources {
        let path = Path::new(name);
        assert!(
            !path.is_absolute() && path.components().all(|c| matches!(c, Component::Normal(_)))
        );
        let bytes = std::fs::read(Path::new(ROOT).join(path)).unwrap();
        assert_eq!(record["bytes"], bytes.len(), "{name}");
        assert_eq!(record["sha256"], digest(&bytes), "{name}");
    }
    let record = manifest["runtime_oracles"]
        .as_array()
        .unwrap()
        .iter()
        .find(|record| record["package"] == family)
        .expect("actual recorded process family");
    assert_eq!(record["exit_status"], 0);
    assert_eq!(record["stdin"]["bytes"], 0);
    assert_eq!(record["stdin"]["sha256"], digest(&[]));
    for (name, bytes) in [("stdout", stdout), ("stderr", stderr)] {
        assert_eq!(record[name]["bytes"], bytes.len());
        assert_eq!(record[name]["sha256"], digest(bytes));
    }
    let info = record["embedded_build_info"].as_str().unwrap();
    for expected in [
        format!("vcs.revision={REVISION}"),
        "vcs.modified=false".into(),
        "GOOS=linux".into(),
        "GOARCH=amd64".into(),
    ] {
        assert!(info.contains(&expected), "native process binary provenance");
    }
    let count = match family {
        "cli-triage" => 16,
        "mcp-triage" => 4,
        "mcp-agent-error-json" => 1,
        _ => panic!("unknown family"),
    };
    assert_eq!(record["cases"].as_array().unwrap().len(), count);
}

pub fn observation(family: &str) -> Option<Vec<u8>> {
    match std::env::var("SYMERASEME_PARITY_LIVE_GO").as_deref() {
        Ok("1") => return None,
        Ok("0") | Err(std::env::VarError::NotPresent) => {
            if !cfg!(all(target_os = "linux", target_arch = "x86_64")) {
                return None;
            }
        }
        _ => panic!("SYMERASEME_PARITY_LIVE_GO must be 0 or 1"),
    }
    let (stdout, stderr) = streams(family);
    verify(MANIFEST, family, stdout, stderr);
    // The actual recorded comparator must reject changed whole frames before
    // they can serve as a reference for the live Rust process below.
    let mut changed = stdout.to_vec();
    changed.push(b'!');
    assert!(std::panic::catch_unwind(|| verify(MANIFEST, family, &changed, stderr)).is_err());
    assert!(
        std::panic::catch_unwind(|| verify(MANIFEST, family, &stdout[..stdout.len() - 1], stderr))
            .is_err()
    );
    let mut wrong_source: Value = serde_json::from_slice(MANIFEST).unwrap();
    wrong_source["source_revision"] = "fabricated-revision".into();
    assert!(
        std::panic::catch_unwind(|| verify(
            &serde_json::to_vec(&wrong_source).unwrap(),
            family,
            stdout,
            stderr
        ))
        .is_err()
    );
    Some(stdout.to_vec())
}

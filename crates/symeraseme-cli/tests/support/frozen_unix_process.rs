//! Complete measured Go process observations on all four native Unix hosts.
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::path::{Component, Path};

const ROOT: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../..");
const REVISION: &str = "0972f73f2be6197197814bf4dbfba0c039a3c6dd";
fn native_manifest() -> Option<(&'static str, &'static [u8], &'static str)> {
    match (std::env::consts::OS, std::env::consts::ARCH) {
        ("linux", "x86_64") => Some(("linux/amd64", include_bytes!("../../../../tests/fixtures/go-frozen/unix-process-native097/linux-amd64/manifest.json").as_slice(), "653176a94c6305333da872e9515ab4907495d5bbb4fddb05569b2ca2ca9411fa")),
        ("linux", "aarch64") => Some(("linux/arm64", include_bytes!("../../../../tests/fixtures/go-frozen/unix-process-native097/linux-arm64/manifest.json").as_slice(), "f4828f7028adccaaf7937b68302a848024f1a53ffe7b11eaf9618eb79b0efcba")),
        ("macos", "x86_64") => Some(("darwin/amd64", include_bytes!("../../../../tests/fixtures/go-frozen/unix-process-native097/darwin-amd64/manifest.json").as_slice(), "bdafb6e3992e6f4bff83396dbe92b1e1d28e233ba38bd5a5749aaf4c405d08d6")),
        ("macos", "aarch64") => Some(("darwin/arm64", include_bytes!("../../../../tests/fixtures/go-frozen/unix-process-native097/darwin-arm64/manifest.json").as_slice(), "ab8ff5e20026cbe60008d0706730cc0ca637eb7d997b9bc4d31ae0023902dc4f")),
        _ => None,
    }
}

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

fn verify(manifest: &[u8], pin: &str, target: &str, family: &str, stdout: &[u8], stderr: &[u8]) {
    assert_eq!(digest(manifest), pin, "whole actual native manifest");
    let manifest: Value = serde_json::from_slice(manifest).unwrap();
    assert_eq!(manifest["source_revision"], REVISION);
    assert_eq!(manifest["go_version"], "go1.26.6");
    assert_eq!(manifest["native_target"], target);
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
    let (os, arch) = target.split_once('/').unwrap();
    for expected in [
        format!("vcs.revision={REVISION}"),
        "vcs.modified=false".into(),
        format!("GOOS={os}"),
        format!("GOARCH={arch}"),
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
        Ok("0") | Err(std::env::VarError::NotPresent) => {}
        _ => panic!("SYMERASEME_PARITY_LIVE_GO must be 0 or 1"),
    }
    let (target, manifest, pin) = native_manifest()?;
    let (stdout, stderr) = streams(family);
    verify(manifest, pin, target, family, stdout, stderr);
    // The actual recorded comparator must reject changed whole frames before
    // they can serve as a reference for the live Rust process below.
    let mut changed = stdout.to_vec();
    changed.push(b'!');
    assert!(
        std::panic::catch_unwind(|| verify(manifest, pin, target, family, &changed, stderr))
            .is_err()
    );
    assert!(
        std::panic::catch_unwind(|| verify(
            manifest,
            pin,
            target,
            family,
            &stdout[..stdout.len() - 1],
            stderr
        ))
        .is_err()
    );
    let mut wrong_source: Value = serde_json::from_slice(manifest).unwrap();
    wrong_source["source_revision"] = "fabricated-revision".into();
    assert!(
        std::panic::catch_unwind(|| verify(
            &serde_json::to_vec(&wrong_source).unwrap(),
            pin,
            target,
            family,
            stdout,
            stderr
        ))
        .is_err()
    );
    Some(stdout.to_vec())
}

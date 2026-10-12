//! Whole original native Go helper observations; no Go process or server shim.
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
        "13ddc6a43036d6b34706db2b82bbb5a13c40d6de",
        "176d45db1e6eaeaa72ba0e7fb938bd2305250ce81fb206457fdabfbc6a4d431b",
        include_bytes!(
            "../../../../tests/fixtures/go-frozen/refresh-13ddc6a4/agent-error/darwin-amd64/agent-error.json"
        ),
    ),
    (
        "darwin/arm64",
        "13ddc6a43036d6b34706db2b82bbb5a13c40d6de",
        "f048b9c496f11d173ade0841f0cc870a3ddb358a521735f26e38a3f579aaff31",
        include_bytes!(
            "../../../../tests/fixtures/go-frozen/refresh-13ddc6a4/agent-error/darwin-arm64/agent-error.json"
        ),
    ),
    (
        "linux/amd64",
        "13ddc6a43036d6b34706db2b82bbb5a13c40d6de",
        "d09d93418076d1f68789bb597134032d9ca24493bed571a4f25c45641bf1fae9",
        include_bytes!(
            "../../../../tests/fixtures/go-frozen/refresh-13ddc6a4/agent-error/linux-amd64/agent-error.json"
        ),
    ),
    (
        "linux/arm64",
        "13ddc6a43036d6b34706db2b82bbb5a13c40d6de",
        "0445c5c5506357c13c5b6144b80272dad17c849ced76334fea7e86973de4acb6",
        include_bytes!(
            "../../../../tests/fixtures/go-frozen/refresh-13ddc6a4/agent-error/linux-arm64/agent-error.json"
        ),
    ),
    (
        "windows/amd64",
        "13ddc6a43036d6b34706db2b82bbb5a13c40d6de",
        "5bedf53a0205913bde2de8b258ea8c3f345acb6171fa72a047d2676ca7ca6ce1",
        include_bytes!(
            "../../../../tests/fixtures/go-frozen/refresh-13ddc6a4/agent-error/windows-amd64/agent-error.json"
        ),
    ),
    (
        "windows/arm64",
        "13ddc6a43036d6b34706db2b82bbb5a13c40d6de",
        "1be72dda4a1ef7f456326b8ec95f6734d6f1e65e9356709f98ebf75549f65bc3",
        include_bytes!(
            "../../../../tests/fixtures/go-frozen/refresh-13ddc6a4/agent-error/windows-arm64/agent-error.json"
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
    assert_eq!(digest(raw), pin, "whole measured native agent record");
    let record: Value = serde_json::from_slice(raw).unwrap();
    assert_eq!(
        record["schema"],
        "symeraseme.actual-go.native-agent-error.v1"
    );
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
    for (field, count) in [("source_files", 172), ("archived_generators", 5)] {
        let files = record[field].as_object().unwrap();
        assert_eq!(files.len(), count);
        for (name, recorded) in files {
            let path = Path::new(name);
            assert!(
                !path.is_absolute() && path.components().all(|c| matches!(c, Component::Normal(_)))
            );
            let autocrlf = record["checkout_autocrlf"].as_str().unwrap();
            assert!(["true", "false", "input"].contains(&autocrlf));
            let archive = Command::new("git")
                .args([
                    "-c",
                    &format!("core.autocrlf={autocrlf}"),
                    "cat-file",
                    "--filters",
                    &format!("{revision}:{name}"),
                ])
                .current_dir(ROOT)
                .output()
                .unwrap();
            assert!(
                archive.status.success(),
                "immutable native agent producer must remain verifiable"
            );
            assert_eq!(recorded["bytes"], archive.stdout.len(), "{name}");
            assert_eq!(recorded["sha256"], digest(&archive.stdout), "{name}");
            if go_source_pin::current_tree_bound(name) {
                let current = go_source_pin::current_source(ROOT, name);
                go_source_pin::assert_current_matches_archive(name, &archive.stdout, &current);
            }
        }
    }
    let observed = &record["observations"];
    assert_eq!(observed["Rust_fixture_matches_actual_Go_sanity"], true);
    for (name, expected_status) in [("oracle", 0), ("helper_sanity", 23)] {
        assert_eq!(observed[name]["exit_status"], expected_status);
        for stream in ["stdout", "stderr"] {
            decode(&observed[name][stream]);
        }
    }
    assert_eq!(observed["oracle"]["argv"], serde_json::json!(["--oracle"]));
    assert_eq!(observed["helper_sanity"]["argv"], serde_json::json!([]));
    assert!(decode(&observed["oracle"]["stderr"]).is_empty());
    assert!(decode(&observed["helper_sanity"]["stdout"]).is_empty());
    assert_eq!(
        decode(&observed["helper_sanity"]["stderr"]),
        b"before\xf0\x80\x80after\xffend"
    );
    let limits = observed["helper_limit_controls"].as_array().unwrap();
    assert_eq!(limits.len(), 2);
    for (control, stream) in limits.iter().zip(["stdout", "stderr"]) {
        assert_eq!(control["stream"], stream);
        assert_eq!(control["rejected"], true);
        assert_eq!(control["capture_limit_bytes"], 1024 * 1024);
        assert!(control["elapsed_ms"].as_u64().unwrap() < 10000);
        assert_eq!(
            control["actual_reason"],
            format!("go-flood-{stream}: capture limit or read failure")
        );
    }
    record
}

fn decode(stream: &Value) -> Vec<u8> {
    let raw = base64::engine::general_purpose::STANDARD
        .decode(stream["base64"].as_str().unwrap())
        .unwrap();
    assert!(raw.len() <= 1024 * 1024);
    assert_eq!(stream["bytes"], raw.len());
    raw
}

pub struct Corpus(Value);

impl Corpus {
    pub fn process(&self, name: &str) -> (i32, Vec<u8>, Vec<u8>) {
        assert!(["oracle", "helper_sanity"].contains(&name));
        let observed = &self.0["observations"][name];
        (
            i32::try_from(observed["exit_status"].as_i64().unwrap()).unwrap(),
            decode(&observed["stdout"]),
            decode(&observed["stderr"]),
        )
    }
}

pub fn observations() -> Option<Corpus> {
    if std::env::var_os("SYMERASEME_CAPTURE_AGENT_ERROR").is_some_and(|value| !value.is_empty()) {
        return None;
    }
    match std::env::var("SYMERASEME_PARITY_LIVE_GO").as_deref() {
        Ok("1") => return None,
        Ok("0") | Err(std::env::VarError::NotPresent) => {}
        _ => panic!("SYMERASEME_PARITY_LIVE_GO must be 0 or 1"),
    }
    let target = match (std::env::consts::OS, std::env::consts::ARCH) {
        ("linux", "x86_64") => "linux/amd64",
        ("linux", "aarch64") => "linux/arm64",
        ("macos", "x86_64") => "darwin/amd64",
        ("macos", "aarch64") => "darwin/arm64",
        ("windows", "x86_64") => "windows/amd64",
        ("windows", "aarch64") => "windows/arm64",
        _ => return None,
    };
    let (_, revision, pin, raw) = RECORDS.iter().find(|record| record.0 == target)?;
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
    Some(Corpus(record))
}

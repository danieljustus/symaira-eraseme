//! Actual whole native Go triage processes, saved effects and invocation controls.
use base64::Engine;
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::path::{Component, Path};
use std::process::Command;

#[path = "go_source_pin.rs"]
mod go_source_pin;

const ROOT: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../..");
// Populate only after retaining and independently verifying actual native captures.
const RECORDS: &[(&str, &str, &str, &str, &[u8])] = &[
    (
        "darwin/amd64",
        "cli",
        "13ddc6a43036d6b34706db2b82bbb5a13c40d6de",
        "38af7d8d6897e28b7a47e2072e0c5749aa9bf121e72c3256c02a3fb04e3429a6",
        include_bytes!(
            "../../../../tests/fixtures/go-frozen/refresh-13ddc6a4/native-triage/darwin-amd64/cli.json"
        ),
    ),
    (
        "darwin/amd64",
        "mcp",
        "13ddc6a43036d6b34706db2b82bbb5a13c40d6de",
        "0c8ac95e083e4209f11e5d0c832a282658e868a04974a03769b29f564ae051f1",
        include_bytes!(
            "../../../../tests/fixtures/go-frozen/refresh-13ddc6a4/native-triage/darwin-amd64/mcp.json"
        ),
    ),
    (
        "darwin/arm64",
        "cli",
        "13ddc6a43036d6b34706db2b82bbb5a13c40d6de",
        "82bff15085cd0f640c0930bfa5f51b43e2e5d9de8a67e6750ff94f979cf04784",
        include_bytes!(
            "../../../../tests/fixtures/go-frozen/refresh-13ddc6a4/native-triage/darwin-arm64/cli.json"
        ),
    ),
    (
        "darwin/arm64",
        "mcp",
        "13ddc6a43036d6b34706db2b82bbb5a13c40d6de",
        "5ce908fcaff70aa638f61eae08acb7c6ab9a97dec5ec7c4ae1b961f9c1e4f2ed",
        include_bytes!(
            "../../../../tests/fixtures/go-frozen/refresh-13ddc6a4/native-triage/darwin-arm64/mcp.json"
        ),
    ),
    (
        "linux/amd64",
        "cli",
        "13ddc6a43036d6b34706db2b82bbb5a13c40d6de",
        "05716789baf920819f2725fb899fef8131eadf675e33e7ad12a55bd992e0524f",
        include_bytes!(
            "../../../../tests/fixtures/go-frozen/refresh-13ddc6a4/native-triage/linux-amd64/cli.json"
        ),
    ),
    (
        "linux/amd64",
        "mcp",
        "13ddc6a43036d6b34706db2b82bbb5a13c40d6de",
        "0f1a90b1f7cd0fc80e540caf957aa7cacd8041452ac64fed992237ea0983e855",
        include_bytes!(
            "../../../../tests/fixtures/go-frozen/refresh-13ddc6a4/native-triage/linux-amd64/mcp.json"
        ),
    ),
    (
        "linux/arm64",
        "cli",
        "13ddc6a43036d6b34706db2b82bbb5a13c40d6de",
        "e1510fcc9fd408884891abfc85bae17bc400b25ff4ccad0c318a9285aa12e3f5",
        include_bytes!(
            "../../../../tests/fixtures/go-frozen/refresh-13ddc6a4/native-triage/linux-arm64/cli.json"
        ),
    ),
    (
        "linux/arm64",
        "mcp",
        "13ddc6a43036d6b34706db2b82bbb5a13c40d6de",
        "549bf39e63973b63f08745ece3b34be445da0c64d2b29d73f87b76228a53b387",
        include_bytes!(
            "../../../../tests/fixtures/go-frozen/refresh-13ddc6a4/native-triage/linux-arm64/mcp.json"
        ),
    ),
    (
        "windows/amd64",
        "cli",
        "13ddc6a43036d6b34706db2b82bbb5a13c40d6de",
        "0f1d7e0afefd7460a81e1c15f37510b8778c2aec44ab6d9b1ae5479b8d670661",
        include_bytes!(
            "../../../../tests/fixtures/go-frozen/refresh-13ddc6a4/native-triage/windows-amd64/cli.json"
        ),
    ),
    (
        "windows/amd64",
        "mcp",
        "13ddc6a43036d6b34706db2b82bbb5a13c40d6de",
        "3d360424d2521720627a017a7524d8103785a24f69dcb19507650aa11c469d44",
        include_bytes!(
            "../../../../tests/fixtures/go-frozen/refresh-13ddc6a4/native-triage/windows-amd64/mcp.json"
        ),
    ),
    (
        "windows/arm64",
        "cli",
        "13ddc6a43036d6b34706db2b82bbb5a13c40d6de",
        "1f3e332909abba044ac34207e04595dc579f55a36d3b8f26638e823488b1b917",
        include_bytes!(
            "../../../../tests/fixtures/go-frozen/refresh-13ddc6a4/native-triage/windows-arm64/cli.json"
        ),
    ),
    (
        "windows/arm64",
        "mcp",
        "13ddc6a43036d6b34706db2b82bbb5a13c40d6de",
        "3deccdaabedd2e74375396b6efe49026eb1f22a3e360d2d3433c79ef8aa037a1",
        include_bytes!(
            "../../../../tests/fixtures/go-frozen/refresh-13ddc6a4/native-triage/windows-arm64/mcp.json"
        ),
    ),
];

fn digest(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

fn verify(raw: &[u8], pin: &str, revision: &str, target: &str, family: &str) -> Value {
    assert!(raw.len() <= 1024 * 1024);
    assert_eq!(digest(raw), pin, "whole measured native triage record");
    let record: Value = serde_json::from_slice(raw).unwrap();
    assert_eq!(record["schema"], "symeraseme.actual-go.native-triage.v1");
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
    for (field, count) in [("source_files", 172), ("archived_generators", 6)] {
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
                let current = std::fs::read(Path::new(ROOT).join(path)).unwrap();
                go_source_pin::assert_current_matches_archive(name, &archive.stdout, &current);
            }
        }
    }
    go_source_pin::assert_checkout_attributes_unchanged(
        ROOT,
        revision,
        ["source_files", "archived_generators"]
            .iter()
            .flat_map(|field| record[*field].as_object().unwrap().keys()),
    );
    assert_eq!(record["family"], family);
    let cases = record["cases"].as_array().unwrap();
    assert_eq!(
        cases.len(),
        match family {
            "cli" => 16,
            "mcp" => 8,
            _ => panic!("unknown family"),
        }
    );
    let mut ids = std::collections::BTreeSet::new();
    for case in cases {
        assert!(ids.insert(case["id"].as_str().unwrap()));
        assert!([0, 1].contains(&case["exit_status"].as_i64().unwrap()));
        for stream in ["stdout", "stderr"] {
            let raw = base64::engine::general_purpose::STANDARD
                .decode(case[format!("{stream}_base64")].as_str().unwrap())
                .unwrap();
            assert!(raw.len() <= 65536);
            assert_eq!(case[format!("{stream}_bytes")], raw.len());
        }
        let invocations = case["actual_agent_invocations"].as_str().unwrap();
        assert!(invocations.len() <= 1024 && invocations.lines().all(|line| line == "invoked"));
        assert!(case["saved_reply_and_ordered_events"]["events"].is_array());
        assert!(case["input"]["argv"].is_array() && case["input"]["environment"].is_object());
        let stdin = base64::engine::general_purpose::STANDARD
            .decode(case["input"]["stdin_base64"].as_str().unwrap())
            .unwrap();
        assert!(stdin.len() <= 65536);
        if family == "cli" {
            assert!(stdin.is_empty());
        }
    }
    record
}

pub struct Corpus(Value);

impl Corpus {
    pub fn case(&self, id: &str, input: &Value) -> Value {
        let matches = self.0["cases"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|case| case["id"] == id)
            .collect::<Vec<_>>();
        assert_eq!(matches.len(), 1);
        assert_eq!(
            &matches[0]["input"], input,
            "original native triage input changed"
        );
        matches[0].clone()
    }
}

pub fn observations(family: &str) -> Option<Corpus> {
    if std::env::var_os("SYMERASEME_CAPTURE_NATIVE_TRIAGE").is_some_and(|value| !value.is_empty()) {
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
    let (_, _, revision, pin, raw) = RECORDS
        .iter()
        .find(|record| record.0 == target && record.1 == family)?;
    let record = verify(raw, pin, revision, target, family);
    let mut corrupt = raw.to_vec();
    corrupt.push(b'!');
    assert!(std::panic::catch_unwind(|| verify(&corrupt, pin, revision, target, family)).is_err());
    assert!(
        std::panic::catch_unwind(|| verify(&raw[..raw.len() - 1], pin, revision, target, family))
            .is_err()
    );
    let mut changed = record.clone();
    changed["source_revision"] = "fabricated-source".into();
    assert!(
        std::panic::catch_unwind(|| verify(
            &serde_json::to_vec(&changed).unwrap(),
            pin,
            revision,
            target,
            family
        ))
        .is_err()
    );
    Some(Corpus(record))
}

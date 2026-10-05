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
        "e8bb6643cbc2a05dbc19f3ad3749513887095ac9",
        "d78af80fa5c68b6174dff70c2a4aa8bce1e22b0421296345a324c17ecdd646af",
        include_bytes!("../../../../tests/fixtures/go-frozen/native-triage/darwin-amd64/cli.json"),
    ),
    (
        "darwin/amd64",
        "mcp",
        "e8bb6643cbc2a05dbc19f3ad3749513887095ac9",
        "df1dcda7f2d49e177f0254a138c374275ffd3f0c5cc7694e14e9a411b5bcc553",
        include_bytes!("../../../../tests/fixtures/go-frozen/native-triage/darwin-amd64/mcp.json"),
    ),
    (
        "darwin/arm64",
        "cli",
        "e8bb6643cbc2a05dbc19f3ad3749513887095ac9",
        "bae4fb753af87ffbac8fdab2fbb927a69507494df9c3e5d9b6db0f8834ad673d",
        include_bytes!("../../../../tests/fixtures/go-frozen/native-triage/darwin-arm64/cli.json"),
    ),
    (
        "darwin/arm64",
        "mcp",
        "e8bb6643cbc2a05dbc19f3ad3749513887095ac9",
        "469fc9b5e519c55d52beccf20e9dab9186e4116ba2d95fd96db98b75197115cf",
        include_bytes!("../../../../tests/fixtures/go-frozen/native-triage/darwin-arm64/mcp.json"),
    ),
    (
        "linux/amd64",
        "cli",
        "e8bb6643cbc2a05dbc19f3ad3749513887095ac9",
        "58ac85c3a9d9682c19735557b0bf07c329f4a966d8b2ad50f909cb261cc27648",
        include_bytes!("../../../../tests/fixtures/go-frozen/native-triage/linux-amd64/cli.json"),
    ),
    (
        "linux/amd64",
        "mcp",
        "e8bb6643cbc2a05dbc19f3ad3749513887095ac9",
        "a01319705fe01734c26aa39da9947be291e5c18751fa317cb3e5f8092e935c84",
        include_bytes!("../../../../tests/fixtures/go-frozen/native-triage/linux-amd64/mcp.json"),
    ),
    (
        "linux/arm64",
        "cli",
        "e8bb6643cbc2a05dbc19f3ad3749513887095ac9",
        "fa72f415b3bea867cfa6dc30f0068c09d952ed5d4244ece2cd5a1ad1810e2b67",
        include_bytes!("../../../../tests/fixtures/go-frozen/native-triage/linux-arm64/cli.json"),
    ),
    (
        "linux/arm64",
        "mcp",
        "e8bb6643cbc2a05dbc19f3ad3749513887095ac9",
        "d08982762bdc08bca5e2240e59ebbbe7ac6ffa5bd475bbe80a30ff7edb451dd3",
        include_bytes!("../../../../tests/fixtures/go-frozen/native-triage/linux-arm64/mcp.json"),
    ),
    (
        "windows/amd64",
        "cli",
        "e8bb6643cbc2a05dbc19f3ad3749513887095ac9",
        "ebf0a604f7f4973cd9c6edcf3199222714bf007d52c3a381f8ad82e89c1c4487",
        include_bytes!("../../../../tests/fixtures/go-frozen/native-triage/windows-amd64/cli.json"),
    ),
    (
        "windows/amd64",
        "mcp",
        "e8bb6643cbc2a05dbc19f3ad3749513887095ac9",
        "1acc1d1a8ff62757e03e9e5d3202ee8ae9dfd572a80671f9ef349177a7e80e46",
        include_bytes!("../../../../tests/fixtures/go-frozen/native-triage/windows-amd64/mcp.json"),
    ),
    (
        "windows/arm64",
        "cli",
        "e8bb6643cbc2a05dbc19f3ad3749513887095ac9",
        "cfe1deb0e374ab9e35387a21f694e16c81771ad9ffcd009473909a077fee5279",
        include_bytes!("../../../../tests/fixtures/go-frozen/native-triage/windows-arm64/cli.json"),
    ),
    (
        "windows/arm64",
        "mcp",
        "e8bb6643cbc2a05dbc19f3ad3749513887095ac9",
        "86bf286ed1bffb858321c6156938aa6fada08a551f222743c4ff77b0cc81e751",
        include_bytes!("../../../../tests/fixtures/go-frozen/native-triage/windows-arm64/mcp.json"),
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
            let current = go_source_pin::current_tree_bound(name)
                .then(|| std::fs::read(Path::new(ROOT).join(path)).unwrap());
            for bytes in std::iter::once(&archive.stdout).chain(&current) {
                assert_eq!(recorded["bytes"], bytes.len(), "{name}");
                assert_eq!(recorded["sha256"], digest(bytes), "{name}");
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

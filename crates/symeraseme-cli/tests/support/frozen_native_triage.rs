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
        "darwin/arm64",
        "cli",
        "2a7a5c1fe6ca1fcf7ffa73ad659eaacba1e7b5ca",
        "b477572a3fc341c7ee26648077964e0d9b58585096c3e9e6e0f7b82de5180228",
        include_bytes!(
            "../../../../tests/fixtures/go-frozen/refresh-2a7a5c1f/native-triage/darwin-arm64/cli.json"
        ),
    ),
    (
        "darwin/arm64",
        "mcp",
        "2a7a5c1fe6ca1fcf7ffa73ad659eaacba1e7b5ca",
        "a7fe29e48b0b1f31abeb3729009f42f10880c1ebf62442cc6882f96966b3bac6",
        include_bytes!(
            "../../../../tests/fixtures/go-frozen/refresh-2a7a5c1f/native-triage/darwin-arm64/mcp.json"
        ),
    ),
    (
        "linux/amd64",
        "cli",
        "2a7a5c1fe6ca1fcf7ffa73ad659eaacba1e7b5ca",
        "28e9927a5b35cf3ec855ad54a425f61f5d1047bc36832ea3d4ad8b754aa4c47c",
        include_bytes!(
            "../../../../tests/fixtures/go-frozen/refresh-2a7a5c1f/native-triage/linux-amd64/cli.json"
        ),
    ),
    (
        "linux/amd64",
        "mcp",
        "2a7a5c1fe6ca1fcf7ffa73ad659eaacba1e7b5ca",
        "23b54a13977e6ef127e9adaaee091af7092557a0e48b96e15eecbc2a869e508e",
        include_bytes!(
            "../../../../tests/fixtures/go-frozen/refresh-2a7a5c1f/native-triage/linux-amd64/mcp.json"
        ),
    ),
    (
        "linux/arm64",
        "cli",
        "2a7a5c1fe6ca1fcf7ffa73ad659eaacba1e7b5ca",
        "f4dda8751a6eb6acd176e109b2a3b76837e8c3455851b7ef4eaefe88dc3598cc",
        include_bytes!(
            "../../../../tests/fixtures/go-frozen/refresh-2a7a5c1f/native-triage/linux-arm64/cli.json"
        ),
    ),
    (
        "linux/arm64",
        "mcp",
        "2a7a5c1fe6ca1fcf7ffa73ad659eaacba1e7b5ca",
        "52b3bb7a4d6639758fc6caf21eae00789284de31b03bde9ae55910b5ca542b55",
        include_bytes!(
            "../../../../tests/fixtures/go-frozen/refresh-2a7a5c1f/native-triage/linux-arm64/mcp.json"
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
            if matches!(
                name.as_str(),
                "crates/symeraseme-cli/tests/triage_native_process.rs"
                    | "crates/symeraseme-cli/tests/support/capture_native_triage.rs"
            ) {
                // Retired producers stay bound to the independent Go-era pin,
                // using the original recorded checkout policy on every host.
                let pinned = Command::new("git")
                    .args([
                        "-c",
                        &format!("core.autocrlf={autocrlf}"),
                        "cat-file",
                        "--filters",
                        &format!("{}:{name}", go_source_pin::RETIREMENT_SOURCE_REVISION),
                    ])
                    .current_dir(ROOT)
                    .output()
                    .expect("read independently pinned triage producer");
                assert!(
                    pinned.status.success(),
                    "retired triage producer unavailable"
                );
                assert_eq!(
                    archive.stdout, pinned.stdout,
                    "retired triage producer: {name}"
                );
            } else if go_source_pin::current_tree_bound(name) {
                let current = go_source_pin::current_source(ROOT, name);
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

pub fn observations(family: &str) -> Corpus {
    assert!(
        std::env::var_os("SYMERASEME_CAPTURE_NATIVE_TRIAGE").is_none(),
        "live Go capture is retired; use the immutable historical capture producer"
    );
    match std::env::var("SYMERASEME_PARITY_LIVE_GO").as_deref() {
        Ok("0") | Err(std::env::VarError::NotPresent) => {}
        _ => panic!("live Go execution is retired; SYMERASEME_PARITY_LIVE_GO must be 0 or unset"),
    }
    let target = match (std::env::consts::OS, std::env::consts::ARCH) {
        ("linux", "x86_64") => "linux/amd64",
        ("linux", "aarch64") => "linux/arm64",
        ("macos", "aarch64") => "darwin/arm64",
        _ => panic!("no recorded native triage capture for this target; live Go is retired"),
    };
    let (_, _, revision, pin, raw) = RECORDS
        .iter()
        .find(|record| record.0 == target && record.1 == family)
        .expect("recorded native triage family; live Go is retired");
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
    Corpus(record)
}

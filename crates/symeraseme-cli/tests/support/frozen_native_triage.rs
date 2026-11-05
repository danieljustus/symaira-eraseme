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
        "433622f6e6c884694a304f483ae481f93069b455",
        "0ca09e6c673f5c4cb7aa428edb8f6442ba52d544d930db2abe1d0b66e8ce41f6",
        include_bytes!(
            "../../../../tests/fixtures/go-frozen/refresh-433622f6/native-triage/darwin-amd64/cli.json"
        ),
    ),
    (
        "darwin/amd64",
        "mcp",
        "433622f6e6c884694a304f483ae481f93069b455",
        "27f39ef029373d9f0f2a572b240bcca46f4a4ac1c7f586573cace88b1ca6a473",
        include_bytes!(
            "../../../../tests/fixtures/go-frozen/refresh-433622f6/native-triage/darwin-amd64/mcp.json"
        ),
    ),
    (
        "darwin/arm64",
        "cli",
        "433622f6e6c884694a304f483ae481f93069b455",
        "2d4b983f243cf65857e7b528c0fbe467d639503bfe636894b2ce1fc479198c5a",
        include_bytes!(
            "../../../../tests/fixtures/go-frozen/refresh-433622f6/native-triage/darwin-arm64/cli.json"
        ),
    ),
    (
        "darwin/arm64",
        "mcp",
        "433622f6e6c884694a304f483ae481f93069b455",
        "e62e7c9248a32e2f5452f0111e0644b8775676d02c550b2fcf6f51a015ec7ff9",
        include_bytes!(
            "../../../../tests/fixtures/go-frozen/refresh-433622f6/native-triage/darwin-arm64/mcp.json"
        ),
    ),
    (
        "linux/amd64",
        "cli",
        "433622f6e6c884694a304f483ae481f93069b455",
        "17a49ffa5345b7d0e3e5e2834514ff60e5b5b2d113fe6919690468768044cac2",
        include_bytes!(
            "../../../../tests/fixtures/go-frozen/refresh-433622f6/native-triage/linux-amd64/cli.json"
        ),
    ),
    (
        "linux/amd64",
        "mcp",
        "433622f6e6c884694a304f483ae481f93069b455",
        "bc25782248dee18240e83e4162e6f9698e2624c7b9f3aa945f9bff2cf9c992c5",
        include_bytes!(
            "../../../../tests/fixtures/go-frozen/refresh-433622f6/native-triage/linux-amd64/mcp.json"
        ),
    ),
    (
        "linux/arm64",
        "cli",
        "433622f6e6c884694a304f483ae481f93069b455",
        "d5b3faf1743db8b35812bbc4838dd04bda139dd4ba3bd1a09a5192ac2e6ce077",
        include_bytes!(
            "../../../../tests/fixtures/go-frozen/refresh-433622f6/native-triage/linux-arm64/cli.json"
        ),
    ),
    (
        "linux/arm64",
        "mcp",
        "433622f6e6c884694a304f483ae481f93069b455",
        "b8a29b906d287a80c6f11246a92f7c9c1f123c4f177f8d40cc92063460d45c50",
        include_bytes!(
            "../../../../tests/fixtures/go-frozen/refresh-433622f6/native-triage/linux-arm64/mcp.json"
        ),
    ),
    (
        "windows/amd64",
        "cli",
        "433622f6e6c884694a304f483ae481f93069b455",
        "c6327218f98ab9845660d1df78f0bc8f6a5698783b0d5e5651b9fef3a09c591e",
        include_bytes!(
            "../../../../tests/fixtures/go-frozen/refresh-433622f6/native-triage/windows-amd64/cli.json"
        ),
    ),
    (
        "windows/amd64",
        "mcp",
        "433622f6e6c884694a304f483ae481f93069b455",
        "89ec5dbf595928157c13dc8009d999e4234a42b1335652df947e3f8e8d32f504",
        include_bytes!(
            "../../../../tests/fixtures/go-frozen/refresh-433622f6/native-triage/windows-amd64/mcp.json"
        ),
    ),
    (
        "windows/arm64",
        "cli",
        "433622f6e6c884694a304f483ae481f93069b455",
        "a5e214b7458cec77958381cdcffb8f2fa2f1d50f3a3305d76bdc160cfdf35e0b",
        include_bytes!(
            "../../../../tests/fixtures/go-frozen/refresh-433622f6/native-triage/windows-arm64/cli.json"
        ),
    ),
    (
        "windows/arm64",
        "mcp",
        "433622f6e6c884694a304f483ae481f93069b455",
        "d7b7d69b744bb8775732614c735a0d96ccbd9cdc25618b347e9df675bec710a4",
        include_bytes!(
            "../../../../tests/fixtures/go-frozen/refresh-433622f6/native-triage/windows-arm64/mcp.json"
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
        ("macos", "x86_64") => "darwin/amd64",
        ("macos", "aarch64") => "darwin/arm64",
        ("windows", "x86_64") => "windows/amd64",
        ("windows", "aarch64") => "windows/arm64",
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

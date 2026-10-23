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
        "e2cf176337f6f90e64f76b5f39d3a859e87774ea",
        "f1d189140da200efc04d1a5ead6cbd7bd962e80ea3b2c168bd239e97a66345bd",
        include_bytes!(
            "../../../../tests/fixtures/go-frozen/refresh-e2cf1763/native-triage/darwin-amd64/cli.json"
        ),
    ),
    (
        "darwin/amd64",
        "mcp",
        "e2cf176337f6f90e64f76b5f39d3a859e87774ea",
        "4cb6ebbe498ea54279b96ff8abf9d347e8c9a3348f12063e04632bf7ce268454",
        include_bytes!(
            "../../../../tests/fixtures/go-frozen/refresh-e2cf1763/native-triage/darwin-amd64/mcp.json"
        ),
    ),
    (
        "darwin/arm64",
        "cli",
        "e2cf176337f6f90e64f76b5f39d3a859e87774ea",
        "d9a08130c46116e64f2dbb192c1e0bfa710145ca38a60bff3307b00ff6d1d098",
        include_bytes!(
            "../../../../tests/fixtures/go-frozen/refresh-e2cf1763/native-triage/darwin-arm64/cli.json"
        ),
    ),
    (
        "darwin/arm64",
        "mcp",
        "e2cf176337f6f90e64f76b5f39d3a859e87774ea",
        "5e54c13c02a65cf5908f9bd8d44b1d194530385fffb5686160f3294840450dd0",
        include_bytes!(
            "../../../../tests/fixtures/go-frozen/refresh-e2cf1763/native-triage/darwin-arm64/mcp.json"
        ),
    ),
    (
        "linux/amd64",
        "cli",
        "e2cf176337f6f90e64f76b5f39d3a859e87774ea",
        "ede6181d1cd3b1312d9d5ce89ebc741e801f74e49ad1dfb67001911526e6dc9d",
        include_bytes!(
            "../../../../tests/fixtures/go-frozen/refresh-e2cf1763/native-triage/linux-amd64/cli.json"
        ),
    ),
    (
        "linux/amd64",
        "mcp",
        "e2cf176337f6f90e64f76b5f39d3a859e87774ea",
        "ceb226f4b6a023f7aa2f434e21d83d7c1e3a371feacadc1e253561d7a9f6f05a",
        include_bytes!(
            "../../../../tests/fixtures/go-frozen/refresh-e2cf1763/native-triage/linux-amd64/mcp.json"
        ),
    ),
    (
        "linux/arm64",
        "cli",
        "e2cf176337f6f90e64f76b5f39d3a859e87774ea",
        "3e79155bedc10bb4bddaf4531253fadeddbe2ca1deda99064e0754dac3c6cfc5",
        include_bytes!(
            "../../../../tests/fixtures/go-frozen/refresh-e2cf1763/native-triage/linux-arm64/cli.json"
        ),
    ),
    (
        "linux/arm64",
        "mcp",
        "e2cf176337f6f90e64f76b5f39d3a859e87774ea",
        "fae87bbddaab5d8f87daf7a4c7f059d9bc2ee9c1b50626c1026bfba51ad8ef41",
        include_bytes!(
            "../../../../tests/fixtures/go-frozen/refresh-e2cf1763/native-triage/linux-arm64/mcp.json"
        ),
    ),
    (
        "windows/amd64",
        "cli",
        "e2cf176337f6f90e64f76b5f39d3a859e87774ea",
        "d25bac37871ff712555e3f8ca1175ad37f0b2d86aedbccb2fc7cdec4f60e7249",
        include_bytes!(
            "../../../../tests/fixtures/go-frozen/refresh-e2cf1763/native-triage/windows-amd64/cli.json"
        ),
    ),
    (
        "windows/amd64",
        "mcp",
        "e2cf176337f6f90e64f76b5f39d3a859e87774ea",
        "5d319a7cae53277443495658c9579c9606807e8f6e63701efbe989cb9f6c56ba",
        include_bytes!(
            "../../../../tests/fixtures/go-frozen/refresh-e2cf1763/native-triage/windows-amd64/mcp.json"
        ),
    ),
    (
        "windows/arm64",
        "cli",
        "e2cf176337f6f90e64f76b5f39d3a859e87774ea",
        "aa51b237cd2a57b25ea6dd5792563dcdadb16440bc28e795ebf47b766c5b8330",
        include_bytes!(
            "../../../../tests/fixtures/go-frozen/refresh-e2cf1763/native-triage/windows-arm64/cli.json"
        ),
    ),
    (
        "windows/arm64",
        "mcp",
        "e2cf176337f6f90e64f76b5f39d3a859e87774ea",
        "9feca3aadaafa24e3ce4fde0b54f354a9ac6b745a191b9e9a4e78918d15d03ba",
        include_bytes!(
            "../../../../tests/fixtures/go-frozen/refresh-e2cf1763/native-triage/windows-arm64/mcp.json"
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

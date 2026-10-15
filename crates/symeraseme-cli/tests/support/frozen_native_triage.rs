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
        "968e1a2dda9c621ba880e7b618dc49daa31970b3",
        "d2f9b6a91eeee2e783d29a458415455756284a31565ca2d083542e25151001bb",
        include_bytes!(
            "../../../../tests/fixtures/go-frozen/refresh-968e1a2d/native-triage/darwin-amd64/cli.json"
        ),
    ),
    (
        "darwin/amd64",
        "mcp",
        "968e1a2dda9c621ba880e7b618dc49daa31970b3",
        "761de8bab9b4fb18c4bb109a1aebb8db0fd4fbb7a39f27375b54c79deac306f2",
        include_bytes!(
            "../../../../tests/fixtures/go-frozen/refresh-968e1a2d/native-triage/darwin-amd64/mcp.json"
        ),
    ),
    (
        "darwin/arm64",
        "cli",
        "968e1a2dda9c621ba880e7b618dc49daa31970b3",
        "7f47b35c0e8671096be25a25cde1cd3b499417df29f16ebfd12948d71d0e69cc",
        include_bytes!(
            "../../../../tests/fixtures/go-frozen/refresh-968e1a2d/native-triage/darwin-arm64/cli.json"
        ),
    ),
    (
        "darwin/arm64",
        "mcp",
        "968e1a2dda9c621ba880e7b618dc49daa31970b3",
        "08e01edb3b6df295316495adeb3f1271ff0bb7272d7c6f4fc7d52306cbd51258",
        include_bytes!(
            "../../../../tests/fixtures/go-frozen/refresh-968e1a2d/native-triage/darwin-arm64/mcp.json"
        ),
    ),
    (
        "linux/amd64",
        "cli",
        "968e1a2dda9c621ba880e7b618dc49daa31970b3",
        "301a54bb9fe46efa35cdc931e4a0d48ee71c1f5ef4ab64f6cdba7283000297c0",
        include_bytes!(
            "../../../../tests/fixtures/go-frozen/refresh-968e1a2d/native-triage/linux-amd64/cli.json"
        ),
    ),
    (
        "linux/amd64",
        "mcp",
        "968e1a2dda9c621ba880e7b618dc49daa31970b3",
        "28c33ccc2d123b21f574dfd224be68f3d9556d136d012305f8ead34e3ddbf4d9",
        include_bytes!(
            "../../../../tests/fixtures/go-frozen/refresh-968e1a2d/native-triage/linux-amd64/mcp.json"
        ),
    ),
    (
        "linux/arm64",
        "cli",
        "968e1a2dda9c621ba880e7b618dc49daa31970b3",
        "d3eb2e729177785ace8548e50fa379ce4f4f5b656cc90038b18e0367d5df7269",
        include_bytes!(
            "../../../../tests/fixtures/go-frozen/refresh-968e1a2d/native-triage/linux-arm64/cli.json"
        ),
    ),
    (
        "linux/arm64",
        "mcp",
        "968e1a2dda9c621ba880e7b618dc49daa31970b3",
        "70cf0afdd7ea07290a6553c737c406f00bf032e89e286ec40840c90b0513d91f",
        include_bytes!(
            "../../../../tests/fixtures/go-frozen/refresh-968e1a2d/native-triage/linux-arm64/mcp.json"
        ),
    ),
    (
        "windows/amd64",
        "cli",
        "968e1a2dda9c621ba880e7b618dc49daa31970b3",
        "b2c7f905cb7673e981cd8d18089d0dc7079f2ad73a5b009135031ae632c5eadc",
        include_bytes!(
            "../../../../tests/fixtures/go-frozen/refresh-968e1a2d/native-triage/windows-amd64/cli.json"
        ),
    ),
    (
        "windows/amd64",
        "mcp",
        "968e1a2dda9c621ba880e7b618dc49daa31970b3",
        "b4cd8c02de4e8a1e9c7ab935f860b022eb716682e92890b489c9a7a62063351a",
        include_bytes!(
            "../../../../tests/fixtures/go-frozen/refresh-968e1a2d/native-triage/windows-amd64/mcp.json"
        ),
    ),
    (
        "windows/arm64",
        "cli",
        "968e1a2dda9c621ba880e7b618dc49daa31970b3",
        "bc3116a404ab93686b12f396be947a1642a9725957971dffac2ea636d6ad134e",
        include_bytes!(
            "../../../../tests/fixtures/go-frozen/refresh-968e1a2d/native-triage/windows-arm64/cli.json"
        ),
    ),
    (
        "windows/arm64",
        "mcp",
        "968e1a2dda9c621ba880e7b618dc49daa31970b3",
        "1c49ad8db7a7719ea1c0cc88c22e2922ef60f7f69051c6c50e2587ff0fea0a1b",
        include_bytes!(
            "../../../../tests/fixtures/go-frozen/refresh-968e1a2d/native-triage/windows-arm64/mcp.json"
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

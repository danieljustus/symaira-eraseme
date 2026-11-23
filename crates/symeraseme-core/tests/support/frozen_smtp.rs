//! Original SMTP process and transaction comparisons backed only by measured native Go responses.
use base64::Engine;
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::path::{Component, Path};
use std::process::Command;

#[path = "../../../symeraseme-cli/tests/support/go_source_pin.rs"]
pub(crate) mod go_source_pin;

const ROOT: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../..");
// Populate only after retaining and independently verifying actual native captures.
const RECORDS: &[(&str, &str, &str, &str, &[u8])] = &[
    (
        "darwin/arm64",
        "campaign",
        "a8eda4f77ce938c32d18dbad7d45de148b7d0482",
        "32009ec3c1b21f1f22ed47111aa1ad53905170e27f581b808078bb3541ebd8c1",
        include_bytes!(
            "../../../../tests/fixtures/go-frozen/refresh-a8eda4f7/smtp/darwin-arm64/campaign.json"
        ),
    ),
    (
        "darwin/arm64",
        "transport",
        "a8eda4f77ce938c32d18dbad7d45de148b7d0482",
        "e2de35cc57334bb624436b4b869cfe7055bcefc2707e0ac6e7a898c538d2851a",
        include_bytes!(
            "../../../../tests/fixtures/go-frozen/refresh-a8eda4f7/smtp/darwin-arm64/transport.json"
        ),
    ),
    (
        "linux/amd64",
        "campaign",
        "a8eda4f77ce938c32d18dbad7d45de148b7d0482",
        "5ab3b5b907368ee44ea792f0ad48ed38ccfa70ceb6bf8603916eefbded269ce4",
        include_bytes!(
            "../../../../tests/fixtures/go-frozen/refresh-a8eda4f7/smtp/linux-amd64/campaign.json"
        ),
    ),
    (
        "linux/amd64",
        "transport",
        "a8eda4f77ce938c32d18dbad7d45de148b7d0482",
        "8814d8e42e22befcd2f976b860a2e7ef5a638ad1ae6e5085d07828fe92ffe137",
        include_bytes!(
            "../../../../tests/fixtures/go-frozen/refresh-a8eda4f7/smtp/linux-amd64/transport.json"
        ),
    ),
    (
        "linux/arm64",
        "campaign",
        "a8eda4f77ce938c32d18dbad7d45de148b7d0482",
        "3e4474a2e844db60ddb759202ed94977db4584a4c78135c8cddc5b23d1934497",
        include_bytes!(
            "../../../../tests/fixtures/go-frozen/refresh-a8eda4f7/smtp/linux-arm64/campaign.json"
        ),
    ),
    (
        "linux/arm64",
        "transport",
        "a8eda4f77ce938c32d18dbad7d45de148b7d0482",
        "fb5f64693aef0c5f535dc7d8cd15241dd244f82ef9f442eccc277a800c4777b2",
        include_bytes!(
            "../../../../tests/fixtures/go-frozen/refresh-a8eda4f7/smtp/linux-arm64/transport.json"
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
    assert_eq!(digest(raw), pin, "whole measured HTTP record");
    let record: Value = serde_json::from_slice(raw).unwrap();
    assert_eq!(record["schema"], "symeraseme.actual-go.smtp.v1");
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
    for (field, count) in [("source_files", 172), ("archived_generators", 4)] {
        let files = record[field].as_object().unwrap();
        assert_eq!(files.len(), count);
        for (name, recorded) in files {
            let path = Path::new(name);
            assert!(
                !path.is_absolute() && path.components().all(|c| matches!(c, Component::Normal(_)))
            );
            let archive = Command::new("git")
                .args(["show", &format!("{revision}:{name}")])
                .current_dir(ROOT)
                .output()
                .unwrap();
            assert!(
                archive.status.success(),
                "immutable HTTP producer must remain verifiable"
            );
            // The former capture producers are retired or Rust-only replay tests.
            // Bind their original bytes to the independent retirement commit,
            // not to the changed replay implementation. All other inputs retain
            // the current-tree check below, including Cargo.lock.
            let retired_producer = field == "archived_generators"
                && matches!(
                    name.as_str(),
                    "crates/symeraseme-core/tests/campaign_smtp.rs"
                        | "crates/symeraseme-core/tests/smtp_transport.rs"
                        | "crates/symeraseme-core/tests/support/capture_smtp.rs"
                );
            if retired_producer {
                go_source_pin::assert_retired_generator_matches_archive(
                    ROOT,
                    name,
                    &archive.stdout,
                );
            }
            let archived_bytes = archive.stdout;
            assert_eq!(recorded["bytes"], archived_bytes.len(), "{name}");
            assert_eq!(recorded["sha256"], digest(&archived_bytes), "{name}");
            if !retired_producer && go_source_pin::current_tree_bound(name) {
                let current = go_source_pin::current_source(ROOT, name);
                go_source_pin::assert_current_matches_archive(name, &archived_bytes, &current);
            }
        }
    }
    let cases = record["cases"].as_array().unwrap();
    assert_eq!(record["family"], family);
    let expected: &[&str] = match family {
        "campaign" => &["campaign"],
        "transport" => &[
            "plain",
            "oauth",
            "auth-rejected",
            "auth-challenge",
            "credential-echo",
            "missing-starttls",
            "data-rejected",
            "greeting-rejected",
            "helo-fallback",
        ],
        _ => panic!("unknown SMTP family"),
    };
    assert_eq!(cases.len(), expected.len());
    for (case, name) in cases.iter().zip(expected) {
        assert_eq!(case["name"], *name);
        assert_eq!(case["exit_status"], 0);
        for stream in ["stdin", "stdout", "stderr"] {
            let bytes = base64::engine::general_purpose::STANDARD
                .decode(case[format!("{stream}_base64")].as_str().unwrap())
                .unwrap();
            assert!(bytes.len() <= 1024 * 1024);
            assert_eq!(case[stream]["bytes"], bytes.len());
            assert_eq!(case[stream]["sha256"], digest(&bytes));
        }
        let transcript = case["smtp_transcript"].as_array().unwrap();
        assert_eq!(transcript.is_empty(), *name == "greeting-rejected");
    }
    record
}

pub struct Corpus(Value);

impl Corpus {
    fn case(&self, name: &str) -> &Value {
        self.0["cases"]
            .as_array()
            .unwrap()
            .iter()
            .find(|case| case["name"] == name)
            .unwrap()
    }
    pub fn input(&self, name: &str) -> Vec<u8> {
        base64::engine::general_purpose::STANDARD
            .decode(self.case(name)["stdin_base64"].as_str().unwrap())
            .unwrap()
    }
    pub fn observation(&self, name: &str, input: &[u8]) -> (std::process::Output, Vec<String>) {
        let case = self.case(name);
        assert_eq!(self.input(name), input, "original SMTP input changed");
        let status = {
            use std::os::unix::process::ExitStatusExt;
            std::process::ExitStatus::from_raw(0)
        };
        (
            std::process::Output {
                status,
                stdout: base64::engine::general_purpose::STANDARD
                    .decode(case["stdout_base64"].as_str().unwrap())
                    .unwrap(),
                stderr: base64::engine::general_purpose::STANDARD
                    .decode(case["stderr_base64"].as_str().unwrap())
                    .unwrap(),
            },
            serde_json::from_value(case["smtp_transcript"].clone()).unwrap(),
        )
    }
}

pub fn observations(family: &str) -> Corpus {
    assert!(
        !std::env::var_os("SYMERASEME_CAPTURE_SMTP").is_some_and(|value| !value.is_empty()),
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
        _ => panic!("no recorded native SMTP capture for this target; live Go is retired"),
    };
    let (_, _, revision, pin, raw) = RECORDS
        .iter()
        .find(|record| record.0 == target && record.1 == family)
        .expect("no recorded SMTP family for the native target");
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

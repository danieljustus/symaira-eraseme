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
        "darwin/amd64",
        "campaign",
        "968e1a2dda9c621ba880e7b618dc49daa31970b3",
        "0e61b715558e698f329299ae3b49606c53d1668c2732b91b4949caeb82a27afd",
        include_bytes!(
            "../../../../tests/fixtures/go-frozen/refresh-968e1a2d/smtp/darwin-amd64/campaign.json"
        ),
    ),
    (
        "darwin/amd64",
        "transport",
        "968e1a2dda9c621ba880e7b618dc49daa31970b3",
        "1335bfb2f56433005f9e0649e6ab60ac9d01207a4ede4efc16aa4e498bafb70d",
        include_bytes!(
            "../../../../tests/fixtures/go-frozen/refresh-968e1a2d/smtp/darwin-amd64/transport.json"
        ),
    ),
    (
        "darwin/arm64",
        "campaign",
        "968e1a2dda9c621ba880e7b618dc49daa31970b3",
        "b0b0e8f73d2df50559c226d10be051889078af5375767819792821936736774d",
        include_bytes!(
            "../../../../tests/fixtures/go-frozen/refresh-968e1a2d/smtp/darwin-arm64/campaign.json"
        ),
    ),
    (
        "darwin/arm64",
        "transport",
        "968e1a2dda9c621ba880e7b618dc49daa31970b3",
        "aaf2571ad8ef887e5032bdf4224bd0c0b44062c5acca526662897a3a3305eabe",
        include_bytes!(
            "../../../../tests/fixtures/go-frozen/refresh-968e1a2d/smtp/darwin-arm64/transport.json"
        ),
    ),
    (
        "linux/amd64",
        "campaign",
        "968e1a2dda9c621ba880e7b618dc49daa31970b3",
        "85909375da6e65a3ddec15eb90b726cf190dd099de28d8ef00f799eaeb9b367e",
        include_bytes!(
            "../../../../tests/fixtures/go-frozen/refresh-968e1a2d/smtp/linux-amd64/campaign.json"
        ),
    ),
    (
        "linux/amd64",
        "transport",
        "968e1a2dda9c621ba880e7b618dc49daa31970b3",
        "940a9527461d1a3f43ab52b08854e79ab9b48cc8e75d5d29734118aa1a7901d0",
        include_bytes!(
            "../../../../tests/fixtures/go-frozen/refresh-968e1a2d/smtp/linux-amd64/transport.json"
        ),
    ),
    (
        "linux/arm64",
        "campaign",
        "968e1a2dda9c621ba880e7b618dc49daa31970b3",
        "2e5cda7ba5e1de0725e8f1dc91633ab2fdc1d519ce4a7ae254ca022e123d4a04",
        include_bytes!(
            "../../../../tests/fixtures/go-frozen/refresh-968e1a2d/smtp/linux-arm64/campaign.json"
        ),
    ),
    (
        "linux/arm64",
        "transport",
        "968e1a2dda9c621ba880e7b618dc49daa31970b3",
        "01f277f4e60b35f8f16c1aec72dd3392127ed9e2cee9ee464eee5fe3ab826f41",
        include_bytes!(
            "../../../../tests/fixtures/go-frozen/refresh-968e1a2d/smtp/linux-arm64/transport.json"
        ),
    ),
    (
        "windows/amd64",
        "campaign",
        "968e1a2dda9c621ba880e7b618dc49daa31970b3",
        "dcdcf84ef7fe56580ffddf05df0e59284a902192f8deab9c9a152656ae939b3d",
        include_bytes!(
            "../../../../tests/fixtures/go-frozen/refresh-968e1a2d/smtp/windows-amd64/campaign.json"
        ),
    ),
    (
        "windows/amd64",
        "transport",
        "968e1a2dda9c621ba880e7b618dc49daa31970b3",
        "4c85af71d0977fcbc12a35dbc4fb02478d3b4239a3b00167b83b78cb0c553da0",
        include_bytes!(
            "../../../../tests/fixtures/go-frozen/refresh-968e1a2d/smtp/windows-amd64/transport.json"
        ),
    ),
    (
        "windows/arm64",
        "campaign",
        "968e1a2dda9c621ba880e7b618dc49daa31970b3",
        "339dcebde4b0333bede71c240c6f6557bca54e8b20eef71a3e67183696c9b76c",
        include_bytes!(
            "../../../../tests/fixtures/go-frozen/refresh-968e1a2d/smtp/windows-arm64/campaign.json"
        ),
    ),
    (
        "windows/arm64",
        "transport",
        "968e1a2dda9c621ba880e7b618dc49daa31970b3",
        "cec214e00d8c3c622f0dc1b784aa002aecd68bbbcc4802392cdf4c31fc7d6186",
        include_bytes!(
            "../../../../tests/fixtures/go-frozen/refresh-968e1a2d/smtp/windows-arm64/transport.json"
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
            let archived_bytes = if field == "archived_generators" && target.starts_with("windows/")
            {
                assert!(!archive.stdout.windows(2).any(|bytes| bytes == b"\r\n"));
                let mut transformed = Vec::new();
                for byte in archive.stdout {
                    if byte == b'\n' {
                        transformed.push(b'\r');
                    }
                    transformed.push(byte);
                }
                transformed
            } else {
                archive.stdout
            };
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
        #[cfg(unix)]
        let status = {
            use std::os::unix::process::ExitStatusExt;
            std::process::ExitStatus::from_raw(0)
        };
        #[cfg(windows)]
        let status = {
            use std::os::windows::process::ExitStatusExt;
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
        ("windows", "x86_64") => "windows/amd64",
        ("windows", "aarch64") => "windows/arm64",
        ("macos", "x86_64") => "darwin/amd64",
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

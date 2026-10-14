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
        "13ddc6a43036d6b34706db2b82bbb5a13c40d6de",
        "6a391d93a54a6dc70f84105a89f5e163cb14a597660a32ebaa9b54bcf17cd954",
        include_bytes!(
            "../../../../tests/fixtures/go-frozen/refresh-13ddc6a4/smtp/darwin-amd64/campaign.json"
        ),
    ),
    (
        "darwin/amd64",
        "transport",
        "13ddc6a43036d6b34706db2b82bbb5a13c40d6de",
        "6ed4ef8f1aac72c690a0926769b4b422090cacb128ef3f1bf7b139d7c4b76106",
        include_bytes!(
            "../../../../tests/fixtures/go-frozen/refresh-13ddc6a4/smtp/darwin-amd64/transport.json"
        ),
    ),
    (
        "darwin/arm64",
        "campaign",
        "13ddc6a43036d6b34706db2b82bbb5a13c40d6de",
        "80aeca61c669be98415c4d62466ef01c29a9a17121fab870d771701c1b463ff6",
        include_bytes!(
            "../../../../tests/fixtures/go-frozen/refresh-13ddc6a4/smtp/darwin-arm64/campaign.json"
        ),
    ),
    (
        "darwin/arm64",
        "transport",
        "13ddc6a43036d6b34706db2b82bbb5a13c40d6de",
        "98444153d364dc59920bdf151ee41f9de4848d06f7868fe6f524cddd3d2ce107",
        include_bytes!(
            "../../../../tests/fixtures/go-frozen/refresh-13ddc6a4/smtp/darwin-arm64/transport.json"
        ),
    ),
    (
        "linux/amd64",
        "campaign",
        "13ddc6a43036d6b34706db2b82bbb5a13c40d6de",
        "b3d98e5e1b4dd29f2f78e5bd15964144428ce383975fbdc5e70a07a9a6f71617",
        include_bytes!(
            "../../../../tests/fixtures/go-frozen/refresh-13ddc6a4/smtp/linux-amd64/campaign.json"
        ),
    ),
    (
        "linux/amd64",
        "transport",
        "13ddc6a43036d6b34706db2b82bbb5a13c40d6de",
        "42e54e41ddfed149e78889215fa4b362458026adfd71bba50580fbdf76781d20",
        include_bytes!(
            "../../../../tests/fixtures/go-frozen/refresh-13ddc6a4/smtp/linux-amd64/transport.json"
        ),
    ),
    (
        "linux/arm64",
        "campaign",
        "13ddc6a43036d6b34706db2b82bbb5a13c40d6de",
        "df4057c5913fcb9f969cc4c336cc3d48be5e1fc52bcff7de788a972124b73cf5",
        include_bytes!(
            "../../../../tests/fixtures/go-frozen/refresh-13ddc6a4/smtp/linux-arm64/campaign.json"
        ),
    ),
    (
        "linux/arm64",
        "transport",
        "13ddc6a43036d6b34706db2b82bbb5a13c40d6de",
        "543f4667fa37dab9bdaecb579679f3b50e17c4938ff09ef1f600765b9a016330",
        include_bytes!(
            "../../../../tests/fixtures/go-frozen/refresh-13ddc6a4/smtp/linux-arm64/transport.json"
        ),
    ),
    (
        "windows/amd64",
        "campaign",
        "13ddc6a43036d6b34706db2b82bbb5a13c40d6de",
        "2ca3001c400368cdf7b26cf60a5e0b57cbbef6a5c7e6f83237da9376b4bfd599",
        include_bytes!(
            "../../../../tests/fixtures/go-frozen/refresh-13ddc6a4/smtp/windows-amd64/campaign.json"
        ),
    ),
    (
        "windows/amd64",
        "transport",
        "13ddc6a43036d6b34706db2b82bbb5a13c40d6de",
        "750d30bdee128eb656af458491ff9b200080bba52fad6419bae06689210b89fe",
        include_bytes!(
            "../../../../tests/fixtures/go-frozen/refresh-13ddc6a4/smtp/windows-amd64/transport.json"
        ),
    ),
    (
        "windows/arm64",
        "campaign",
        "13ddc6a43036d6b34706db2b82bbb5a13c40d6de",
        "ce876267bd13099dbcf45b801423e611fad61a8c7960c19f4b09035cbbcffc91",
        include_bytes!(
            "../../../../tests/fixtures/go-frozen/refresh-13ddc6a4/smtp/windows-arm64/campaign.json"
        ),
    ),
    (
        "windows/arm64",
        "transport",
        "13ddc6a43036d6b34706db2b82bbb5a13c40d6de",
        "15b5752ef279c5b5db88db318ddb601aee6c64a501e9068a8eb53cf4d7935086",
        include_bytes!(
            "../../../../tests/fixtures/go-frozen/refresh-13ddc6a4/smtp/windows-arm64/transport.json"
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

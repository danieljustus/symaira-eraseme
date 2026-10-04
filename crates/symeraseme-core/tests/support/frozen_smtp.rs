//! Original SMTP process and transaction comparisons backed only by measured native Go responses.
use base64::Engine;
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::path::{Component, Path};
use std::process::Command;

const ROOT: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../..");
// Populate only after retaining and independently verifying actual native captures.
const RECORDS: &[(&str, &str, &str, &str, &[u8])] = &[];

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
            let current = std::fs::read(Path::new(ROOT).join(path)).unwrap();
            for bytes in [&archived_bytes, &current] {
                assert_eq!(recorded["bytes"], bytes.len(), "{name}");
                assert_eq!(recorded["sha256"], digest(bytes), "{name}");
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

pub fn observations(family: &str) -> Option<Corpus> {
    if std::env::var_os("SYMERASEME_CAPTURE_SMTP").is_some_and(|value| !value.is_empty()) {
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
        ("windows", "x86_64") => "windows/amd64",
        ("windows", "aarch64") => "windows/arm64",
        ("macos", "x86_64") => "darwin/amd64",
        ("macos", "aarch64") => "darwin/arm64",
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

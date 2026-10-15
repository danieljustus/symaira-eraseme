//! Complete actual native Linux/Windows Go plan processes and persisted effects.
use base64::Engine;
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::path::{Component, Path};
use std::process::{Command, Output};

#[path = "go_source_pin.rs"]
mod go_source_pin;

const ROOT: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../..");
const REVISION: &str = "ac0b3b4b5c5729c9cbcf1ffbd5c7efce59440a33";

fn digest(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

fn verify(raw: &[u8], expected: &str, case: &str, target: &str) -> Value {
    assert_eq!(digest(raw), expected, "whole actual process record");
    let record: Value = serde_json::from_slice(raw).unwrap();
    assert_eq!(record["schema"], "symeraseme.actual-go.plan-process.v1");
    assert_eq!(record["case"], case);
    assert_eq!(record["source_revision"], REVISION);
    assert_eq!(record["go_version"], "go1.26.6");
    assert_eq!(record["native_target"], target);
    assert_eq!(record["exit_status"], 0);
    assert_eq!(record["stdin"]["bytes"], 0);
    assert_eq!(record["stdin"]["sha256"], digest(&[]));
    let sources = record["source_files"].as_object().unwrap();
    assert_eq!(sources.len(), 171);
    for (name, pin) in sources {
        let path = Path::new(name);
        assert!(
            !path.is_absolute() && path.components().all(|c| matches!(c, Component::Normal(_)))
        );
        if !go_source_pin::current_tree_bound(name) {
            continue;
        }
        let bytes = go_source_pin::current_source(ROOT, name);
        assert_eq!(pin["bytes"], bytes.len(), "{name}");
        assert_eq!(pin["sha256"], digest(&bytes), "{name}");
    }
    let generators = record["archived_generators"].as_object().unwrap();
    assert_eq!(generators.len(), 3);
    for (name, pin) in generators {
        let path = Path::new(name);
        assert!(
            !path.is_absolute() && path.components().all(|c| matches!(c, Component::Normal(_)))
        );
        let archive = Command::new("git")
            .args(["show", &format!("{REVISION}:{name}")])
            .current_dir(ROOT)
            .output()
            .unwrap();
        assert!(
            archive.status.success(),
            "immutable input generator must remain verifiable"
        );
        let retired = matches!(
            name.as_str(),
            "crates/symeraseme-cli/tests/plan_execute_live_process.rs"
                | "crates/symeraseme-cli/tests/support/capture_plan_process.rs"
        );
        if retired {
            go_source_pin::assert_retired_generator_matches_archive(ROOT, name, &archive.stdout);
        }
        // These three archived generators were measured as an actual Git
        // autocrlf checkout on both Windows producers. Preserve the recorded
        // digest and reproduce only that independently verified transform.
        let bytes = if target.starts_with("windows/") {
            assert!(!archive.stdout.windows(2).any(|bytes| bytes == b"\r\n"));
            let mut bytes = Vec::with_capacity(archive.stdout.len());
            for byte in archive.stdout {
                if byte == b'\n' {
                    bytes.push(b'\r');
                }
                bytes.push(byte);
            }
            bytes
        } else {
            archive.stdout
        };
        assert_eq!(pin["bytes"], bytes.len());
        assert_eq!(pin["sha256"], digest(&bytes));
        // Retired producers were independently authenticated above. Other
        // inputs remain checkout-bound, including the strict dependency check.
        if !retired {
            let current = go_source_pin::current_source(ROOT, name);
            go_source_pin::assert_current_matches_archive(name, &bytes, &current);
        }
    }
    let build = record["embedded_build_info"].as_str().unwrap();
    let (os, arch) = target.split_once('/').unwrap();
    for expected in [
        format!("vcs.revision={REVISION}"),
        "vcs.modified=false".into(),
        format!("GOOS={os}"),
        format!("GOARCH={arch}"),
    ] {
        assert!(build.contains(&expected));
    }
    for stream in ["stdout", "stderr"] {
        let bytes = base64::engine::general_purpose::STANDARD
            .decode(record[format!("{stream}_base64")].as_str().unwrap())
            .unwrap();
        assert_eq!(record[stream]["bytes"], bytes.len());
        assert_eq!(record[stream]["sha256"], digest(&bytes));
    }
    assert_eq!(record["persisted_effects"].as_array().unwrap().len(), 3);
    record
}

pub fn observation(case: &str) -> (Output, Value) {
    assert!(
        std::env::var_os("SYMERASEME_CAPTURE_PLAN_PROCESSES").is_none(),
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
        _ => panic!("no recorded plan process for this target; live Go is retired"),
    };
    let (raw, pin) = match (target, case) {
        ("darwin/arm64", "web-form") => (include_bytes!("../../../../tests/fixtures/go-frozen/refresh-ac0b3b4b/plan-processes/darwin-arm64/web-form.json").as_slice(), "ad5a699a55a862709b9f3128e4a7bd26490b4c41713d6fc5281ccc1be3e4f9e6"),
        ("darwin/arm64", "senderless-email") => (include_bytes!("../../../../tests/fixtures/go-frozen/refresh-ac0b3b4b/plan-processes/darwin-arm64/senderless-email.json").as_slice(), "3137775992c586e4091baae699bd26bc334bceb12a29f4d3d3ac9fefe01ee69d"),
        ("darwin/amd64", "web-form") => (include_bytes!("../../../../tests/fixtures/go-frozen/refresh-ac0b3b4b/plan-processes/darwin-amd64/web-form.json").as_slice(), "2c262166a1415b5a0c83244005bf88e1033db056f575d4e8852ba993b1819076"),
        ("darwin/amd64", "senderless-email") => (include_bytes!("../../../../tests/fixtures/go-frozen/refresh-ac0b3b4b/plan-processes/darwin-amd64/senderless-email.json").as_slice(), "ff16b347e0f12e0d6a9d5e0ca5fa27b07a7a1aabb64db1a82bc282d1626b9952"),
        ("windows/amd64", "web-form") => (include_bytes!("../../../../tests/fixtures/go-frozen/refresh-ac0b3b4b/plan-processes/windows-amd64/web-form.json").as_slice(), "bfc7f220210954ff0d5047d43485790dfb7736fd59b126205e7c0efbbc3cca37"),
        ("windows/amd64", "senderless-email") => (include_bytes!("../../../../tests/fixtures/go-frozen/refresh-ac0b3b4b/plan-processes/windows-amd64/senderless-email.json").as_slice(), "81a8e67cc3d2fa0cb16fcb46080de7db216b0ba7e71485491652c50658741b9c"),
        ("windows/arm64", "web-form") => (include_bytes!("../../../../tests/fixtures/go-frozen/refresh-ac0b3b4b/plan-processes/windows-arm64/web-form.json").as_slice(), "1fa13a0dc0f5d5047c11acaf8c6ffe4f0b16ab8b9db92361a263eab8915e7628"),
        ("windows/arm64", "senderless-email") => (include_bytes!("../../../../tests/fixtures/go-frozen/refresh-ac0b3b4b/plan-processes/windows-arm64/senderless-email.json").as_slice(), "4eea33b0f86632a134a5b2c38b5885055dda67f26b3168bd9004ff22f4d6cdef"),
        ("linux/arm64", "web-form") => (include_bytes!("../../../../tests/fixtures/go-frozen/refresh-ac0b3b4b/plan-processes/linux-arm64/web-form.json").as_slice(), "500042b32aac5e19d41150263db84b506a5d7d1983b7beddbe8e958742aa551f"),
        ("linux/arm64", "senderless-email") => (include_bytes!("../../../../tests/fixtures/go-frozen/refresh-ac0b3b4b/plan-processes/linux-arm64/senderless-email.json").as_slice(), "22b55272a935f9223565309f3c0e2a6dfc9982b572564f0aec66f6d9f2324097"),
        ("linux/amd64", "web-form") => (include_bytes!("../../../../tests/fixtures/go-frozen/refresh-ac0b3b4b/plan-processes/linux-amd64/web-form.json").as_slice(), "3ef286e96de88afd7fe900b87c471601f494dc34d9e1291961cc6ca0351b217c"),
        ("linux/amd64", "senderless-email") => (include_bytes!("../../../../tests/fixtures/go-frozen/refresh-ac0b3b4b/plan-processes/linux-amd64/senderless-email.json").as_slice(), "560657180a2edc3685d31bdf91bdadaac251a2879406ad102c76cf054d8c8e71"),
        _ => panic!("unrecorded plan process cannot receive a cached answer"),
    };
    let record = verify(raw, pin, case, target);
    let mut changed = raw.to_vec();
    changed.push(b'!');
    assert!(std::panic::catch_unwind(|| verify(&changed, pin, case, target)).is_err());
    assert!(std::panic::catch_unwind(|| verify(&raw[..raw.len() - 1], pin, case, target)).is_err());
    let mut wrong_effects = record.clone();
    wrong_effects["persisted_effects"][0] = "fabricated-state".into();
    assert!(
        std::panic::catch_unwind(|| verify(
            &serde_json::to_vec(&wrong_effects).unwrap(),
            pin,
            case,
            target
        ))
        .is_err()
    );
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
    let output = Output {
        status,
        stdout: base64::engine::general_purpose::STANDARD
            .decode(record["stdout_base64"].as_str().unwrap())
            .unwrap(),
        stderr: base64::engine::general_purpose::STANDARD
            .decode(record["stderr_base64"].as_str().unwrap())
            .unwrap(),
    };
    (output, record["persisted_effects"].clone())
}

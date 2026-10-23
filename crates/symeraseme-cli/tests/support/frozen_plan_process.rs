//! Complete actual native Linux/Windows Go plan processes and persisted effects.
use base64::Engine;
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::path::{Component, Path};
use std::process::{Command, Output};

#[path = "go_source_pin.rs"]
mod go_source_pin;

const ROOT: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../..");
const REVISION: &str = "e2cf176337f6f90e64f76b5f39d3a859e87774ea";

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
        ("darwin/arm64", "web-form") => (include_bytes!("../../../../tests/fixtures/go-frozen/refresh-e2cf1763/plan-processes/darwin-arm64/web-form.json").as_slice(), "a841416bfc862eddd2490a136339925e7e96916b2e2576e7a9848d9ecf760d3f"),
        ("darwin/arm64", "senderless-email") => (include_bytes!("../../../../tests/fixtures/go-frozen/refresh-e2cf1763/plan-processes/darwin-arm64/senderless-email.json").as_slice(), "968d61da2b8f6762cbe044ec03299d5dd3eaf4a52b0829fd91f1c493aa043809"),
        ("darwin/amd64", "web-form") => (include_bytes!("../../../../tests/fixtures/go-frozen/refresh-e2cf1763/plan-processes/darwin-amd64/web-form.json").as_slice(), "f3ecfc266056f84316df8f53f0938c5ad5770720e20186515d00db11aef7a918"),
        ("darwin/amd64", "senderless-email") => (include_bytes!("../../../../tests/fixtures/go-frozen/refresh-e2cf1763/plan-processes/darwin-amd64/senderless-email.json").as_slice(), "6fcf8be5831f2ad553e70fc8b73eced387d3f54d4dcccb6f34808162d286407c"),
        ("windows/amd64", "web-form") => (include_bytes!("../../../../tests/fixtures/go-frozen/refresh-e2cf1763/plan-processes/windows-amd64/web-form.json").as_slice(), "c51fb91266d11b7af72e261f37ffe26b5cc8732670351c0371447b9c576a97c2"),
        ("windows/amd64", "senderless-email") => (include_bytes!("../../../../tests/fixtures/go-frozen/refresh-e2cf1763/plan-processes/windows-amd64/senderless-email.json").as_slice(), "f65a764a4fbe664dc0ac5a7e37043c4b9a4aa000097b3d4c6f15190ad79ad017"),
        ("windows/arm64", "web-form") => (include_bytes!("../../../../tests/fixtures/go-frozen/refresh-e2cf1763/plan-processes/windows-arm64/web-form.json").as_slice(), "a40c487d0968ba2fe3f8764de88ef2364cb49c3cf59db58eba9c335a3bd97c0d"),
        ("windows/arm64", "senderless-email") => (include_bytes!("../../../../tests/fixtures/go-frozen/refresh-e2cf1763/plan-processes/windows-arm64/senderless-email.json").as_slice(), "d890c4632cd669ca5dbec76981928a1a1f1bf9c0f7299cf3866b16db8fd9d4f5"),
        ("linux/arm64", "web-form") => (include_bytes!("../../../../tests/fixtures/go-frozen/refresh-e2cf1763/plan-processes/linux-arm64/web-form.json").as_slice(), "43a63548ccf9f3243610e751fba841d0e199eb97d79297d4200a6772eab8769a"),
        ("linux/arm64", "senderless-email") => (include_bytes!("../../../../tests/fixtures/go-frozen/refresh-e2cf1763/plan-processes/linux-arm64/senderless-email.json").as_slice(), "e4bd4020ed858382020a297af5987734e31346f9b0e223764c4e525214b8deb9"),
        ("linux/amd64", "web-form") => (include_bytes!("../../../../tests/fixtures/go-frozen/refresh-e2cf1763/plan-processes/linux-amd64/web-form.json").as_slice(), "8f6a3b3b72fbec6cf938d8f8ead90a14bca2f5461db78123a6125b8d0c7a6036"),
        ("linux/amd64", "senderless-email") => (include_bytes!("../../../../tests/fixtures/go-frozen/refresh-e2cf1763/plan-processes/linux-amd64/senderless-email.json").as_slice(), "83f0d8d64826242a866ab4e7c25e2d6c17193509be76f357b326134c8576e0c5"),
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

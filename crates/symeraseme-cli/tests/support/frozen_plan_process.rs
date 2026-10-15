//! Complete actual native Linux/Windows Go plan processes and persisted effects.
use base64::Engine;
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::path::{Component, Path};
use std::process::{Command, Output};

#[path = "go_source_pin.rs"]
mod go_source_pin;

const ROOT: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../..");
const REVISION: &str = "968e1a2dda9c621ba880e7b618dc49daa31970b3";

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
        ("darwin/arm64", "web-form") => (include_bytes!("../../../../tests/fixtures/go-frozen/refresh-968e1a2d/plan-processes/darwin-arm64/web-form.json").as_slice(), "8c9f8aefa7bd8b3baa36ef308c09355ce979074c1321c0d6a69f7d29d3ed49a8"),
        ("darwin/arm64", "senderless-email") => (include_bytes!("../../../../tests/fixtures/go-frozen/refresh-968e1a2d/plan-processes/darwin-arm64/senderless-email.json").as_slice(), "4353afdafefc230f3623cf2469796e9d054ef694355a87ed1992579e5e28f202"),
        ("darwin/amd64", "web-form") => (include_bytes!("../../../../tests/fixtures/go-frozen/refresh-968e1a2d/plan-processes/darwin-amd64/web-form.json").as_slice(), "54728c14ed15dd681674bcfa946f40bbe3963544429ba858198ffc910dcb9cf5"),
        ("darwin/amd64", "senderless-email") => (include_bytes!("../../../../tests/fixtures/go-frozen/refresh-968e1a2d/plan-processes/darwin-amd64/senderless-email.json").as_slice(), "fa02291fdb840e74e9280eb597f39aa64e6c20ddcfc1b77cc8e72d02c8da55d3"),
        ("windows/amd64", "web-form") => (include_bytes!("../../../../tests/fixtures/go-frozen/refresh-968e1a2d/plan-processes/windows-amd64/web-form.json").as_slice(), "b4a07f336c8c1f48404db70b0b9e12aad68b82ee7266b4a0f79b5cecd837c12a"),
        ("windows/amd64", "senderless-email") => (include_bytes!("../../../../tests/fixtures/go-frozen/refresh-968e1a2d/plan-processes/windows-amd64/senderless-email.json").as_slice(), "afd639ce3fdf0e79dd76ec9c21635643e6378137a5e217235dce0b51b6187bec"),
        ("windows/arm64", "web-form") => (include_bytes!("../../../../tests/fixtures/go-frozen/refresh-968e1a2d/plan-processes/windows-arm64/web-form.json").as_slice(), "16375420637aac9faf1049aafaccae33ce93ff75a0071891e059c07130034888"),
        ("windows/arm64", "senderless-email") => (include_bytes!("../../../../tests/fixtures/go-frozen/refresh-968e1a2d/plan-processes/windows-arm64/senderless-email.json").as_slice(), "42c13c5f8f6053c7ed55b4b22a605461730de47d77053af60aa13e7c2d05c6e5"),
        ("linux/arm64", "web-form") => (include_bytes!("../../../../tests/fixtures/go-frozen/refresh-968e1a2d/plan-processes/linux-arm64/web-form.json").as_slice(), "f27d29a9d37094351553b1e3df1b27b8f4c8d98a530e6627fd4a1788d9b6d45f"),
        ("linux/arm64", "senderless-email") => (include_bytes!("../../../../tests/fixtures/go-frozen/refresh-968e1a2d/plan-processes/linux-arm64/senderless-email.json").as_slice(), "0d62580098f4c297441e3468ba63ed5eb1bd24e705cc199a6fa182a8b56d9127"),
        ("linux/amd64", "web-form") => (include_bytes!("../../../../tests/fixtures/go-frozen/refresh-968e1a2d/plan-processes/linux-amd64/web-form.json").as_slice(), "0c1e03e33a9ac12bcd6e6a024821398d5423124387a69d750234be1fa83241d9"),
        ("linux/amd64", "senderless-email") => (include_bytes!("../../../../tests/fixtures/go-frozen/refresh-968e1a2d/plan-processes/linux-amd64/senderless-email.json").as_slice(), "1544a4e7bc1d5fb40eda101b3e6aa501015d5c041e2bf67bafbf57d148e4b344"),
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

//! Complete actual native Linux/Windows Go plan processes and persisted effects.
use base64::Engine;
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::path::{Component, Path};
use std::process::{Command, Output};

#[path = "go_source_pin.rs"]
mod go_source_pin;

const ROOT: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../..");
const REVISION: &str = "e8bb6643cbc2a05dbc19f3ad3749513887095ac9";

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
        let bytes = std::fs::read(Path::new(ROOT).join(path)).unwrap();
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
        // A changed test input generator or dependency lock requires actual
        // recapture, even if its older immutable archive remains available.
        let current = std::fs::read(Path::new(ROOT).join(path)).unwrap();
        assert_eq!(pin["bytes"], current.len(), "current plan input: {name}");
        assert_eq!(
            pin["sha256"],
            digest(&current),
            "current plan input: {name}"
        );
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

pub fn observation(case: &str) -> Option<(Output, Value)> {
    if std::env::var_os("SYMERASEME_CAPTURE_PLAN_PROCESSES").is_some_and(|value| !value.is_empty())
    {
        return None; // Explicit native recording always executes actual Go.
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
        _ => return None, // Unrecorded hosts still execute actual Go.
    };
    let (raw, pin) = match (target, case) {
        ("darwin/arm64", "web-form") => (include_bytes!("../../../../tests/fixtures/go-frozen/plan-process-native097/darwin-arm64/web-form.json").as_slice(), "3135eec6189914f38421e778e1b9bb72e581f217a8fa742dcfc6b2fb8ae3be7c"),
        ("darwin/arm64", "senderless-email") => (include_bytes!("../../../../tests/fixtures/go-frozen/plan-process-native097/darwin-arm64/senderless-email.json").as_slice(), "0605eb4f5dcf3de37b2a9d04e8ed4f6dd978f441cf24d47334f5031fb505931c"),
        ("darwin/amd64", "web-form") => (include_bytes!("../../../../tests/fixtures/go-frozen/plan-process-native097/darwin-amd64/web-form.json").as_slice(), "91c7416dc8ae9c4786c6ecec066d785c360eb8b7a90624bad138b66f9acf3832"),
        ("darwin/amd64", "senderless-email") => (include_bytes!("../../../../tests/fixtures/go-frozen/plan-process-native097/darwin-amd64/senderless-email.json").as_slice(), "5187679086a3a54b59db33715fbb42947bf6b50fb59428f30fd090fa95129989"),
        ("windows/amd64", "web-form") => (include_bytes!("../../../../tests/fixtures/go-frozen/plan-process-native097/windows-amd64/web-form.json").as_slice(), "0a7548346f6f0fc2fe3ae18ab9d666bd8f1e4b51f8dbcd8878fe19dbfe5d7230"),
        ("windows/amd64", "senderless-email") => (include_bytes!("../../../../tests/fixtures/go-frozen/plan-process-native097/windows-amd64/senderless-email.json").as_slice(), "3bacfe64bbaa9b775154645fe5b7dbb1e3c4108f22b1049748815bbc2d918ee9"),
        ("windows/arm64", "web-form") => (include_bytes!("../../../../tests/fixtures/go-frozen/plan-process-native097/windows-arm64/web-form.json").as_slice(), "903374b559180ba400ee857340ab3540a18273fdfba8b0875844a2725aa6ee02"),
        ("windows/arm64", "senderless-email") => (include_bytes!("../../../../tests/fixtures/go-frozen/plan-process-native097/windows-arm64/senderless-email.json").as_slice(), "516f3b5fc689d29c060bd2610e6700f8405f00b8169e4637a176076b5e6569a9"),
        ("linux/arm64", "web-form") => (include_bytes!("../../../../tests/fixtures/go-frozen/plan-process-native097/linux-arm64/web-form.json").as_slice(), "09561d363744719edf9ae71b4df644dd3ec354621290a1df9a4e5d2080f42431"),
        ("linux/arm64", "senderless-email") => (include_bytes!("../../../../tests/fixtures/go-frozen/plan-process-native097/linux-arm64/senderless-email.json").as_slice(), "46ade87bdea026423af0b14a51e2669eea677ac10ba3190e55893282491d218f"),
        ("linux/amd64", "web-form") => (include_bytes!("../../../../tests/fixtures/go-frozen/plan-process-native097/linux-amd64/web-form.json").as_slice(), "2b4e9a7a1b18fd860c50d0e50d8f13ab123439d33f3f029a0b22484ef7bcb4b2"),
        ("linux/amd64", "senderless-email") => (include_bytes!("../../../../tests/fixtures/go-frozen/plan-process-native097/linux-amd64/senderless-email.json").as_slice(), "ab97bc882040434d06cecb94965e1971a0c4513c16ffb9e858a92ebcbc4cc87c"),
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
    Some((output, record["persisted_effects"].clone()))
}

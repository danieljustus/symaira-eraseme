//! Actual complete Linux-amd64 Go plan process and original persisted effects.
use base64::Engine;
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::path::{Component, Path};
use std::process::{Command, Output};

const ROOT: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../..");
const REVISION: &str = "fae7b5da416db5e491ed3b4d1944069d3f80d291";

fn digest(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

fn verify(raw: &[u8], expected: &str, case: &str) -> Value {
    assert_eq!(digest(raw), expected, "whole actual process record");
    let record: Value = serde_json::from_slice(raw).unwrap();
    assert_eq!(record["schema"], "symeraseme.actual-go.plan-process.v1");
    assert_eq!(record["case"], case);
    assert_eq!(record["source_revision"], REVISION);
    assert_eq!(record["go_version"], "go1.26.6");
    assert_eq!(record["native_target"], "linux/amd64");
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
        assert_eq!(pin["bytes"], archive.stdout.len());
        assert_eq!(pin["sha256"], digest(&archive.stdout));
    }
    let build = record["embedded_build_info"].as_str().unwrap();
    for expected in [
        format!("vcs.revision={REVISION}"),
        "vcs.modified=false".into(),
        "GOOS=linux".into(),
        "GOARCH=amd64".into(),
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
        Ok("0") | Err(std::env::VarError::NotPresent) => {
            if !cfg!(all(target_os = "linux", target_arch = "x86_64")) {
                return None;
            }
        }
        _ => panic!("SYMERASEME_PARITY_LIVE_GO must be 0 or 1"),
    }
    let (raw, pin) = match case {
        "web-form" => (
            include_bytes!("../../../../tests/fixtures/go-frozen/plan-process-fae7/web-form.json")
                .as_slice(),
            "6927b253e89d9bc680ead7e04108e09fedcd41502219efe7a6c6854f86580f95",
        ),
        "senderless-email" => (
            include_bytes!(
                "../../../../tests/fixtures/go-frozen/plan-process-fae7/senderless-email.json"
            )
            .as_slice(),
            "03de24f57a8ecb4ebfb5d520059fe0e357ad1cac80b3ff3521b38da32dab59d4",
        ),
        _ => panic!("unrecorded plan process cannot receive a cached answer"),
    };
    let record = verify(raw, pin, case);
    let mut changed = raw.to_vec();
    changed.push(b'!');
    assert!(std::panic::catch_unwind(|| verify(&changed, pin, case)).is_err());
    assert!(std::panic::catch_unwind(|| verify(&raw[..raw.len() - 1], pin, case)).is_err());
    let mut wrong_effects = record.clone();
    wrong_effects["persisted_effects"][0] = "fabricated-state".into();
    assert!(
        std::panic::catch_unwind(|| verify(
            &serde_json::to_vec(&wrong_effects).unwrap(),
            pin,
            case
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

//! Complete actual native Go plan processes and persisted effects (macOS arm64, Linux).
use base64::Engine;
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::path::{Component, Path};
use std::process::{Command, Output};

#[path = "go_source_pin.rs"]
mod go_source_pin;

const ROOT: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../..");
const REVISION: &str = "fc446060e4346a5896b2c80f5788a5a92be58992";

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
        let bytes = archive.stdout;
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
        ("macos", "aarch64") => "darwin/arm64",
        _ => panic!("no recorded plan process for this target; live Go is retired"),
    };
    let (raw, pin) = match (target, case) {
        ("darwin/arm64", "web-form") => (include_bytes!("../../../../tests/fixtures/go-frozen/refresh-fc446060/plan-processes/darwin-arm64/web-form.json").as_slice(), "f828878c4c8cfe276ac80c14304b32519a04df33396db6669e6aff0505e8c0c6"),
        ("darwin/arm64", "senderless-email") => (include_bytes!("../../../../tests/fixtures/go-frozen/refresh-fc446060/plan-processes/darwin-arm64/senderless-email.json").as_slice(), "569d5ac80bb9f4ab77044107c3547849e2ccc45fba17c854b6f0303e7df11b56"),
        ("linux/arm64", "web-form") => (include_bytes!("../../../../tests/fixtures/go-frozen/refresh-fc446060/plan-processes/linux-arm64/web-form.json").as_slice(), "d16d7e5d74c39e7491dc2634ab831b6740e0c30175bd3f0dc4a87eb0e297bc44"),
        ("linux/arm64", "senderless-email") => (include_bytes!("../../../../tests/fixtures/go-frozen/refresh-fc446060/plan-processes/linux-arm64/senderless-email.json").as_slice(), "9e7593a46c82c827296dc59150645b7ccd2ddb45f58dbbede355030947053450"),
        ("linux/amd64", "web-form") => (include_bytes!("../../../../tests/fixtures/go-frozen/refresh-fc446060/plan-processes/linux-amd64/web-form.json").as_slice(), "bf2ee0f4354929ccb31fdcee6c212ba0921cd54000b5d7a63630acc4cdd739bf"),
        ("linux/amd64", "senderless-email") => (include_bytes!("../../../../tests/fixtures/go-frozen/refresh-fc446060/plan-processes/linux-amd64/senderless-email.json").as_slice(), "5d10e07812585ca7e9c0510cf8ffd526e1bc6cf48b35dbd370508038c16b6369"),
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
    let status = {
        use std::os::unix::process::ExitStatusExt;
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

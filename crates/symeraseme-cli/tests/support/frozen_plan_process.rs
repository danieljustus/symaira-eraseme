//! Complete actual native Linux/Windows Go plan processes and persisted effects.
use base64::Engine;
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::path::{Component, Path};
use std::process::{Command, Output};

#[path = "go_source_pin.rs"]
mod go_source_pin;

const ROOT: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../..");
const REVISION: &str = "433622f6e6c884694a304f483ae481f93069b455";

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
        ("darwin/arm64", "web-form") => (include_bytes!("../../../../tests/fixtures/go-frozen/refresh-433622f6/plan-processes/darwin-arm64/web-form.json").as_slice(), "a0f1ed8ffea1759f6af5ce086f1db31e4b53bc6e71dabfd44d48b76ddd97a8c6"),
        ("darwin/arm64", "senderless-email") => (include_bytes!("../../../../tests/fixtures/go-frozen/refresh-433622f6/plan-processes/darwin-arm64/senderless-email.json").as_slice(), "6ad91859e185a5fdf95d5157a536141f3226c453c7f20f0808a0f8671fabfbdb"),
        ("darwin/amd64", "web-form") => (include_bytes!("../../../../tests/fixtures/go-frozen/refresh-433622f6/plan-processes/darwin-amd64/web-form.json").as_slice(), "efb491f77fc33eaf7b86cfeb5a83e5b303eebe1ffa3f2f927ad58085d28dcdcd"),
        ("darwin/amd64", "senderless-email") => (include_bytes!("../../../../tests/fixtures/go-frozen/refresh-433622f6/plan-processes/darwin-amd64/senderless-email.json").as_slice(), "1e925d45edac05a291c98869d7a3c9bd1b3c9e3582bd179e7305cd723eaa328a"),
        ("windows/amd64", "web-form") => (include_bytes!("../../../../tests/fixtures/go-frozen/refresh-433622f6/plan-processes/windows-amd64/web-form.json").as_slice(), "f26d563bfebe97e535c47c4f4e7f8e7129b37db327485d740235ffdf8ae8afa0"),
        ("windows/amd64", "senderless-email") => (include_bytes!("../../../../tests/fixtures/go-frozen/refresh-433622f6/plan-processes/windows-amd64/senderless-email.json").as_slice(), "e646db81750dc3181fbfe6ce6b756ab6f54152cd12f99cda5bd01902ab33f00c"),
        ("windows/arm64", "web-form") => (include_bytes!("../../../../tests/fixtures/go-frozen/refresh-433622f6/plan-processes/windows-arm64/web-form.json").as_slice(), "6cba7e0eac77b893b92691c24fa6de143cccd0fad245c7cfa6781115968c674a"),
        ("windows/arm64", "senderless-email") => (include_bytes!("../../../../tests/fixtures/go-frozen/refresh-433622f6/plan-processes/windows-arm64/senderless-email.json").as_slice(), "82530c0c7b54b1ffc149726ba4cb940fce1db8bf5cfd1e9afee58c9ceeb20eca"),
        ("linux/arm64", "web-form") => (include_bytes!("../../../../tests/fixtures/go-frozen/refresh-433622f6/plan-processes/linux-arm64/web-form.json").as_slice(), "8587f08f062eef2468f9c96789844743fc9c510969b372067fa614fdff60a0d0"),
        ("linux/arm64", "senderless-email") => (include_bytes!("../../../../tests/fixtures/go-frozen/refresh-433622f6/plan-processes/linux-arm64/senderless-email.json").as_slice(), "508179bba80d376000825195863ac18c45bdbddb8b57dbf346e3e35d01e1622a"),
        ("linux/amd64", "web-form") => (include_bytes!("../../../../tests/fixtures/go-frozen/refresh-433622f6/plan-processes/linux-amd64/web-form.json").as_slice(), "33437c5d73b981a1d06baf111aceb4e33e7139c81c56ea84ec2c45d80783a252"),
        ("linux/amd64", "senderless-email") => (include_bytes!("../../../../tests/fixtures/go-frozen/refresh-433622f6/plan-processes/linux-amd64/senderless-email.json").as_slice(), "46e12d6167acb09df81a52470d43a90248e005e36e333ab4ee476df922903809"),
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

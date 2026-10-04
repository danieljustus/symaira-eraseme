#[path = "../../../symeraseme-core/tests/support/frozen_native_capture.rs"]
mod frozen_native_capture;

use serde_json::Value;
use sha2::{Digest, Sha256};
use std::path::Path;

const ROOT: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../..");

macro_rules! streams {
    ($case:literal) => {
        (
            include_bytes!(concat!(
                "../../../../tests/fixtures/go-frozen/cli-review/",
                $case,
                ".stdout"
            ))
            .as_slice(),
            include_bytes!(concat!(
                "../../../../tests/fixtures/go-frozen/cli-review/",
                $case,
                ".stderr"
            ))
            .as_slice(),
        )
    };
}

pub fn live_required() -> bool {
    match std::env::var("SYMERASEME_PARITY_LIVE_GO").as_deref() {
        Ok("1") => true,
        Ok("0") | Err(std::env::VarError::NotPresent) => !cfg!(target_os = "linux"),
        _ => panic!("SYMERASEME_PARITY_LIVE_GO must be 0 or 1"),
    }
}

// Review has measured native records on all six targets. Grant remains live
// outside Linux until its separate native capture is available.
pub fn live_review_required() -> bool {
    match std::env::var("SYMERASEME_PARITY_LIVE_GO").as_deref() {
        Ok("1") => true,
        Ok("0") | Err(std::env::VarError::NotPresent) => {
            frozen_native_capture::manifest_bytes(std::env::consts::OS, std::env::consts::ARCH)
                .is_none()
        }
        _ => panic!("SYMERASEME_PARITY_LIVE_GO must be 0 or 1"),
    }
}

pub fn digest(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

pub fn valid_bytes(record: &Value, bytes: &[u8]) -> bool {
    record["bytes"] == bytes.len() && record["sha256"] == digest(bytes)
}

pub struct Observation {
    pub argv: Vec<String>,
    pub status: i32,
    pub stdout: &'static [u8],
    pub stderr: &'static [u8],
}

pub fn observations() -> Vec<Observation> {
    let native = frozen_native_capture::verify(
        frozen_native_capture::manifest_bytes(std::env::consts::OS, std::env::consts::ARCH)
            .expect("unrecorded native review target must use the actual Go producer"),
        std::env::consts::OS,
        std::env::consts::ARCH,
    );
    assert_eq!(native["cli_review"]["input_unchanged"], true);
    assert_eq!(
        native["cli_review"]["input_sha256"],
        digest(b"Alice Example <alice@example.invalid>\n")
    );
    let manifest: Value = serde_json::from_slice(include_bytes!(
        "../../../../tests/fixtures/go-frozen/cli-review/manifest.json"
    ))
    .unwrap();
    assert_eq!(
        manifest["source_revision"],
        "46f2aec331a49b1d2e6423f15763b67c0fda7eba"
    );
    assert_eq!(manifest["go_version"], "go1.26.6");
    assert_eq!(manifest["native_target"], "linux/amd64");
    let build_info = manifest["binary_build_info"].as_str().unwrap();
    assert!(build_info.contains("vcs.revision=46f2aec331a49b1d2e6423f15763b67c0fda7eba"));
    assert!(build_info.contains("vcs.modified=false"));
    assert_eq!(manifest["build"]["exit_status"], 0);
    let (build_out, build_err) = streams!("build");
    assert!(valid_bytes(&manifest["build"]["stdout"], build_out));
    assert!(valid_bytes(&manifest["build"]["stderr"], build_err));
    assert!(build_err.is_empty());
    assert_eq!(manifest["input_unchanged"], true);
    assert_eq!(
        manifest["input_sha256"],
        digest(b"Alice Example <alice@example.invalid>\n")
    );
    let sources = manifest["source_files"].as_object().unwrap();
    assert_eq!(sources.len(), 171);
    for (name, expected) in sources {
        let bytes = std::fs::read(Path::new(ROOT).join(name)).unwrap();
        assert_eq!(expected, &digest(&bytes), "Go source drift: {name}");
    }
    let streams = [
        streams!("case-0"),
        streams!("case-1"),
        streams!("case-2"),
        streams!("case-3"),
        streams!("case-4"),
        streams!("case-5"),
        streams!("case-6"),
        streams!("case-7"),
    ];
    let cases = manifest["cases"].as_array().unwrap();
    assert_eq!(cases.len(), streams.len());
    let observations: Vec<Observation> = cases
        .iter()
        .zip(streams)
        .map(|(case, (stdout, stderr))| {
            assert!(valid_bytes(&case["stdout"], stdout));
            assert!(valid_bytes(&case["stderr"], stderr));
            Observation {
                argv: serde_json::from_value(case["argv"].clone()).unwrap(),
                status: case["exit_status"].as_i64().unwrap().try_into().unwrap(),
                stdout,
                stderr,
            }
        })
        .collect();
    verify_native_cases(&native, &observations);
    observations
}

fn verify_native_cases(native: &Value, observations: &[Observation]) {
    let cases = native["cli_review"]["cases"].as_array().unwrap();
    assert_eq!(cases.len(), observations.len());
    for (case, observed) in cases.iter().zip(observations) {
        assert_eq!(case["argv"], serde_json::to_value(&observed.argv).unwrap());
        assert_eq!(case["exit_status"], observed.status);
        assert!(valid_bytes(&case["stdout"], observed.stdout));
        assert!(valid_bytes(&case["stderr"], observed.stderr));
    }
}

pub fn verify_all_native_records_and_reject_changed_frames() {
    let observations = observations();
    for (os, arch) in [
        ("linux", "x86_64"),
        ("linux", "aarch64"),
        ("windows", "x86_64"),
        ("windows", "aarch64"),
        ("macos", "x86_64"),
        ("macos", "aarch64"),
    ] {
        let native = frozen_native_capture::verify(
            frozen_native_capture::manifest_bytes(os, arch).unwrap(),
            os,
            arch,
        );
        verify_native_cases(&native, &observations);
        let mut changed = native;
        changed["cli_review"]["cases"][0]["exit_status"] = 0.into();
        assert!(std::panic::catch_unwind(|| verify_native_cases(&changed, &observations)).is_err());
    }
}

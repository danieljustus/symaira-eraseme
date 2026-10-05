#![allow(dead_code)] // Included separately by the execution and plan integrations.

#[path = "frozen_native_capture.rs"]
mod frozen_native_capture;

use serde_json::Value;
use sha2::{Digest, Sha256};

pub const EXECUTION: &[u8] =
    include_bytes!("../../../../tests/fixtures/go-frozen/campaign/campaign-execution.stdout");

pub fn live_go_required() -> bool {
    match std::env::var("SYMERASEME_PARITY_LIVE_GO").as_deref() {
        Ok("1") => true,
        Ok("0") | Err(std::env::VarError::NotPresent) => {
            frozen_native_capture::manifest_bytes(std::env::consts::OS, std::env::consts::ARCH)
                .is_none()
        }
        _ => panic!("SYMERASEME_PARITY_LIVE_GO must be 0 or 1"),
    }
}

fn manifest() -> Value {
    let manifest: Value = serde_json::from_slice(include_bytes!(
        "../../../../tests/fixtures/go-frozen/campaign/manifest.json"
    ))
    .unwrap();
    assert_eq!(
        manifest["source_revision"],
        "30eeb38f1e43c8f633d3537818d1de8b96ba9d6a"
    );
    assert_eq!(manifest["go_version"], "go version go1.26.6 linux/amd64");
    assert_eq!(manifest["native_target"], "linux/amd64");
    manifest
}

fn verify_bytes(metadata: &Value, bytes: &[u8]) {
    assert_eq!(metadata["bytes"], bytes.len());
    assert_eq!(metadata["sha256"], hex::encode(Sha256::digest(bytes)));
}

pub fn execution(bytes: &[u8]) -> Value {
    let manifest = manifest();
    let native = native_capture();
    let observed = native["observations"]
        .as_array()
        .unwrap()
        .iter()
        .find(|case| case["package"] == "campaign-execution")
        .unwrap();
    verify_bytes(&observed["stdout"], bytes);
    assert_eq!(observed["exit_status"], 0);
    assert_eq!(observed["stderr"]["bytes"], 0);
    let capture = &manifest["execution"];
    assert_eq!(capture["exit_status"], 0);
    verify_bytes(&capture["stdout"], bytes);
    let stderr =
        include_bytes!("../../../../tests/fixtures/go-frozen/campaign/campaign-execution.stderr");
    assert!(stderr.is_empty());
    verify_bytes(&capture["stderr"], stderr);
    let document: Value = serde_json::from_slice(bytes).unwrap();
    let mut fields = document
        .as_object()
        .unwrap()
        .keys()
        .map(String::as_str)
        .collect::<Vec<_>>();
    fields.sort_unstable();
    assert_eq!(serde_json::to_value(&fields).unwrap(), capture["fields"]);
    assert_eq!(fields.len(), 9);
    assert_eq!(document["events"].as_array().unwrap().len(), 4);
    assert_eq!(document["fake_send_events"].as_array().unwrap().len(), 4);
    document
}

pub fn verify_plan_capture(fixture: &[u8]) {
    let manifest = manifest();
    let native = native_capture();
    let observed = native["controls"]
        .as_array()
        .unwrap()
        .iter()
        .find(|case| case["package"] == "campaign-plan-go-test")
        .unwrap();
    assert_eq!(observed["exit_status"], 0);
    assert_eq!(observed["stderr"]["bytes"], 0);
    assert_eq!(
        observed["executed_pass_tests"],
        serde_json::json!(["TestCampaignPlanBytesOracle"])
    );
    verify_bytes(&observed["fixture"], fixture);
    let capture = &manifest["plan_bytes"];
    assert_eq!(capture["exit_status"], 0);
    verify_bytes(&capture["fixture"], fixture);
    let stdout = include_bytes!(
        "../../../../tests/fixtures/go-frozen/campaign/campaign-plan-go-test.stdout"
    );
    let stderr = include_bytes!(
        "../../../../tests/fixtures/go-frozen/campaign/campaign-plan-go-test.stderr"
    );
    assert!(stderr.is_empty());
    verify_bytes(&capture["stdout"], stdout);
    verify_bytes(&capture["stderr"], stderr);
    let records = stdout
        .split(|byte| *byte == b'\n')
        .filter(|line| !line.is_empty())
        .map(|line| serde_json::from_slice::<Value>(line).unwrap())
        .collect::<Vec<_>>();
    assert!(!records.iter().any(|record| record["Action"] == "fail"));
    let passes = records
        .iter()
        .filter(|record| {
            record["Action"] == "pass"
                && record["Test"] == "TestCampaignPlanBytesOracle"
                && record["Package"] == "github.com/danieljustus/symaira-eraseme/internal/campaign"
        })
        .count();
    assert_eq!(passes, 1);
    assert_eq!(capture["executed_test_pass_records"], passes);
}

fn native_capture() -> Value {
    frozen_native_capture::verify(
        frozen_native_capture::manifest_bytes(std::env::consts::OS, std::env::consts::ARCH)
            .expect("unrecorded native campaign target must use the actual Go producer"),
        std::env::consts::OS,
        std::env::consts::ARCH,
    )
}

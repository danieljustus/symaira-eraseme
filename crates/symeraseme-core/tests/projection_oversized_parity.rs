//! Frozen native Go-oracle parity for oversized `expected_response_days` payloads.
//!
//! Go evaluates `time.Duration(days) * 24 * time.Hour` with signed int64
//! nanosecond arithmetic, so values at the duration boundary and at the int64
//! payload limits wrap instead of being rejected. The committed DB-004 fixture
//! only covers ordinary fractional and negative values, so this test pins the
//! boundary behaviour against actual native Go observations, with explicit live mode.

#[path = "support/go_oracle.rs"]
mod go_oracle;

#[path = "support/frozen_native_capture.rs"]
mod frozen_native_capture;

use chrono::{TimeZone, Utc};
use serde::Deserialize;
use serde_json::{Map, Value};
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use symeraseme_core::storage::{EventRecord, EventType, ProjectionState, Source, fold_events};

const CASES: &str = include_str!("../../../rust-tests/parity/oracle/projection/cases.json");

#[derive(Debug, Deserialize)]
struct Case {
    name: String,
    expected_response_days: Value,
}

#[derive(Debug, Deserialize)]
struct OracleOutput {
    cases: BTreeMap<String, ProjectionState>,
}

fn run_go_oracle() -> OracleOutput {
    match std::env::var("SYMERASEME_PARITY_LIVE_GO").as_deref() {
        Ok("1") => {}
        Ok("0") | Err(std::env::VarError::NotPresent) => {
            return frozen_output(frozen_bytes());
        }
        _ => panic!("SYMERASEME_PARITY_LIVE_GO must be 0 or 1"),
    }
    let run = go_oracle::run_oracle("projection", None);
    assert!(
        run.status.success(),
        "Go projection oracle failed: {}",
        String::from_utf8_lossy(&run.stderr)
    );
    serde_json::from_slice(&run.stdout).expect("Go projection oracle must emit JSON")
}

fn frozen_bytes() -> &'static [u8] {
    match std::env::consts::ARCH {
        "x86_64" => include_bytes!("../../../tests/fixtures/go-frozen/projection/amd64.stdout"),
        "aarch64" => include_bytes!("../../../tests/fixtures/go-frozen/projection/arm64.stdout"),
        other => panic!("projection freeze has no actual Go observation for {other}"),
    }
}

fn frozen_output(bytes: &[u8]) -> OracleOutput {
    verify_native_output(bytes, std::env::consts::OS, std::env::consts::ARCH);
    let (raw_manifest, target) = match std::env::consts::ARCH {
        "x86_64" => (
            include_bytes!(
                "../../../tests/fixtures/go-frozen/projection/linux-amd64.manifest.json"
            )
            .as_slice(),
            "linux/amd64",
        ),
        "aarch64" => (
            include_bytes!(
                "../../../tests/fixtures/go-frozen/projection/linux-arm64.manifest.json"
            )
            .as_slice(),
            "linux/arm64",
        ),
        other => panic!("projection freeze has no actual Go observation for {other}"),
    };
    let manifest: Value = serde_json::from_slice(raw_manifest).unwrap();
    assert_eq!(
        manifest["source_revision"],
        "6ab95372e69e6346d3929f709f9403321c642067"
    );
    assert_eq!(manifest["go_version"], "go1.26.6");
    assert_eq!(manifest["native_target"], target);
    for (path, source) in [
        (
            "rust-tests/parity/oracle/projection/cases.json",
            CASES.as_bytes(),
        ),
        (
            "rust-tests/parity/oracle/projection/main.go",
            include_bytes!("../../../rust-tests/parity/oracle/projection/main.go").as_slice(),
        ),
        (
            "internal/eventstore/projection.go",
            include_bytes!("../../../internal/eventstore/projection.go").as_slice(),
        ),
    ] {
        assert_eq!(manifest["source_files"][path]["bytes"], source.len());
        assert_eq!(
            manifest["source_files"][path]["sha256"],
            hex::encode(Sha256::digest(source))
        );
    }
    let observation = manifest["observations"]
        .as_array()
        .unwrap()
        .iter()
        .find(|observation| observation["package"] == "projection")
        .unwrap();
    assert_eq!(observation["exit_status"], 0);
    assert_eq!(observation["stdin"]["bytes"], 0);
    assert_eq!(observation["stderr"]["bytes"], 0);
    assert_eq!(observation["stdout"]["bytes"], bytes.len());
    assert_eq!(
        observation["stdout"]["sha256"],
        hex::encode(Sha256::digest(bytes))
    );
    let output: OracleOutput = serde_json::from_slice(bytes).unwrap();
    let inputs: Vec<Case> = serde_json::from_str(CASES).unwrap();
    assert_eq!(inputs.len(), 7);
    assert_eq!(output.cases.len(), inputs.len());
    for input in &inputs {
        assert!(output.cases.contains_key(&input.name));
    }
    output
}

fn verify_native_output(bytes: &[u8], os: &str, arch: &str) {
    let native = frozen_native_capture::verify(
        frozen_native_capture::manifest_bytes(os, arch)
            .expect("unrecorded native projection target must use the actual Go producer"),
        os,
        arch,
    );
    let recorded = native["observations"]
        .as_array()
        .unwrap()
        .iter()
        .find(|record| record["package"] == "projection")
        .unwrap();
    assert_eq!(recorded["stdin"]["bytes"], 0);
    assert_eq!(
        recorded["stdin"]["sha256"],
        hex::encode(Sha256::digest(b""))
    );
    assert_eq!(recorded["stdout"]["bytes"], bytes.len());
    assert_eq!(
        recorded["stdout"]["sha256"],
        hex::encode(Sha256::digest(bytes))
    );
}

#[test]
fn frozen_projection_rejects_changed_bytes_and_missing_boundary_case() {
    for (os, arch) in [
        ("linux", "x86_64"),
        ("linux", "aarch64"),
        ("windows", "x86_64"),
        ("windows", "aarch64"),
        ("macos", "x86_64"),
        ("macos", "aarch64"),
    ] {
        let bytes = if arch == "x86_64" {
            include_bytes!("../../../tests/fixtures/go-frozen/projection/amd64.stdout").as_slice()
        } else {
            include_bytes!("../../../tests/fixtures/go-frozen/projection/arm64.stdout").as_slice()
        };
        verify_native_output(bytes, os, arch);
        assert!(
            std::panic::catch_unwind(|| verify_native_output(&bytes[..bytes.len() - 1], os, arch))
                .is_err()
        );
    }
    let mut changed = frozen_bytes().to_vec();
    changed[0] ^= 1;
    assert!(std::panic::catch_unwind(|| frozen_output(&changed)).is_err());
    let mut shortened: Value = serde_json::from_slice(frozen_bytes()).unwrap();
    shortened["cases"]
        .as_object_mut()
        .unwrap()
        .remove("int64_max_payload")
        .unwrap();
    let shortened = serde_json::to_vec(&shortened).unwrap();
    assert!(std::panic::catch_unwind(|| frozen_output(&shortened)).is_err());
}

#[test]
fn oversized_expected_response_days_matches_live_go_oracle() {
    let cases: Vec<Case> = serde_json::from_str(CASES).expect("valid projection cases");
    let oracle = run_go_oracle();
    assert_eq!(oracle.cases.len(), cases.len());
    let occurred_at = Utc
        .with_ymd_and_hms(2026, 1, 1, 0, 0, 0)
        .single()
        .expect("valid boundary timestamp");

    for (index, case) in cases.iter().enumerate() {
        let mut payload = Map::new();
        payload.insert(
            "expected_response_days".to_owned(),
            case.expected_response_days.clone(),
        );
        let event = EventRecord {
            id: (index + 1) as i64,
            request_id: (index + 1) as i64,
            occurred_at,
            recorded_at: occurred_at,
            event_type: EventType::Sent,
            payload,
            source: Source::System,
        };
        let rust_state = fold_events((index + 1) as i64, &[event]);
        let go_state = oracle
            .cases
            .get(&case.name)
            .unwrap_or_else(|| panic!("Go oracle omitted case {}", case.name));
        assert_eq!(
            &rust_state, go_state,
            "Rust projection differs from live Go oracle for {}",
            case.name
        );
    }
}

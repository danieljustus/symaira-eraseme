use chrono::{TimeZone, Utc};
use serde::Deserialize;
use serde_json::{Map, Value};
use std::process::Command;
use std::time::Duration;
use symeraseme_core::storage::{EventRecord, EventType, Source, fold_events};

#[path = "common/oracle_runner.rs"]
mod oracle_runner;

const CASES: &str = include_str!("../../../rust-tests/parity/oracle/projection/cases.json");
const ORACLE_TIMEOUT: Duration = Duration::from_secs(30);
const ORACLE_MAX_OUTPUT_BYTES: u64 = 4 * 1024 * 1024;

#[derive(Debug, Deserialize)]
struct Case {
    name: String,
    expected_response_days: Value,
}

#[derive(Debug, Deserialize)]
struct OracleOutput {
    cases: std::collections::BTreeMap<String, symeraseme_core::storage::ProjectionState>,
}

fn run_go_oracle() -> OracleOutput {
    let repo_root = concat!(env!("CARGO_MANIFEST_DIR"), "/../..");
    let temp = tempfile::tempdir().expect("create Go oracle temp directory");
    let executable = temp.path().join(if cfg!(windows) {
        "projection-oracle.exe"
    } else {
        "projection-oracle"
    });
    let mut build = Command::new("go");
    build
        .current_dir(repo_root)
        .args(["build", "-o"])
        .arg(&executable)
        .arg("./rust-tests/parity/oracle/projection")
        .env("GOWORK", "off");
    let build_output = oracle_runner::run(
        &mut build,
        &temp.path().join("build.stdout"),
        &temp.path().join("build.stderr"),
        ORACLE_TIMEOUT,
        ORACLE_MAX_OUTPUT_BYTES,
    )
    .expect("Go must be available and finish building the projection oracle");
    assert!(
        build_output.status.success(),
        "Go projection oracle build failed: {}",
        String::from_utf8_lossy(&build_output.stderr)
    );

    let mut oracle = Command::new(&executable);
    oracle.current_dir(repo_root).env_clear();
    let output = oracle_runner::run(
        &mut oracle,
        &temp.path().join("oracle.stdout"),
        &temp.path().join("oracle.stderr"),
        ORACLE_TIMEOUT,
        ORACLE_MAX_OUTPUT_BYTES,
    )
    .expect("Go projection oracle must finish within its bounded timeout/output limit");
    assert!(
        output.status.success(),
        "Go projection oracle failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    serde_json::from_slice(&output.stdout).expect("Go projection oracle must emit JSON")
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

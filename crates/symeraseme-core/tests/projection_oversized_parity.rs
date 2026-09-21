//! Live Go-oracle parity for oversized `expected_response_days` payloads.
//!
//! Go evaluates `time.Duration(days) * 24 * time.Hour` with signed int64
//! nanosecond arithmetic, so values at the duration boundary and at the int64
//! payload limits wrap instead of being rejected. The committed DB-004 fixture
//! only covers ordinary fractional and negative values, so this test pins the
//! boundary behaviour against a live Go executable oracle.

#[path = "support/go_oracle.rs"]
mod go_oracle;

use chrono::{TimeZone, Utc};
use serde::Deserialize;
use serde_json::{Map, Value};
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
    let run = go_oracle::run_oracle("projection", None);
    assert!(
        run.status.success(),
        "Go projection oracle failed: {}",
        String::from_utf8_lossy(&run.stderr)
    );
    serde_json::from_slice(&run.stdout).expect("Go projection oracle must emit JSON")
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

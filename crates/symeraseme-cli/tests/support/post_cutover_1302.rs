//! Explicit expected-value transform for the post-cutover contract change
//! #1302 (see `docs/rust-port-contract-matrix.md`).
//!
//! The frozen Go records stay untouched. Go printed only `success` for the
//! text mode of `schedule status`; the replay derives the new lines from the
//! recorded Go JSON payload of the same scenario and nothing else.
#![allow(dead_code)] // Each including test target uses a subset.

use serde_json::Value;

/// Replaces a recorded text-mode `success\n` of `schedule status` with the
/// current rendering of the same scenario's recorded JSON payload
/// (`Platform` and `Entries[]` with `Label`, `Installed`, `Active`, `Legacy`,
/// `Path`, `Error`). Any other recorded text passes through unchanged.
pub fn schedule_status_text(recorded_text: &[u8], recorded_json: &[u8]) -> Vec<u8> {
    if recorded_text != b"success\n" {
        return recorded_text.to_vec();
    }
    let payload: Value = serde_json::from_slice(recorded_json).expect("recorded status JSON");
    let flag = |entry: &Value, key: &str| entry[key].as_bool().expect("recorded flag");
    let mut out = format!(
        "Scheduler platform: {}\n",
        payload["Platform"].as_str().unwrap()
    );
    for entry in payload["Entries"].as_array().expect("recorded entries") {
        out.push_str(&format!(
            "  {}: {}, {}{} ({})",
            entry["Label"].as_str().unwrap(),
            if flag(entry, "Installed") {
                "installed"
            } else {
                "not installed"
            },
            if flag(entry, "Active") {
                "active"
            } else {
                "inactive"
            },
            if flag(entry, "Legacy") {
                ", legacy"
            } else {
                ""
            },
            entry["Path"].as_str().unwrap(),
        ));
        let error = entry["Error"].as_str().unwrap_or("");
        if !error.is_empty() {
            out.push_str(&format!(" - {error}"));
        }
        out.push('\n');
    }
    out.into_bytes()
}

#[test]
fn schedule_status_transform_is_narrow() {
    let json = br#"{"Platform":"cron","Entries":[{"Label":"cron","Installed":false,"Active":false,"Path":"crontab","Legacy":false,"Error":"no crontab"}]}"#;
    assert_eq!(
        schedule_status_text(b"success\n", json),
        b"Scheduler platform: cron\n  cron: not installed, inactive (crontab) - no crontab\n"
    );
    assert_eq!(schedule_status_text(b"other\n", json), b"other\n");
    assert_eq!(schedule_status_text(b"", json), b"");
}

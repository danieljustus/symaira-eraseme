//! Portable controls for the frozen native Windows HTTP reader.
//!
//! These run on every host. They authenticate the committed real Go observations
//! and drive tampering through the production comparator. Passing here is not
//! Windows runtime evidence; native replay lives in mcp_http_windows_process.rs.
#[path = "support/frozen_windows_http.rs"]
mod frozen_windows_http;

use frozen_windows_http as frozen;
use serde_json::{Value, json};
use std::ffi::OsStr;

const TARGETS: [&str; 2] = ["windows/amd64", "windows/arm64"];
const HTTP: &str = "native_windows_http_matches_checked_out_go";
const HOST: &str = "native_windows_disconnect_reaps_host_agent_like_go";
const PATHEXT: &str = "native_windows_pathext_availability_matches_live_go";
const PROVIDER: &str = "native_windows_disconnect_cancels_provider_request_like_go";
const SLOW: &str = "native_windows_slow_header_timeout_matches_go";
const CTRL_BREAK: &str = "native_windows_ctrl_break_shutdown_matches_go";

fn go_index(items: &Value) -> usize {
    items
        .as_array()
        .unwrap()
        .iter()
        .position(|item| item["implementation"] == "go")
        .unwrap()
}

/// True when the production validator rejects a mutated copy of a valid record.
fn rejects(
    valid: &Value,
    target: &str,
    test: &str,
    streams: u64,
    edit: impl FnOnce(&mut Value),
) -> bool {
    let mut value = valid.clone();
    edit(&mut value);
    frozen::validate_record(&value, target, test, streams).is_err()
}

fn streams_of(target: &str, test: &str) -> u64 {
    let selection = frozen::verify_selection(frozen::SELECTION, frozen::SELECTION_PIN).unwrap();
    frozen::selection_entries(&selection)
        .unwrap()
        .into_iter()
        .find(|entry| entry.target == target && entry.test == test)
        .unwrap()
        .streams
}

#[test]
fn frozen_records_authenticate_and_validate_on_every_host() {
    let mut loaded = 0;
    for target in TARGETS {
        for test in frozen::CASES {
            frozen::load(target, test).unwrap_or_else(|error| panic!("{target} {test}: {error}"));
            loaded += 1;
        }
    }
    assert_eq!(loaded, 14);
}

#[test]
fn closure_is_bound_to_immutable_producer_blobs() {
    let summary = frozen::closure_summary().unwrap();
    assert_eq!(summary.sources, 1478);
    assert_eq!(summary.generators, 10);
}

#[test]
fn retired_live_capture_switch_is_rejected_even_when_empty() {
    assert!(frozen::reject_retired(None).is_ok());
    assert!(frozen::reject_retired(Some(OsStr::new(""))).is_err());
    assert!(frozen::reject_retired(Some(OsStr::new("1"))).is_err());
}

#[test]
fn whole_byte_pins_reject_selection_and_member_tampering() {
    let selection = frozen::verify_selection(frozen::SELECTION, frozen::SELECTION_PIN).unwrap();
    let mut appended = frozen::SELECTION.to_vec();
    appended.push(b'\n');
    assert!(frozen::verify_selection(&appended, frozen::SELECTION_PIN).is_err());
    let mut flipped = frozen::SELECTION.to_vec();
    flipped[0] ^= 1;
    assert!(frozen::verify_selection(&flipped, frozen::SELECTION_PIN).is_err());
    assert!(frozen::verify_selection(frozen::SELECTION, &"0".repeat(64)).is_err());
    // A missing case cannot slip through: removing one record changes the whole-byte pin.
    let mut dropped = selection.clone();
    dropped["records"].as_array_mut().unwrap().remove(0);
    let reserialized = serde_json::to_vec(&dropped).unwrap();
    assert!(frozen::verify_selection(&reserialized, frozen::SELECTION_PIN).is_err());

    let entries = frozen::selection_entries(&selection).unwrap();
    assert_eq!(entries.len(), 14);
    let entry = &entries[0];
    let raw = frozen::member_bytes(&entry.target, &entry.test).unwrap();
    frozen::authenticate_member(raw, entry).unwrap();
    let mut flipped_member = raw.to_vec();
    flipped_member[0] ^= 1;
    assert!(frozen::authenticate_member(&flipped_member, entry).is_err());
    let mut grown = raw.to_vec();
    grown.push(b' ');
    assert!(frozen::authenticate_member(&grown, entry).is_err());
    assert!(frozen::authenticate_member(&raw[..raw.len() - 1], entry).is_err());
}

#[test]
fn comparator_rejects_semantic_mutations_of_committed_records() {
    let target = "windows/amd64";

    // HTTP: target/test/stream mismatches, incomplete record, bool/int, missing/null.
    let http = frozen::load(target, HTTP).unwrap().value;
    let s = streams_of(target, HTTP);
    assert!(frozen::validate_record(&http, "windows/arm64", HTTP, s).is_err());
    assert!(frozen::validate_record(&http, target, HOST, s).is_err());
    assert!(frozen::validate_record(&http, target, HTTP, s + 1).is_err());
    assert!(rejects(&http, target, HTTP, s, |v| {
        v.as_object_mut().unwrap().remove("observations");
    }));
    assert!(rejects(&http, target, HTTP, s, |v| {
        v.as_object_mut().unwrap().remove("outcome");
    }));
    assert!(rejects(&http, target, HTTP, s, |v| {
        v["case_version"] = json!(true);
    }));
    assert!(rejects(&http, target, HTTP, s, |v| {
        v["checkout_autocrlf"] = json!(true);
    }));
    assert!(rejects(&http, target, HTTP, s, |v| {
        v["source_files"]["go.mod"]["sha256"] = json!("0".repeat(64));
    }));
    assert!(rejects(&http, target, HTTP, s, |v| {
        v["source_revision"] = json!("0".repeat(40));
    }));
    assert!(rejects(&http, target, HTTP, s, |v| {
        v["source_files"].as_object_mut().unwrap().remove("go.mod");
    }));
    assert!(rejects(&http, target, HTTP, s, |v| {
        v["observations"]["http"][0]["go"]["body"]["sha256"] = json!("0".repeat(64));
    }));
    assert!(rejects(&http, target, HTTP, s, |v| {
        v["observations"]["http"][0]["go"]["status"] = json!(true);
    }));
    assert!(rejects(&http, target, HTTP, s, |v| {
        v["observations"]["http"][0]["go"]["status"] = json!("401");
    }));
    assert!(rejects(&http, target, HTTP, s, |v| {
        v["observations"]["http"][0]
            .as_object_mut()
            .unwrap()
            .remove("origin");
    }));
    assert!(rejects(&http, target, HTTP, s, |v| {
        v["observations"]["http"][2]["origin"] = json!("https://other.example");
    }));

    // HOST: missing vs null classification, and bool-for-integer wait results.
    let host = frozen::load(target, HOST).unwrap().value;
    let hs = streams_of(target, HOST);
    assert!(rejects(&host, target, HOST, hs, |v| {
        let i = go_index(&v["observations"]["persisted_classification"]);
        v["observations"]["persisted_classification"][i]
            .as_object_mut()
            .unwrap()
            .remove("classified_as");
    }));
    assert!(rejects(&host, target, HOST, hs, |v| {
        let i = go_index(&v["observations"]["persisted_classification"]);
        v["observations"]["persisted_classification"][i]["classified_as"] = json!("null");
    }));
    assert!(rejects(&host, target, HOST, hs, |v| {
        let i = go_index(&v["observations"]["host_agent"]);
        v["observations"]["host_agent"][i]["wait_result"] = json!(true);
    }));

    // PATHEXT: value, source digest and rust-lookup string/bool confusion.
    let pathext = frozen::load(target, PATHEXT).unwrap().value;
    let ps = streams_of(target, PATHEXT);
    assert!(rejects(&pathext, target, PATHEXT, ps, |v| {
        v["observations"]["pathext"][0]["pathext"] = json!(".EXE2");
    }));
    assert!(rejects(&pathext, target, PATHEXT, ps, |v| {
        v["observations"]["pathext"][0]["go_sources_sha256"]["go.mod"] = json!("0".repeat(64));
    }));
    assert!(rejects(&pathext, target, PATHEXT, ps, |v| {
        let i = v["observations"]["pathext"]
            .as_array()
            .unwrap()
            .iter()
            .position(|item| item.get("rust_lookup").is_some())
            .unwrap();
        v["observations"]["pathext"][i]["rust_lookup"] = json!(true);
    }));

    // PROVIDER: fixture digest must match its authenticated bytes.
    let provider = frozen::load(target, PROVIDER).unwrap().value;
    let prs = streams_of(target, PROVIDER);
    assert!(rejects(&provider, target, PROVIDER, prs, |v| {
        v["observations"]["provider_fixture"][0]["generated"]["sha256"] = json!("0".repeat(64));
    }));

    // SLOW: the recorded timing class stays within its original bounds.
    let slow = frozen::load(target, SLOW).unwrap().value;
    let ss = streams_of(target, SLOW);
    assert!(rejects(&slow, target, SLOW, ss, |v| {
        let i = go_index(&v["observations"]["slow_header"]);
        v["observations"]["slow_header"][i]["closure"]["elapsed_ms"] = json!(9000);
    }));

    // CONSOLE: a missing exit code or a string success flag is not a recorded result.
    let ctrl = frozen::load(target, CTRL_BREAK).unwrap().value;
    let cs = streams_of(target, CTRL_BREAK);
    assert!(rejects(&ctrl, target, CTRL_BREAK, cs, |v| {
        let i = v["observations"]["console_children"]
            .as_array()
            .unwrap()
            .iter()
            .position(|item| item.get("exit_code").is_some())
            .unwrap();
        v["observations"]["console_children"][i]
            .as_object_mut()
            .unwrap()
            .remove("exit_code");
    }));
    assert!(rejects(&ctrl, target, CTRL_BREAK, cs, |v| {
        let i = v["observations"]["console_children"]
            .as_array()
            .unwrap()
            .iter()
            .position(|item| item.get("exit_code").is_some())
            .unwrap();
        v["observations"]["console_children"][i]["exit_success"] = json!("true");
    }));
}

#[test]
fn both_native_targets_validate_without_claiming_runtime_success() {
    for target in TARGETS {
        let record = frozen::load(target, HTTP).unwrap();
        assert_eq!(record.streams, 4);
        assert_eq!(record.target, target);
    }
}

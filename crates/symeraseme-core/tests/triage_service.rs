use chrono::{TimeZone, Utc};
use serde::Deserialize;
use serde_json::json;
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use symeraseme_core::llm::UsageRecord;
use symeraseme_core::storage::{EventType, Repository, Source, Store};
use symeraseme_core::triage_service::{
    ClassifyRequest, LlmCall, LlmResponse, RebuttalRequest, Service,
};
use tempfile::tempdir;

#[path = "support/go_oracle.rs"]
mod go_oracle;

const CLASSIFIER_GO: &[u8] = include_bytes!("../../../internal/triage/classifier.go");
const REBUTTAL_GO: &[u8] = include_bytes!("../../../internal/triage/rebuttal.go");
const REPLIES_SERVICE_GO: &[u8] = include_bytes!("../../../internal/replies/service.go");
const REPLIES_REPOSITORY_GO: &[u8] = include_bytes!("../../../internal/replies/repository.go");
const LLM_GO: &[u8] = include_bytes!("../../../internal/llm/llm.go");

#[derive(Deserialize)]
struct GoOracle {
    source_sha256: BTreeMap<String, String>,
    classify: serde_json::Value,
    rebuttal: serde_json::Value,
}

fn sha256_hex(bytes: &[u8]) -> String {
    hex::encode(Sha256::digest(bytes))
}

#[test]
fn go_executable_is_source_bound_and_records_persisted_service_effects() {
    let run = go_oracle::run_oracle("triage-service", None);
    assert!(
        run.status.success(),
        "Go service oracle failed: {}",
        String::from_utf8_lossy(&run.stderr)
    );
    let oracle: GoOracle = serde_json::from_slice(&run.stdout).expect("valid Go oracle output");
    for (name, source) in [
        ("internal/triage/classifier.go", CLASSIFIER_GO),
        ("internal/triage/rebuttal.go", REBUTTAL_GO),
        ("internal/replies/service.go", REPLIES_SERVICE_GO),
        ("internal/replies/repository.go", REPLIES_REPOSITORY_GO),
        ("internal/llm/llm.go", LLM_GO),
    ] {
        assert_eq!(
            oracle.source_sha256[name],
            sha256_hex(source),
            "Go oracle source drift: {name}"
        );
    }
    assert_eq!(oracle.classify["classification"], "ack");
    assert_eq!(oracle.classify["event_type"], "ACK");
    assert_eq!(oracle.classify["row_classified_as"], "ack");
    assert_eq!(oracle.classify["event_payload"]["reply_id"], 1);
    assert_eq!(
        oracle.classify["event_payload"]["extracted_fields"]["case"],
        "42"
    );
    assert_eq!(
        oracle.rebuttal["template_name"],
        "gdpr-rebuttal-address.md.j2"
    );
    assert_eq!(oracle.rebuttal["event_type"], "REBUTTAL_SENT");
    assert_eq!(
        oracle.rebuttal["event_payload"]["rejection_classification"],
        "address_mismatch"
    );
}

fn fixture_store() -> (tempfile::TempDir, Store, i64) {
    let directory = tempdir().unwrap();
    let store = Store::open(directory.path().join("triage.db")).unwrap();
    let repository = Repository::new(&store);
    repository
        .create_campaign("triage-test", "initial", "")
        .unwrap();
    let request_id = repository
        .create_removal_request("broker", "email", "triage-test", "GDPR", "template", "hash")
        .unwrap();
    store
        .append_and_project(
            request_id,
            &EventType::Planned,
            &Default::default(),
            &Source::System,
            Utc.with_ymd_and_hms(2026, 9, 1, 0, 0, 0).unwrap(),
        )
        .unwrap();
    store
        .connection()
        .execute(
            "INSERT INTO inbox_replies (request_id, message_id, subject, snippet) VALUES (?1, 'message-1', 'Reply subject', 'We received your request')",
            [request_id],
        )
        .unwrap();
    (directory, store, request_id)
}

#[test]
fn classify_reply_uses_injected_call_and_saves_row_and_event() {
    let (_directory, store, request_id) = fixture_store();
    let service = Service::new(&store);
    let call: &LlmCall<'_> = &|system, prompt, cache_key| {
        assert!(system.contains("precise email classifier"));
        assert!(prompt.contains("Reply subject: Reply subject"));
        assert_eq!(cache_key, "broker:Example");
        Ok(LlmResponse {
            text: r#"{"classification":"ack","confidence":0.91,"summary":"received","extracted_fields":{"case":"42"}}"#.into(),
            usage: UsageRecord { model: "fake".into(), ..UsageRecord::default() },
        })
    };
    let outcome = service
        .classify_reply(
            request_id,
            &ClassifyRequest {
                broker_name: "Example".into(),
                ..Default::default()
            },
            None,
            Some(call),
            true,
        )
        .unwrap();
    assert_eq!(outcome.result.classification, "ack");
    assert_eq!(outcome.usage.model, "fake");

    let row: (String, f64, String) = store
        .connection()
        .query_row(
            "SELECT classified_as, classifier_confidence, llm_summary FROM inbox_replies WHERE request_id = ?1",
            [request_id],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )
        .unwrap();
    assert_eq!(row, ("ack".into(), 0.91, "received".into()));
    let events = Repository::new(&store).get_events(request_id, 0).unwrap();
    let event = events.last().unwrap();
    assert_eq!(event.event_type.as_str(), "ACK");
    assert_eq!(event.payload["reply_id"], json!(1));
    assert_eq!(event.payload["extracted_fields"]["case"], "42");
}

#[test]
fn generate_rebuttal_calls_classifier_and_saves_matching_event() {
    let (_directory, store, request_id) = fixture_store();
    let call: &LlmCall<'_> = &|system, prompt, cache_key| {
        assert!(system.contains("precise rejection classifier"));
        assert!(prompt.contains("We received your request"));
        assert_eq!(cache_key, "rebuttal:Example");
        Ok(LlmResponse {
            text: r#"{"classification":"address_mismatch","confidence":0.9,"summary":"address","jurisdiction":"GDPR","key_points":["old address"]}"#.into(),
            usage: UsageRecord::default(),
        })
    };
    let result = Service::new(&store)
        .generate_rebuttal(
            request_id,
            &RebuttalRequest {
                broker_name: "Example".into(),
                original_request_template: "original request".into(),
                original_request_date: "2026-09-01".into(),
                ..Default::default()
            },
            None,
            Some(call),
            true,
        )
        .unwrap();
    assert_eq!(result.template_name, "gdpr-rebuttal-address.md.j2");
    assert!(result.llm_used);
    let events = Repository::new(&store).get_events(request_id, 0).unwrap();
    let event = events.last().unwrap();
    assert_eq!(event.event_type.as_str(), "REBUTTAL_SENT");
    assert_eq!(
        event.payload["rejection_classification"],
        "address_mismatch"
    );
    assert_eq!(
        event.payload["broker_message_snippet"],
        "We received your request"
    );
}

#[test]
fn classify_without_llm_saves_safe_review_default() {
    let (_directory, store, request_id) = fixture_store();
    let outcome = Service::new(&store)
        .classify_reply(request_id, &ClassifyRequest::default(), None, None, true)
        .unwrap();
    assert_eq!(outcome.result.classification, "unclear");
    assert!(outcome.result.needs_human_review);
    let events = Repository::new(&store).get_events(request_id, 0).unwrap();
    assert_eq!(
        events.last().unwrap().event_type.as_str(),
        "HUMAN_ACTION_REQUIRED"
    );
}

#[test]
fn classifier_error_is_returned_without_persisting_or_emitting_an_event() {
    let (_directory, store, request_id) = fixture_store();
    let call: &LlmCall<'_> = &|_, _, _| Err("provider unavailable".into());
    let outcome = Service::new(&store)
        .classify_reply(
            request_id,
            &ClassifyRequest::default(),
            None,
            Some(call),
            true,
        )
        .unwrap();
    assert_eq!(outcome.error.as_deref(), Some("provider unavailable"));
    assert_eq!(
        outcome.result.summary.wire_string(),
        "API error: provider unavailable"
    );
    let classified: Option<String> = store
        .connection()
        .query_row(
            "SELECT classified_as FROM inbox_replies WHERE request_id = ?1",
            [request_id],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(classified, None);
    assert_eq!(
        Repository::new(&store)
            .get_events(request_id, 0)
            .unwrap()
            .len(),
        1
    );
}

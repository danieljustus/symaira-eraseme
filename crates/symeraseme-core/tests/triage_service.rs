use serde::Deserialize;
use serde_json::json;
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use symeraseme_core::llm::UsageRecord;
use symeraseme_core::storage::{Repository, Store};
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
const EVENTSTORE_GO: &[u8] = include_bytes!("../../../internal/eventstore/store.go");
const PROJECTION_GO: &[u8] = include_bytes!("../../../internal/eventstore/projection.go");
const ORACLE_GO: &[u8] = include_bytes!("../../../rust-tests/parity/oracle/triage-service/main.go");

#[derive(Deserialize)]
struct GoOracle {
    source_sha256: BTreeMap<String, String>,
    classify: serde_json::Value,
    rebuttal: serde_json::Value,
    fallback: serde_json::Value,
    llm_error: serde_json::Value,
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
    for (name, source, expected_digest) in [
        (
            "internal/triage/classifier.go",
            CLASSIFIER_GO,
            "60adadce027c295a2ffed33baeae2d968a6b3f4a7d9a12a49131c3050756fc97",
        ),
        (
            "internal/triage/rebuttal.go",
            REBUTTAL_GO,
            "232c6850ed8514a6cad5f36eaa165a92b4d3dbaad61ec69fa1ad5d03ffbae3aa",
        ),
        (
            "internal/replies/service.go",
            REPLIES_SERVICE_GO,
            "8144b9407e40d7e1c9aa97528dcb2ef7b814fae66b2f27a4902fb853fe7e0281",
        ),
        (
            "internal/replies/repository.go",
            REPLIES_REPOSITORY_GO,
            "725369fd4a9527004870ab757cbfbb95b6d61e02015ff6debd7e3685cc753e7f",
        ),
        (
            "internal/llm/llm.go",
            LLM_GO,
            "537cb22f0c1536a9e33800a018f471402e2f0688a873cbee96402fef9d96b717",
        ),
        (
            "internal/eventstore/store.go",
            EVENTSTORE_GO,
            "fd1dd416606f29aa4726a62ffe6ad83ef9d7c9eb6c6f42d81e87987913968df3",
        ),
        (
            "internal/eventstore/projection.go",
            PROJECTION_GO,
            "edfa14b15261c65672ca5a35eb05c16190a0d55bd07f9ce3df267b531295c43a",
        ),
        (
            "rust-tests/parity/oracle/triage-service/main.go",
            ORACLE_GO,
            "05f8e0ae89644c4dfd420b3b37afb950f19e1dea94b3160d484bb8c21c2b8810",
        ),
    ] {
        assert_eq!(
            oracle.source_sha256[name], expected_digest,
            "Go oracle source drift: {name}"
        );
        assert_eq!(
            sha256_hex(source),
            expected_digest,
            "pinned source changed: {name}"
        );
    }
    let (_directory, store, request_id) = fixture_store();
    let service = Service::new(&store);
    let classify_call: &LlmCall<'_> = &|_, _, _| {
        Ok(LlmResponse {
            text: r#"{"classification":"ack","confidence":0.91,"summary":"received","extracted_fields":{"case":"42"}}"#.into(),
            usage: UsageRecord { model: "fake".into(), ..UsageRecord::default() },
        })
    };
    let classification = service
        .classify_reply(
            request_id,
            &ClassifyRequest {
                broker_name: "Example".into(),
                ..Default::default()
            },
            None,
            Some(classify_call),
            true,
        )
        .unwrap();
    let repository = Repository::new(&store);
    let reply: serde_json::Value = store.connection().query_row(
        "SELECT id, request_id, message_id, thread_id, from_addr, subject, snippet, classified_as, classifier_confidence, llm_summary FROM inbox_replies WHERE request_id = ?1",
        [request_id],
        |row| Ok(json!({
            "id": row.get::<_, i64>(0)?, "request_id": row.get::<_, i64>(1)?,
            "message_id": row.get::<_, String>(2)?, "thread_id": row.get::<_, Option<String>>(3)?,
            "from": row.get::<_, Option<String>>(4)?, "subject": row.get::<_, Option<String>>(5)?,
            "snippet": row.get::<_, Option<String>>(6)?, "classified_as": row.get::<_, Option<String>>(7)?,
            "classifier_confidence": row.get::<_, Option<f64>>(8)?, "llm_summary": row.get::<_, Option<String>>(9)?,
        })),
    ).unwrap();
    let classify = json!({
        "result": {
            "classification": classification.result.classification,
            "event_type": classification.result.event_type,
            "confidence": classification.result.confidence,
            "summary": classification.result.summary.wire_string(),
            "extracted_fields": classification.result.extracted_fields,
            "needs_human_review": classification.result.needs_human_review,
            "usage": classification.usage.record(),
        },
        "reply": reply,
        "events": normalized_events(&repository, request_id),
        "projection": normalized_projection(&store, request_id),
    });
    assert_json_equal(
        &classify,
        &oracle.classify,
        "Go/Rust classification results or persisted effects diverged",
    );

    let rebuttal_call: &LlmCall<'_> = &|_, _, _| {
        Ok(LlmResponse {
            text: r#"{"classification":"address_mismatch","confidence":0.9,"summary":"address","jurisdiction":"GDPR","key_points":["old address"]}"#.into(),
            usage: UsageRecord { model: "fake".into(), ..UsageRecord::default() },
        })
    };
    let rebuttal = service
        .generate_rebuttal(
            request_id,
            &RebuttalRequest {
                broker_name: "Example".into(),
                original_request_template: "original request".into(),
                original_request_date: "2026-09-01".into(),
                ..Default::default()
            },
            None,
            Some(rebuttal_call),
            true,
        )
        .unwrap();
    let rebuttal_value = normalized_rebuttal(&rebuttal, &repository, request_id);
    assert_json_equal(
        &rebuttal_value,
        &oracle.rebuttal,
        "Go/Rust rebuttal results or persisted events diverged",
    );

    let fallback_request_id = empty_request(&repository, "triage-fallback");
    let fallback = service
        .generate_rebuttal(
            fallback_request_id,
            &fallback_rebuttal_request(),
            None,
            None,
            true,
        )
        .unwrap();
    assert_json_equal(
        &normalized_rebuttal(&fallback, &repository, fallback_request_id),
        &oracle.fallback,
        "Go/Rust nil-client rebuttal fallback diverged",
    );

    let error_request_id = empty_request(&repository, "triage-llm-error");
    let failing_call: &LlmCall<'_> = &|_, _, _| Err("fake classifier failure".into());
    let llm_error = service
        .generate_rebuttal(
            error_request_id,
            &fallback_rebuttal_request(),
            None,
            Some(failing_call),
            true,
        )
        .unwrap();
    assert_json_equal(
        &normalized_rebuttal(&llm_error, &repository, error_request_id),
        &oracle.llm_error,
        "Go/Rust LLM-error rebuttal fallback diverged",
    );
}

fn empty_request(repository: &Repository<'_>, campaign: &str) -> i64 {
    repository.create_campaign(campaign, "initial", "").unwrap();
    repository
        .create_removal_request("broker", "email", campaign, "GDPR", "template", "hash")
        .unwrap()
}

fn fallback_rebuttal_request() -> RebuttalRequest {
    RebuttalRequest {
        broker_name: "Example".into(),
        original_request_template: "The old address on file is wrong".into(),
        original_request_date: "2026-09-01".into(),
        ..Default::default()
    }
}

fn normalized_rebuttal(
    result: &symeraseme_core::triage_service::RebuttalResult,
    repository: &Repository<'_>,
    request_id: i64,
) -> serde_json::Value {
    json!({
        "result": {
            "template_name": result.template_name,
            "label": result.label,
            "description": result.description,
            "jurisdiction": result.jurisdiction,
            "rejection_classification": result.rejection_classification,
            "confidence": result.confidence,
            "rebuttal_body": result.rebuttal_body,
            "needs_human_review": result.needs_human_review,
            "llm_used": result.llm_used,
            "usage": result.usage.record(),
        },
        "events": normalized_events(repository, request_id),
        "projection": normalized_projection(repository.store(), request_id),
    })
}

fn assert_json_equal(left: &serde_json::Value, right: &serde_json::Value, message: &str) {
    fn equal(left: &serde_json::Value, right: &serde_json::Value) -> bool {
        match (left, right) {
            (serde_json::Value::Number(left), serde_json::Value::Number(right)) => {
                left.as_f64() == right.as_f64()
            }
            (serde_json::Value::Array(left), serde_json::Value::Array(right)) => {
                left.len() == right.len() && left.iter().zip(right).all(|(a, b)| equal(a, b))
            }
            (serde_json::Value::Object(left), serde_json::Value::Object(right)) => {
                left.len() == right.len()
                    && left
                        .iter()
                        .all(|(key, value)| right.get(key).is_some_and(|other| equal(value, other)))
            }
            _ => left == right,
        }
    }
    assert!(equal(left, right), "{message}\nRust: {left}\nGo: {right}");
}

fn normalized_events(repository: &Repository<'_>, request_id: i64) -> serde_json::Value {
    let events = repository.get_events(request_id, 0).unwrap();
    json!(
        events
            .into_iter()
            .map(|event| json!({
                "id": event.id,
                "request_id": event.request_id,
                "event_type": event.event_type.as_str(),
                "source": event.source.as_str(),
                "payload": event.payload,
            }))
            .collect::<Vec<_>>()
    )
}

fn normalized_projection(store: &Store, request_id: i64) -> serde_json::Value {
    store
        .connection()
        .query_row(
            "SELECT current_status, last_event_id, reminders_sent, escalation_level FROM request_state WHERE request_id = ?1",
            [request_id],
            |row| Ok(json!({
                "current_status": row.get::<_, String>(0)?,
                "last_event_id": row.get::<_, i64>(1)?,
                "reminders_sent": row.get::<_, i64>(2)?,
                "escalation_level": row.get::<_, i64>(3)?,
            })),
        )
        .unwrap()
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
        .connection()
        .execute(
            "INSERT INTO inbox_replies (request_id, message_id, thread_id, from_addr, subject, snippet) VALUES (?1, 'message-1', 'thread', 'broker@example.test', 'Reply subject', 'We received your request')",
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
        0
    );
}

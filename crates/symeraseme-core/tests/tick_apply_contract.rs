//! Tick transitions: scanning due requests and applying the resulting actions.
//!
//! Mirrors the transition half of Go's `internal/deadlines` conformance tests:
//! `RunTick` scans, `ApplyTickActions` appends each action's event with source
//! `scheduler`, dry runs write nothing, and the projections are rebuilt.

use chrono::{DateTime, Duration, Utc};
use serde_json::{Map, Value, json};
use symeraseme_core::deadlines::{RunOpts, apply_tick_actions, run_tick};
use symeraseme_core::storage::repository::Repository;
use symeraseme_core::storage::store::Store;
use symeraseme_core::storage::types::{EventType, Source};
use tempfile::tempdir;

fn pinned_now() -> DateTime<Utc> {
    DateTime::parse_from_rfc3339("2026-08-06T12:00:00+00:00")
        .expect("pinned instant")
        .with_timezone(&Utc)
}

/// A request whose next action is unset, so the scan always picks it up.
fn seed_awaiting_ack(store: &Store, sent_at: &str) -> i64 {
    let repository = Repository::new(store);
    let request_id = repository
        .create_removal_request("broker-a", "email", "tick-contract", "DE", "", "")
        .expect("create request");
    store
        .connection()
        .execute(
            "INSERT INTO request_state(request_id,current_status,sent_at,reminders_sent,escalation_level)\n\
             VALUES (?, 'AWAITING_ACK', ?, 0, 0)",
            rusqlite::params![request_id, sent_at],
        )
        .expect("seed state");
    request_id
}

fn events_for(store: &Store, request_id: i64) -> Vec<(String, String)> {
    let connection = store.connection();
    let mut statement = connection
        .prepare("SELECT event_type, source FROM request_events WHERE request_id = ? ORDER BY id")
        .expect("prepare");
    let rows = statement
        .query_map([request_id], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
        })
        .expect("query");
    rows.map(|row| row.expect("row")).collect()
}

#[test]
fn run_tick_scans_and_apply_writes_the_scheduler_event() {
    let tree = tempdir().expect("temp dir");
    let store = Store::open(tree.path().join("db.sqlite")).expect("open store");
    let now = pinned_now();
    // 21 days since sending: the first reminder is due (7-day threshold).
    let sent_at = (now - Duration::days(21)).to_rfc3339();
    let request_id = seed_awaiting_ack(&store, &sent_at);

    let actions = run_tick(
        &store,
        &RunOpts {
            dry_run: false,
            batch_size: 0,
        },
        now,
    )
    .expect("run tick");
    assert_eq!(actions.len(), 1, "{actions:?}");
    assert_eq!(actions[0].request_id, request_id);
    assert_eq!(actions[0].action_type, "send_reminder");
    assert_eq!(actions[0].event_type, "REMINDER_SENT");
    assert!(!actions[0].dry_run);

    let results = apply_tick_actions(&store, &actions, now).expect("apply");
    assert_eq!(results.len(), 1);
    assert!(results[0].executed, "{results:?}");
    assert_eq!(results[0].event_type, "REMINDER_SENT");
    assert!(results[0].error.is_empty());

    let events = events_for(&store, request_id);
    assert_eq!(
        events,
        vec![("REMINDER_SENT".to_owned(), "scheduler".to_owned())]
    );
}

#[test]
fn dry_run_reports_without_writing() {
    let tree = tempdir().expect("temp dir");
    let store = Store::open(tree.path().join("db.sqlite")).expect("open store");
    let now = pinned_now();
    let sent_at = (now - Duration::days(21)).to_rfc3339();
    let request_id = seed_awaiting_ack(&store, &sent_at);

    let actions = run_tick(
        &store,
        &RunOpts {
            dry_run: true,
            batch_size: 0,
        },
        now,
    )
    .expect("run tick");
    assert_eq!(actions.len(), 1);
    assert!(actions[0].dry_run);

    let results = apply_tick_actions(&store, &actions, now).expect("apply");
    assert_eq!(results.len(), 1);
    assert!(!results[0].executed);
    assert!(results[0].dry_run);
    assert!(
        events_for(&store, request_id).is_empty(),
        "dry run wrote an event"
    );
}

#[test]
fn apply_without_actions_writes_nothing() {
    let tree = tempdir().expect("temp dir");
    let store = Store::open(tree.path().join("db.sqlite")).expect("open store");
    let results = apply_tick_actions(&store, &[], pinned_now()).expect("apply");
    assert!(results.is_empty());
}

#[test]
fn golden_tick_histories_scan_and_persist_all_four_go_actions() {
    // The same event histories and fixture consumed by Go's
    // TestGoldenTickConformance and TestApplyTickActionsWritesEvents.
    let tree = tempdir().expect("temp dir");
    let store = Store::open(tree.path().join("db.sqlite")).expect("open store");
    let fixture: Value = serde_json::from_str(include_str!(
        "../../../tests/fixtures/event-store/golden-tick.json"
    ))
    .expect("golden tick fixture");
    let now = DateTime::parse_from_rfc3339(fixture["now"].as_str().expect("now"))
        .expect("timestamp")
        .with_timezone(&Utc);

    let histories: [(&str, &[(EventType, Value, i64)]); 4] = [
        (
            "broker-a",
            &[
                (EventType::Sent, json!({"expected_response_days": 30}), -21),
                (
                    EventType::ReminderSent,
                    json!({"count": 1, "days_since_sent": 14}),
                    -14,
                ),
            ],
        ),
        (
            "broker-b",
            &[
                (EventType::Sent, json!({"expected_response_days": 30}), -40),
                (EventType::VerificationProvided, json!({}), -38),
            ],
        ),
        (
            "broker-c",
            &[
                (EventType::Sent, json!({"expected_response_days": 30}), -50),
                (
                    EventType::DeadlineReached,
                    json!({"deadline_days": 30}),
                    -20,
                ),
            ],
        ),
        (
            "broker-d",
            &[
                (EventType::Sent, json!({"expected_response_days": 30}), -100),
                (EventType::Ack, json!({"message_id": "m-1"}), -98),
                (EventType::Confirmed, json!({"via": "ack"}), -98),
            ],
        ),
    ];
    for (broker, events) in histories {
        let id = Repository::new(&store)
            .create_removal_request(broker, "email", "golden-tick", "GDPR", "", "")
            .expect("create request");
        for (event_type, payload, days) in events {
            let payload: Map<String, Value> = payload.as_object().expect("object").clone();
            store
                .append_and_project(
                    id,
                    event_type,
                    &payload,
                    &Source::System,
                    now + Duration::days(*days),
                )
                .expect("append event");
        }
    }
    store.rebuild_all_states(500).expect("rebuild states");

    let actions = run_tick(
        &store,
        &RunOpts {
            dry_run: false,
            batch_size: 0,
        },
        now,
    )
    .expect("run tick");
    assert_eq!(
        serde_json::to_value(&actions).expect("actions JSON"),
        fixture["actions"]
    );
    let results = apply_tick_actions(&store, &actions, now).expect("apply tick");
    assert_eq!(results.len(), 4);
    assert!(
        results
            .iter()
            .all(|result| result.executed && result.error.is_empty())
    );

    let mut statement = store
        .connection()
        .prepare(
            "SELECT request_id, event_type, payload_json, source FROM request_events
         WHERE source = 'scheduler' ORDER BY id",
        )
        .expect("scheduler events query");
    let persisted: Vec<(i64, String, Value, String)> = statement
        .query_map([], |row| {
            let payload: String = row.get(2)?;
            Ok((
                row.get(0)?,
                row.get(1)?,
                serde_json::from_str(&payload).expect("payload JSON"),
                row.get(3)?,
            ))
        })
        .expect("scheduler events")
        .map(|row| row.expect("scheduler event"))
        .collect();
    let expected: Vec<(i64, String, Value, String)> = fixture["actions"]
        .as_array()
        .expect("actions")
        .iter()
        .map(|action| {
            (
                action["request_id"].as_i64().expect("request id"),
                action["event_type"]
                    .as_str()
                    .expect("event type")
                    .to_owned(),
                action["payload"].clone(),
                "scheduler".to_owned(),
            )
        })
        .collect();
    assert_eq!(persisted, expected);

    let projected: Vec<(i64, String, i64, i64)> = store
        .connection()
        .prepare(
            "SELECT request_id, current_status, reminders_sent, escalation_level
             FROM request_state ORDER BY request_id",
        )
        .expect("projection query")
        .query_map([], |row| {
            Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?))
        })
        .expect("projections")
        .map(|row| row.expect("projection"))
        .collect();
    assert_eq!(
        projected,
        [
            (1, "AWAITING_ACK".to_owned(), 2, 0),
            (2, "OVERDUE".to_owned(), 0, 1),
            (3, "ESCALATED".to_owned(), 0, 2),
            (4, "RE_SCAN_DUE".to_owned(), 0, 0),
        ]
    );
}

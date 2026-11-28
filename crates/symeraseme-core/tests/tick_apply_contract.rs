//! Tick transitions: scanning due requests and applying the resulting actions.
//!
//! Mirrors the transition half of Go's `internal/deadlines` conformance tests:
//! `RunTick` scans, `ApplyTickActions` appends each action's event with source
//! `scheduler`, dry runs write nothing, and the projections are rebuilt.

use serde_json::{Map, Value, json};
use symeraseme_core::deadlines::{RunOpts, apply_tick, apply_tick_actions, run_tick};
use symeraseme_core::storage::repository::Repository;
use symeraseme_core::storage::store::Store;
use symeraseme_core::storage::types::{EventType, Source};
use tempfile::tempdir;
use time::{Duration, OffsetDateTime};

fn pinned_now() -> OffsetDateTime {
    symeraseme_core::timeutil::parse_rfc3339("2026-08-06T12:00:00+00:00").expect("pinned instant")
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
    let sent_at =
        symeraseme_core::timeutil::format_rfc3339_auto_si(now - Duration::days(21), false);
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
    let sent_at =
        symeraseme_core::timeutil::format_rfc3339_auto_si(now - Duration::days(21), false);
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

fn golden_fixture() -> (Value, OffsetDateTime) {
    let fixture: Value = serde_json::from_str(include_str!(
        "../../../tests/fixtures/event-store/golden-tick.json"
    ))
    .expect("golden tick fixture");
    let now = symeraseme_core::timeutil::parse_rfc3339(fixture["now"].as_str().expect("now"))
        .expect("timestamp");
    (fixture, now)
}

fn projections(store: &Store) -> Vec<(i64, String, i64, i64)> {
    store
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
        .collect()
}

fn count_events(store: &Store, event_type: &str) -> i64 {
    store
        .connection()
        .query_row(
            "SELECT COUNT(*) FROM request_events WHERE event_type = ?",
            [event_type],
            |row| row.get(0),
        )
        .expect("event count")
}

fn tick(store: &Store, now: OffsetDateTime) -> Vec<symeraseme_core::deadlines::TickAction> {
    run_tick(
        store,
        &RunOpts {
            dry_run: false,
            batch_size: 0,
        },
        now,
    )
    .expect("run tick")
}

/// Post-cutover contract change #1362: a non-dry tick records only the state
/// transitions, queues the reminder and the DPA complaint as manual tasks, and
/// the completed task records the event the person actually performed.
#[test]
fn apply_tick_records_transitions_and_queues_human_actions_as_tasks() {
    let tree = tempdir().expect("temp dir");
    let store = Store::open(tree.path().join("db.sqlite")).expect("open store");
    let (_, now) = golden_fixture();
    seed_golden_histories(&store, now);

    let actions = tick(&store, now);
    let outcomes = apply_tick(&store, &actions, now);
    let mut summary: Vec<(i64, &str, &str, bool)> = outcomes
        .iter()
        .map(|outcome| {
            assert!(outcome.error.is_empty(), "{outcome:?}");
            (
                outcome.request_id,
                outcome.action.as_str(),
                outcome.event_type.as_str(),
                outcome.manual_task_id.is_some(),
            )
        })
        .collect();
    summary.sort();
    assert_eq!(
        summary,
        [
            (1, "send_reminder", "HUMAN_ACTION_REQUIRED", true),
            (2, "mark_overdue", "DEADLINE_REACHED", false),
            (3, "draft_dpa_complaint", "HUMAN_ACTION_REQUIRED", true),
            (4, "trigger_rescan", "RE_SCAN_TRIGGERED", false),
        ]
    );
    // Only the seeded reminder exists; nothing claims an unsent reminder or an
    // undrafted complaint.
    assert_eq!(count_events(&store, "REMINDER_SENT"), 1);
    assert_eq!(count_events(&store, "DPA_COMPLAINT_DRAFTED"), 0);
    assert_eq!(
        events_for(&store, 2).last(),
        Some(&("DEADLINE_REACHED".to_owned(), "scheduler".to_owned()))
    );
    assert_eq!(
        events_for(&store, 4).last(),
        Some(&("RE_SCAN_TRIGGERED".to_owned(), "scheduler".to_owned()))
    );
    assert_eq!(
        projections(&store),
        [
            (1, "AWAITING_USER_ACTION".to_owned(), 1, 0),
            (2, "OVERDUE".to_owned(), 0, 1),
            (3, "AWAITING_USER_ACTION".to_owned(), 0, 1),
            (4, "RE_SCAN_DUE".to_owned(), 0, 0),
        ]
    );
    let tasks = symeraseme_core::manualtasks::list(&store, &Default::default()).expect("tasks");
    let mut reasons: Vec<(Option<i64>, &str, &str)> = tasks
        .iter()
        .map(|task| (task.request_id, task.reason.as_str(), task.status.as_str()))
        .collect();
    reasons.sort();
    assert_eq!(
        reasons,
        [
            (Some(1), "reminder_due", "pending"),
            (Some(3), "dpa_escalation_due", "pending"),
        ]
    );

    // A second tick finds nothing new: the tasks hold their requests.
    assert!(tick(&store, now).is_empty());

    let task_for = |request_id| {
        outcomes
            .iter()
            .find(|outcome| outcome.request_id == request_id)
            .and_then(|outcome| outcome.manual_task_id)
            .expect("task id")
    };
    symeraseme_core::manualtasks::complete(&store, task_for(1), "sent", true, now)
        .expect("complete reminder")
        .expect("reminder task");
    symeraseme_core::manualtasks::complete(&store, task_for(3), "drafted", true, now)
        .expect("complete complaint")
        .expect("complaint task");
    let recorded = |request_id: i64, event_type: &str| -> (Value, String) {
        store
            .connection()
            .query_row(
                "SELECT payload_json, source FROM request_events
                 WHERE request_id = ? AND event_type = ? ORDER BY id DESC LIMIT 1",
                rusqlite::params![request_id, event_type],
                |row| {
                    let payload: String = row.get(0)?;
                    Ok((
                        serde_json::from_str(&payload).expect("payload"),
                        row.get(1)?,
                    ))
                },
            )
            .expect("recorded event")
    };
    assert_eq!(
        recorded(1, "REMINDER_SENT"),
        (
            json!({"count": 2, "days_since_sent": 21}),
            "user".to_owned()
        )
    );
    assert_eq!(
        recorded(3, "DPA_COMPLAINT_DRAFTED"),
        (json!({"days_since_deadline": 20}), "user".to_owned())
    );
    let after = projections(&store);
    assert_eq!(after[0], (1, "AWAITING_ACK".to_owned(), 2, 0));
    assert_eq!(after[2], (3, "ESCALATED".to_owned(), 0, 2));
    // The next reminder waits for its doubled threshold; the complaint is done.
    assert!(tick(&store, now).is_empty());
}

/// A reply that arrives while the reminder task is pending wins: completing
/// the task then adds only the note, never a `REMINDER_SENT` that would reset
/// the request to `AWAITING_ACK`.
#[test]
fn completing_a_tick_task_after_a_reply_records_only_the_note() {
    let tree = tempdir().expect("temp dir");
    let store = Store::open(tree.path().join("db.sqlite")).expect("open store");
    let (_, now) = golden_fixture();
    seed_golden_histories(&store, now);
    let actions = tick(&store, now);
    let outcomes = apply_tick(&store, &actions, now);
    let task = outcomes[0].manual_task_id.expect("reminder task");
    store
        .append_and_project(
            1,
            &EventType::Ack,
            &Map::new(),
            &Source::Inbox,
            now + Duration::hours(1),
        )
        .expect("ack");
    symeraseme_core::manualtasks::complete(&store, task, "sent", true, now + Duration::hours(2))
        .expect("complete")
        .expect("task");
    assert_eq!(count_events(&store, "REMINDER_SENT"), 1);
    assert_eq!(
        events_for(&store, 1).last(),
        Some(&("NOTE_ADDED".to_owned(), "user".to_owned()))
    );
    assert_eq!(projections(&store)[0].1, "ACK");
}

#[test]
fn golden_tick_histories_scan_and_persist_all_four_go_actions() {
    // The same event histories and fixture consumed by Go's
    // TestGoldenTickConformance and TestApplyTickActionsWritesEvents.
    let tree = tempdir().expect("temp dir");
    let store = Store::open(tree.path().join("db.sqlite")).expect("open store");
    let (fixture, now) = golden_fixture();
    seed_golden_histories(&store, now);

    let actions = tick(&store, now);
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

    assert_eq!(
        projections(&store),
        [
            (1, "AWAITING_ACK".to_owned(), 2, 0),
            (2, "OVERDUE".to_owned(), 0, 1),
            (3, "ESCALATED".to_owned(), 0, 2),
            (4, "RE_SCAN_DUE".to_owned(), 0, 0),
        ]
    );
}

/// Seeds the event histories of Go's TestGoldenTickConformance: one request
/// each with a due reminder, deadline, DPA complaint and re-scan.
fn seed_golden_histories(store: &Store, now: OffsetDateTime) {
    type History<'a> = (&'a str, &'a [(EventType, Value, i64)]);
    let histories: [History<'_>; 4] = [
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
        let id = Repository::new(store)
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
}

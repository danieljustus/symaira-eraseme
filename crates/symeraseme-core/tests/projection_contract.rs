use chrono::{TimeZone, Utc};
use rusqlite::Connection;
use serde_json::{Map, Value, json};
use std::{collections::BTreeMap, fs, path::PathBuf};
use symeraseme_core::storage::{
    EventType, ProjectionError, ProjectionState, Repository, Source, Store,
};
use tempfile::tempdir;

const GOLDEN_DATABASE: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../tests/fixtures/event-store/golden-campaign.db"
);
const GOLDEN_PROJECTION: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../tests/fixtures/event-store/golden-projection.json"
);

fn timestamp(year: i32, month: u32, day: u32, hour: u32, minute: u32) -> chrono::DateTime<Utc> {
    Utc.with_ymd_and_hms(year, month, day, hour, minute, 0)
        .single()
        .expect("valid contract timestamp")
}

fn empty_payload() -> Map<String, Value> {
    Map::new()
}

fn new_request(store: &Store, broker: &str) -> i64 {
    Repository::new(store)
        .create_removal_request(broker, "email", "projection-contract", "DE", "", "")
        .expect("create request")
}

fn copy_golden_database() -> (tempfile::TempDir, PathBuf) {
    let tree = tempdir().expect("create fixture tempdir");
    let database = tree.path().join("golden-campaign.db");
    fs::copy(GOLDEN_DATABASE, &database).expect("copy committed production fixture");
    (tree, database)
}

fn persisted_states(connection: &Connection) -> rusqlite::Result<BTreeMap<i64, Value>> {
    let mut statement = connection.prepare(
        "SELECT request_id, current_status, last_event_id, last_event_at,
                sent_at, acknowledged_at, resolved_at, deadline_at,
                next_action_at, reminders_sent, escalation_level
         FROM request_state ORDER BY request_id ASC",
    )?;
    statement
        .query_map([], |row| {
            let request_id: i64 = row.get(0)?;
            let state = json!({
                "acknowledged_at": row.get::<_, Option<String>>(5)?,
                "current_status": row.get::<_, String>(1)?,
                "deadline_at": row.get::<_, Option<String>>(7)?,
                "escalation_level": row.get::<_, i64>(10)?,
                "last_event_at": row.get::<_, Option<String>>(3)?,
                "last_event_id": row.get::<_, i64>(2)?,
                "next_action_at": row.get::<_, Option<String>>(8)?,
                "reminders_sent": row.get::<_, i64>(9)?,
                "request_id": request_id,
                "resolved_at": row.get::<_, Option<String>>(6)?,
                "sent_at": row.get::<_, Option<String>>(4)?,
            });
            Ok((request_id, state))
        })?
        .collect()
}

#[test]
fn committed_go_fixture_matches_projection_bytes_and_persisted_sqlite_state() {
    let (_tree, database) = copy_golden_database();
    let store = Store::open(&database).expect("open copied Go production fixture");
    let request_state_count: i64 = store
        .connection()
        .query_row("SELECT COUNT(*) FROM request_state", [], |row| row.get(0))
        .expect("read empty derived state");
    assert_eq!(
        request_state_count, 0,
        "fixture must ship without projections"
    );

    let expected: BTreeMap<String, Value> = serde_json::from_slice(
        &fs::read(GOLDEN_PROJECTION).expect("read Go-produced projection fixture"),
    )
    .expect("parse Go-produced projection fixture");

    for request_id in [1_i64, 2, 3] {
        let key = request_id.to_string();
        let state = store
            .rebuild_state(request_id)
            .expect("rebuild production fixture state");
        let got = serde_json::to_vec(&state).expect("serialize projection state");
        let want = serde_json::to_vec(&expected[&key]).expect("compact expected projection");
        assert_eq!(
            got, want,
            "projection JSON differs for request {request_id}"
        );

        let persisted = store
            .upsert_state(request_id)
            .expect("persist production fixture state");
        assert_eq!(persisted, state);
    }

    let persisted = persisted_states(store.connection()).expect("read persisted projections");
    assert_eq!(persisted.len(), expected.len());
    for (request_id, state) in persisted {
        let rebuilt = store
            .rebuild_state(request_id)
            .expect("rebuild state after persistence");
        assert_eq!(
            state,
            serde_json::to_value(rebuilt).expect("serialize rebuilt state")
        );
    }
}

#[test]
fn replay_uses_occurred_at_then_global_event_id_order() {
    let tree = tempdir().expect("create database tempdir");
    let store = Store::open(tree.path().join("ordering.db")).expect("open store");
    let request_id = new_request(&store, "ordered-broker");
    let payload = empty_payload();

    // The later event is inserted first. Replay must still apply SENT before
    // CONFIRMED because business time, not insertion order, is authoritative.
    let confirmed_id = store
        .append_event(
            request_id,
            &EventType::Confirmed,
            &payload,
            &Source::Inbox,
            timestamp(2026, 8, 3, 9, 0),
        )
        .expect("append later event");
    let sent_id = store
        .append_event(
            request_id,
            &EventType::Sent,
            &payload,
            &Source::System,
            timestamp(2026, 8, 1, 9, 0),
        )
        .expect("append backdated event");
    let state = store
        .rebuild_state(request_id)
        .expect("rebuild ordered state");
    assert_eq!(state.current_status, "CONFIRMED");
    assert_eq!(state.last_event_id, confirmed_id);
    assert_eq!(state.sent_at.as_deref(), Some("2026-08-01T09:00:00+00:00"));
    assert_eq!(
        state.last_event_at.as_deref(),
        Some("2026-08-03T09:00:00+00:00")
    );
    assert!(confirmed_id < sent_id);

    // Same-instant events use the global AUTOINCREMENT id as the tie-breaker.
    let tie_request = new_request(&store, "tie-broker");
    let tie = timestamp(2026, 8, 4, 9, 0);
    let sent_id = store
        .append_event(
            tie_request,
            &EventType::Sent,
            &payload,
            &Source::System,
            tie,
        )
        .expect("append tied SENT event");
    let confirmed_id = store
        .append_event(
            tie_request,
            &EventType::Confirmed,
            &payload,
            &Source::Inbox,
            tie,
        )
        .expect("append tied CONFIRMED event");
    let tie_state = store
        .rebuild_state(tie_request)
        .expect("rebuild tied state");
    assert_eq!(tie_state.current_status, "CONFIRMED");
    assert_eq!(tie_state.last_event_id, confirmed_id);
    assert!(sent_id < confirmed_id);

    let events = Repository::new(&store)
        .get_events(tie_request, 0)
        .expect("read replay-ordered events");
    assert_eq!(
        events.iter().map(|event| event.id).collect::<Vec<_>>(),
        vec![sent_id, confirmed_id]
    );
}

#[test]
fn invalid_append_is_rejected_without_side_effects_and_unknown_replay_is_skipped() {
    let tree = tempdir().expect("create database tempdir");
    let store = Store::open(tree.path().join("negative.db")).expect("open store");
    let request_id = new_request(&store, "negative-broker");
    let payload = empty_payload();
    let occurred_at = timestamp(2026, 8, 1, 8, 0);

    let invalid_event = EventType::Unknown("FUTURE_EVENT".to_owned());
    let error = store
        .append_event(
            request_id,
            &invalid_event,
            &payload,
            &Source::System,
            occurred_at,
        )
        .expect_err("unknown event type must be rejected");
    assert!(matches!(error, ProjectionError::UnknownEventType(value) if value == "FUTURE_EVENT"));

    let invalid_source = Source::Unknown("future-writer".to_owned());
    let error = store
        .append_event(
            request_id,
            &EventType::NoteAdded,
            &payload,
            &invalid_source,
            occurred_at,
        )
        .expect_err("unknown source must be rejected");
    assert!(matches!(error, ProjectionError::UnknownSource(value) if value == "future-writer"));

    let event_count: i64 = store
        .connection()
        .query_row(
            "SELECT COUNT(*) FROM request_events WHERE request_id = ?1",
            [request_id],
            |row| row.get(0),
        )
        .expect("count rejected append rows");
    assert_eq!(event_count, 0, "rejected appends must not write events");

    let sent_id = store
        .append_event(
            request_id,
            &EventType::Sent,
            &payload,
            &Source::System,
            occurred_at,
        )
        .expect("append valid event");
    let note_id = store
        .connection()
        .last_insert_rowid()
        .checked_add(1)
        .expect("event id range");
    store
        .connection()
        .execute(
            "INSERT INTO request_events
             (request_id, occurred_at, recorded_at, event_type, payload_json, source)
             VALUES (?1, ?2, ?2, 'NOTE_ADDED', '{}', 'future-writer')",
            rusqlite::params![request_id, "2026-08-02T08:00:00+00:00"],
        )
        .expect("insert known event with unknown replay source");
    let unknown_id = store
        .connection()
        .last_insert_rowid()
        .checked_add(1)
        .expect("event id range");
    store
        .connection()
        .execute(
            "INSERT INTO request_events
             (request_id, occurred_at, recorded_at, event_type, payload_json, source)
             VALUES (?1, ?2, ?2, 'FUTURE_EVENT', '{}', 'future-writer')",
            rusqlite::params![request_id, "2026-08-03T08:00:00+00:00"],
        )
        .expect("insert unknown historical event");
    let malformed_id = store
        .connection()
        .last_insert_rowid()
        .checked_add(1)
        .expect("event id range");
    store
        .connection()
        .execute(
            "INSERT INTO request_events
             (request_id, occurred_at, recorded_at, event_type, payload_json, source)
             VALUES (?1, ?2, ?2, 'SENT', '[]', 'system')",
            rusqlite::params![request_id, "2026-08-04T08:00:00+00:00"],
        )
        .expect("insert malformed historical payload");

    let state = store
        .rebuild_state(request_id)
        .expect("replay skips bad rows");
    assert_eq!(state.current_status, "AWAITING_ACK");
    assert_eq!(state.last_event_id, note_id);
    assert_ne!(state.last_event_id, unknown_id);
    assert_ne!(state.last_event_id, malformed_id);
    assert_eq!(sent_id + 1, note_id);
}

#[test]
fn append_and_project_rolls_back_event_when_projection_write_fails() {
    let tree = tempdir().expect("create database tempdir");
    let store = Store::open(tree.path().join("rollback.db")).expect("open store");
    let request_id = new_request(&store, "rollback-broker");
    store
        .connection()
        .execute_batch(
            "CREATE TRIGGER fail_projection_insert
             BEFORE INSERT ON request_state
             BEGIN
                 SELECT RAISE(ABORT, 'forced projection failure');
             END;",
        )
        .expect("install deterministic projection failure");

    let error = store
        .append_and_project(
            request_id,
            &EventType::Sent,
            &empty_payload(),
            &Source::System,
            timestamp(2026, 8, 1, 8, 0),
        )
        .expect_err("projection trigger must fail the transaction");
    assert!(matches!(error, ProjectionError::Database(_)));

    let events: i64 = store
        .connection()
        .query_row(
            "SELECT COUNT(*) FROM request_events WHERE request_id = ?1",
            [request_id],
            |row| row.get(0),
        )
        .expect("count events after rollback");
    let states: i64 = store
        .connection()
        .query_row(
            "SELECT COUNT(*) FROM request_state WHERE request_id = ?1",
            [request_id],
            |row| row.get(0),
        )
        .expect("count projections after rollback");
    assert_eq!(events, 0, "event append must roll back with projection");
    assert_eq!(states, 0, "failed projection must leave no partial row");
}

#[test]
fn projection_state_json_has_the_go_oracle_field_order() {
    let state = ProjectionState::new(9);
    assert_eq!(
        serde_json::to_string(&state).expect("serialize blank state"),
        r#"{"acknowledged_at":null,"current_status":"PLANNED","deadline_at":null,"escalation_level":0,"last_event_at":null,"last_event_id":0,"next_action_at":null,"reminders_sent":0,"request_id":9,"resolved_at":null,"sent_at":null}"#
    );
}

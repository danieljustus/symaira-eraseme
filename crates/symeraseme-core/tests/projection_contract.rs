use chrono::{TimeZone, Utc};
use rusqlite::{Connection, params};
use serde::Deserialize;
use serde_json::{Map, Value, json};
use sha2::{Digest, Sha256};
use std::{collections::BTreeMap, fs, path::PathBuf};
use symeraseme_core::storage::{
    EventType, ProjectionError, ProjectionState, Repository, Source, Store,
};
use tempfile::{TempDir, tempdir};

const GO_ORACLE_COMMIT: &str = "bf53346eec234929bedf0314b99e3da85dbb991b";
const INPUT_FIXTURE: &[u8] =
    include_bytes!("../../../tests/fixtures/event-store/projection-replay-input.json");
const CONTRACT_FIXTURE: &[u8] =
    include_bytes!("../../../tests/fixtures/event-store/projection-replay-contract.json");

#[derive(Debug, Deserialize, Eq, PartialEq)]
struct OracleProvenance {
    commit: String,
    generator: String,
    sources: Vec<SourceDigest>,
}

#[derive(Debug, Deserialize, Eq, PartialEq)]
struct SourceDigest {
    path: String,
    sha256: String,
}

#[derive(Debug, Deserialize)]
struct InputFixture {
    schema: String,
    oracle: OracleProvenance,
    cases: Vec<InputCase>,
}

#[derive(Debug, Deserialize)]
struct InputCase {
    name: String,
    database: Option<String>,
    request_ids: Vec<i64>,
    #[serde(default)]
    events: Vec<FixtureEvent>,
}

#[derive(Debug, Deserialize)]
struct FixtureEvent {
    id: i64,
    request_id: i64,
    occurred_at: String,
    recorded_at: String,
    event_type: String,
    payload_json: String,
    source: String,
}

#[derive(Debug, Deserialize)]
struct ContractFixture {
    schema: String,
    oracle: OracleProvenance,
    differential_harness: DifferentialHarness,
    cases: Vec<ExpectedCase>,
}

#[derive(Debug, Deserialize)]
struct DifferentialHarness {
    status: String,
    blocker: String,
}

#[derive(Debug, Deserialize)]
struct ExpectedCase {
    name: String,
    states: Vec<ExpectedState>,
}

#[derive(Debug, Deserialize)]
struct ExpectedState {
    request_id: i64,
    state: serde_json::Value,
    compact_json: String,
}

fn fixture_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../tests/fixtures/event-store")
}

fn repository_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn fixtures() -> (InputFixture, ContractFixture) {
    let input: InputFixture =
        serde_json::from_slice(INPUT_FIXTURE).expect("projection input fixture must be valid JSON");
    let contract: ContractFixture = serde_json::from_slice(CONTRACT_FIXTURE)
        .expect("Go-generated projection fixture must be valid JSON");

    assert_eq!(input.schema, "symeraseme.event-store.projection-input.v1");
    assert_eq!(
        contract.schema,
        "symeraseme.event-store.projection-contract.v1"
    );
    assert_eq!(input.oracle, contract.oracle);
    assert_eq!(input.oracle.commit, GO_ORACLE_COMMIT);
    assert_eq!(
        input.oracle.generator,
        "rust-tests/parity/oracle/projection_fixture/main.go"
    );
    assert_eq!(contract.differential_harness.status, "blocked");
    assert!(
        contract
            .differential_harness
            .blocker
            .contains("post-execution SQLite snapshots")
    );

    for source in &input.oracle.sources {
        let contents = fs::read(repository_root().join(&source.path))
            .unwrap_or_else(|error| panic!("read Go oracle source {}: {error}", source.path));
        assert_eq!(
            hex::encode(Sha256::digest(contents)),
            source.sha256,
            "Go oracle source drifted: {}",
            source.path
        );
    }
    (input, contract)
}

fn input_case<'a>(fixture: &'a InputFixture, name: &str) -> &'a InputCase {
    fixture
        .cases
        .iter()
        .find(|case| case.name == name)
        .unwrap_or_else(|| panic!("missing input case {name}"))
}

fn expected_case<'a>(fixture: &'a ContractFixture, name: &str) -> &'a ExpectedCase {
    fixture
        .cases
        .iter()
        .find(|case| case.name == name)
        .unwrap_or_else(|| panic!("missing Go-generated expected case {name}"))
}

fn open_case(case: &InputCase) -> (TempDir, Store) {
    let tree = tempfile::tempdir().expect("create isolated projection database");
    let database = tree.path().join("projection.db");
    if let Some(source) = &case.database {
        fs::copy(fixture_root().join(source), &database)
            .unwrap_or_else(|error| panic!("copy source fixture {source}: {error}"));
    }

    let store = Store::open(&database).expect("open isolated projection database");
    if case.database.is_none() {
        materialize_synthetic_case(&store, case);
    }
    (tree, store)
}

fn materialize_synthetic_case(store: &Store, case: &InputCase) {
    store
        .connection()
        .execute(
            "INSERT INTO campaigns (id, kind, notes) VALUES (?1, 'initial', '')",
            params!["projection-contract"],
        )
        .expect("create synthetic campaign");
    for request_id in &case.request_ids {
        store
            .connection()
            .execute(
                "INSERT INTO removal_requests
                 (id, broker_id, channel, campaign_id, jurisdiction, template_id, identity_snapshot_hash)
                 VALUES (?1, ?2, 'email', 'projection-contract', 'DE', '', '')",
                params![request_id, format!("projection-broker-{request_id}")],
            )
            .expect("create synthetic request");
    }
    for event in &case.events {
        store
            .connection()
            .execute(
                "INSERT INTO request_events
                 (id, request_id, occurred_at, recorded_at, event_type, payload_json, source)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
                params![
                    event.id,
                    event.request_id,
                    event.occurred_at,
                    event.recorded_at,
                    event.event_type,
                    event.payload_json,
                    event.source,
                ],
            )
            .expect("insert Go fixture event row");
    }
}

fn assert_case_matches_go_fixture(name: &str, store: &Store) {
    let (input, contract) = fixtures();
    let input_case = input_case(&input, name);
    let expected = expected_case(&contract, name);
    assert_eq!(expected.states.len(), input_case.request_ids.len());

    for request_id in &input_case.request_ids {
        let actual = store
            .rebuild_state(*request_id)
            .unwrap_or_else(|error| panic!("rebuild request {request_id}: {error}"));
        let expected = expected
            .states
            .iter()
            .find(|state| state.request_id == *request_id)
            .unwrap_or_else(|| panic!("missing Go-generated state for request {request_id}"));
        assert!(
            expected.state.is_object(),
            "Go fixture state must be an object"
        );
        assert!(
            !expected.compact_json.is_empty(),
            "Go fixture compact state must be nonempty"
        );
        assert_eq!(
            serde_json::to_vec(&actual).expect("serialize Rust projection"),
            expected.compact_json.as_bytes(),
            "compact projection bytes differ for {name}/request/{request_id}"
        );
    }
}

#[test]
fn golden_projection_replays_go_fixture_byte_for_byte() {
    let (input, _) = fixtures();
    let case = input_case(&input, "golden_projection");
    let (_tree, store) = open_case(case);
    let count: i64 = store
        .connection()
        .query_row("SELECT COUNT(*) FROM request_state", [], |row| row.get(0))
        .expect("read derived-state row count");
    assert_eq!(count, 0, "the source fixture must ship without projections");
    assert_case_matches_go_fixture("golden_projection", &store);
}

#[test]
fn empty_request_replays_the_go_blank_state() {
    let (input, _) = fixtures();
    let case = input_case(&input, "empty_request");
    let (_tree, store) = open_case(case);
    assert_case_matches_go_fixture("empty_request", &store);
}

#[test]
fn same_occurred_at_uses_global_id_as_the_tie_breaker() {
    let (input, _) = fixtures();
    let case = input_case(&input, "same_occurred_at_tie");
    let (_tree, store) = open_case(case);
    let events = Repository::new(&store)
        .get_events(1, 0)
        .expect("read replay-ordered tie events");
    assert_eq!(
        events.iter().map(|event| event.id).collect::<Vec<_>>(),
        vec![20, 30],
        "SQLite must order tied timestamps by global event id"
    );
    assert_case_matches_go_fixture("same_occurred_at_tie", &store);
}

#[test]
fn occurred_at_precedes_global_id_when_they_disagree() {
    let (input, _) = fixtures();
    let case = input_case(&input, "occurred_at_precedes_id");
    let (_tree, store) = open_case(case);
    let events = Repository::new(&store)
        .get_events(1, 0)
        .expect("read replay-ordered crossed events");
    assert_eq!(
        events.iter().map(|event| event.id).collect::<Vec<_>>(),
        vec![50, 40],
        "SQLite must order by occurred_at before global event id"
    );
    assert_case_matches_go_fixture("occurred_at_precedes_id", &store);
}

#[test]
fn numeric_payloads_follow_go_coercion_and_clamping() {
    let (input, _) = fixtures();
    let case = input_case(&input, "numeric_payload_coercion");
    let (_tree, store) = open_case(case);
    assert_case_matches_go_fixture("numeric_payload_coercion", &store);
}

#[test]
fn replay_skips_malformed_and_direct_fold_error_rows() {
    let (input, _) = fixtures();
    let case = input_case(&input, "replay_errors_are_skipped");
    let (_tree, store) = open_case(case);
    let events = Repository::new(&store)
        .get_events(1, 0)
        .expect("read replay-error rows");
    assert_eq!(
        events.iter().map(|event| event.id).collect::<Vec<_>>(),
        vec![15, 10, 13, 14],
        "timestamp and non-object-payload failures must be omitted before folding"
    );
    assert_case_matches_go_fixture("replay_errors_are_skipped", &store);
}

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

fn copy_golden_database() -> (TempDir, PathBuf) {
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
fn invalid_append_is_rejected_without_side_effects_and_replay_boundaries_are_preserved() {
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
    assert_eq!(state.last_event_id, unknown_id);
    assert_eq!(
        state.last_event_at.as_deref(),
        Some("2026-08-03T08:00:00+00:00")
    );
    assert_eq!(state.sent_at.as_deref(), Some("2026-08-01T08:00:00+00:00"));
    assert_eq!(
        state.deadline_at.as_deref(),
        Some("2026-08-31T08:00:00+00:00")
    );
    assert_eq!(state.acknowledged_at, None);
    assert_eq!(state.resolved_at, None);
    assert_eq!(state.next_action_at, None);
    assert_eq!(state.reminders_sent, 0);
    assert_eq!(state.escalation_level, 0);
    assert_eq!(state.request_id, request_id);
    assert!(unknown_id > note_id);
    assert!(malformed_id > unknown_id);
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

#[test]
fn rebuild_all_states_updates_only_stale_projections() {
    let tree = tempdir().expect("create database tempdir");
    let store = Store::open(tree.path().join("rebuild-all.db")).expect("open store");
    let req1 = new_request(&store, "broker-1");
    let req2 = new_request(&store, "broker-2");
    let payload = empty_payload();

    store
        .append_event(
            req1,
            &EventType::Sent,
            &payload,
            &Source::System,
            timestamp(2026, 8, 1, 8, 0),
        )
        .expect("append req1");
    store
        .append_event(
            req2,
            &EventType::Sent,
            &payload,
            &Source::System,
            timestamp(2026, 8, 2, 8, 0),
        )
        .expect("append req2");

    let rebuilt = store.rebuild_all_states(10).expect("rebuild all");
    assert_eq!(rebuilt, 2);

    let rebuilt_again = store.rebuild_all_states(10).expect("rebuild all again");
    assert_eq!(rebuilt_again, 0);

    store
        .append_event(
            req1,
            &EventType::Ack,
            &payload,
            &Source::Inbox,
            timestamp(2026, 8, 3, 8, 0),
        )
        .expect("append ack to req1");

    let rebuilt_stale = store.rebuild_all_states(10).expect("rebuild stale");
    assert_eq!(rebuilt_stale, 1);
}

#[test]
fn all_projections_collects_sorted_request_states() {
    let tree = tempdir().expect("create database tempdir");
    let store = Store::open(tree.path().join("all-projections.db")).expect("open store");
    let req1 = new_request(&store, "broker-1");
    let req2 = new_request(&store, "broker-2");

    let projections = store.all_projections().expect("get all projections");
    assert_eq!(projections.len(), 2);
    assert!(projections.contains_key(&req1));
    assert!(projections.contains_key(&req2));
    assert_eq!(projections[&req1].current_status, "PLANNED");
    assert_eq!(projections[&req2].current_status, "PLANNED");
}

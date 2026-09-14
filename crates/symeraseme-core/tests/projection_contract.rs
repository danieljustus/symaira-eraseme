use rusqlite::params;
use serde::Deserialize;
use sha2::{Digest, Sha256};
use std::{fs, path::PathBuf};
use symeraseme_core::storage::{Repository, Store};
use tempfile::TempDir;

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

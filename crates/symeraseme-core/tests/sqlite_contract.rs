use std::{
    fs,
    path::{Path, PathBuf},
    process::Command,
};

use rusqlite::Connection;
use serde_json::json;
use sha2::{Digest, Sha256};
use symeraseme_core::storage::{
    EventType,
    repository::{ListRemovalRequestsOptions, Repository},
    store::Store,
};
use tempfile::tempdir;

const GOLDEN_DATABASE_PATH: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../tests/fixtures/event-store/golden-campaign.db"
);
const GOLDEN_DATABASE: &[u8] =
    include_bytes!("../../../tests/fixtures/event-store/golden-campaign.db");
// This record pins the crypto-vector input bytes; immutable DB provenance is
// validated independently below because this generator did not create the DB.
const GOLDEN_DATABASE_CRYPTO_PROVENANCE: &[u8] =
    include_bytes!("../../../tests/fixtures/event-store/crypto/provenance.json");
const PYTHON_FINAL_TAG_OBJECT: &str = "9381296d68c7d674b102196f6d6b25a050bd6a66";
const GOLDEN_DATABASE_SOURCE_PATH: &str = "tests/fixtures/event-store/golden-campaign.db";
const GOLDEN_DATABASE_SHA256: &str =
    "595a4840dbe6a52324b40778c53016b0c809e01451ba4fa5f20c3fd3447e0120";
const GOLDEN_DATABASE_GIT_BLOB: &str = "83d02232e9c1d146ef55c3cee058c646fe1c8c28";
const PYTHON_FIXTURE_ORIGIN_REVISION: &str = "993f9a652efd4470985c29b15fff0d8efb8d72c7";
const PYTHON_FINAL_ARCHIVE_TAG: &str = "python-final";
const PYTHON_FINAL_ARCHIVE_COMMIT: &str = "538f97070202fb1577db3c81d88ab908bd8671eb";
const PYTHON_FIXTURE_GENERATOR_PATH: &str = "scripts/generate_event_store_fixture.py";
const PYTHON_FIXTURE_GENERATOR_GIT_BLOB: &str = "28456fcfe96a27a32ee9be8eeb84089069f49f8e";
const PYTHON_FIXTURE_GENERATOR_SHA256: &str =
    "2dff9a2ab364a4ec80674175fe8731ab9c6061a83960d50b04f184777be82e84";
const GO_EVENT_STORE_SOURCE_REVISION: &str = "18c5dc1109e8824265f862f14584697d08152e40";
const GO_EVENT_STORE_SOURCE_PATH: &str = "internal/eventstore/store.go";
const GO_EVENT_STORE_SOURCE_GIT_BLOB: &str = "2201ac9ffd929f871fbd54f9e7e77eb59749c5a6";
const GO_EVENT_STORE_SOURCE_SHA256: &str =
    "fd1dd416606f29aa4726a62ffe6ad83ef9d7c9eb6c6f42d81e87987913968df3";
const GO_EVENT_STORE_SOURCE: &[u8] = include_bytes!("../../../internal/eventstore/store.go");
const STORAGE_ORACLE_SCHEMA: &str = "symaira-eraseme.storage-parity.v1";

fn golden_database() -> &'static [u8] {
    let provenance: serde_json::Value = serde_json::from_slice(GOLDEN_DATABASE_CRYPTO_PROVENANCE)
        .expect("the committed crypto provenance must be valid JSON");
    validate_golden_database(GOLDEN_DATABASE, &provenance)
        .expect("the source-bound golden database must match immutable provenance");
    GOLDEN_DATABASE
}

fn validate_golden_database(database: &[u8], provenance: &serde_json::Value) -> Result<(), String> {
    validate_crypto_fixture_metadata(database, provenance)?;
    validate_immutable_database_sources(database)
}

fn validate_crypto_fixture_metadata(
    database: &[u8],
    provenance: &serde_json::Value,
) -> Result<(), String> {
    if provenance["generator"].as_str() != Some("scripts/generate-crypto-fixtures.py") {
        return Err("the committed crypto provenance has an unexpected generator path".into());
    }
    if provenance["python_final_commit"].as_str() != Some(PYTHON_FINAL_TAG_OBJECT) {
        return Err(
            "the committed crypto provenance has an unexpected python-final tag object".into(),
        );
    }
    let expected_hash = provenance["golden_campaign_sha256"]
        .as_str()
        .ok_or("the golden-database provenance must record a SHA-256")?;
    let expected_size = provenance["golden_campaign_size"]
        .as_u64()
        .ok_or("the golden-database provenance must record a byte size")?;
    if database.len() as u64 != expected_size {
        return Err(format!(
            "golden database size {} does not match provenance size {expected_size}",
            database.len()
        ));
    }
    let actual_hash = hex::encode(Sha256::digest(database));
    if actual_hash != expected_hash {
        return Err(format!(
            "golden database SHA-256 {actual_hash} does not match provenance {expected_hash}"
        ));
    }
    Ok(())
}

fn validate_immutable_database_sources(database: &[u8]) -> Result<(), String> {
    let immutable_fixture =
        immutable_git_blob(PYTHON_FINAL_TAG_OBJECT, GOLDEN_DATABASE_SOURCE_PATH)?;
    verify_sha256(
        "immutable Python golden database fixture",
        &immutable_fixture,
        GOLDEN_DATABASE_SHA256,
    )?;
    if immutable_fixture != database {
        return Err("golden database does not match immutable Python fixture source".into());
    }

    let immutable_generator =
        immutable_git_blob(PYTHON_FINAL_TAG_OBJECT, PYTHON_FIXTURE_GENERATOR_PATH)?;
    verify_sha256(
        "immutable Python fixture generator",
        &immutable_generator,
        PYTHON_FIXTURE_GENERATOR_SHA256,
    )?;

    let immutable_go_source =
        immutable_git_blob(GO_EVENT_STORE_SOURCE_REVISION, GO_EVENT_STORE_SOURCE_PATH)?;
    verify_sha256(
        "immutable Go event-store source",
        &immutable_go_source,
        GO_EVENT_STORE_SOURCE_SHA256,
    )?;
    if immutable_go_source != GO_EVENT_STORE_SOURCE {
        return Err(
            "working-tree Go event-store source differs from immutable Go provenance".into(),
        );
    }
    Ok(())
}

fn immutable_git_blob(revision: &str, source_path: &str) -> Result<Vec<u8>, String> {
    let repository = repository_root();
    let output = Command::new("git")
        .args(["show", &format!("{revision}:{source_path}")])
        .current_dir(repository)
        .output()
        .map_err(|error| {
            format!("cannot read immutable source provenance {revision}:{source_path}: {error}")
        })?;
    if !output.status.success() {
        return Err(format!(
            "immutable source provenance {revision}:{source_path} is unavailable"
        ));
    }
    Ok(output.stdout)
}

fn verify_sha256(label: &str, contents: &[u8], expected: &str) -> Result<(), String> {
    let actual = hex::encode(Sha256::digest(contents));
    if actual != expected {
        return Err(format!(
            "{label} SHA-256 {actual} does not match immutable provenance"
        ));
    }
    Ok(())
}

fn repository_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()
        .expect("the sqlite contract test must run from a checkout with a repository root")
}

fn canonical_sql(sql: &str) -> String {
    sql.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn run_go_storage_oracle() -> serde_json::Value {
    let build_cache = tempdir().expect("create isolated Go build cache");
    let repository = repository_root();
    let test_output = Command::new("go")
        .args([
            "test",
            "-tags",
            "storage_oracle",
            "./rust-tests/parity/oracle/storage",
        ])
        .current_dir(&repository)
        .env("GOCACHE", build_cache.path())
        .env("GOTOOLCHAIN", "go1.26.6")
        .env("GOWORK", "off")
        .output()
        .expect("run the committed Go storage oracle negative controls");
    assert!(
        test_output.status.success(),
        "Go storage oracle negative controls failed: {}",
        String::from_utf8_lossy(&test_output.stderr)
    );

    let output = Command::new("go")
        .args([
            "run",
            "-tags",
            "storage_oracle",
            "./rust-tests/parity/oracle/storage",
        ])
        .current_dir(repository)
        .env("GOCACHE", build_cache.path())
        .env("GOTOOLCHAIN", "go1.26.6")
        .env("GOWORK", "off")
        .output()
        .expect("run the committed Go storage oracle");
    assert!(
        output.status.success(),
        "Go storage oracle failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    serde_json::from_slice(&output.stdout).expect("Go storage oracle must emit valid JSON")
}

fn oracle_schema_contract(snapshot: &serde_json::Value) -> Vec<(String, String, String)> {
    snapshot["schema"]
        .as_array()
        .expect("Go storage oracle schema must be an array")
        .iter()
        .map(|entry| {
            (
                entry["type"]
                    .as_str()
                    .expect("Go storage oracle schema type must be a string")
                    .to_owned(),
                entry["name"]
                    .as_str()
                    .expect("Go storage oracle schema name must be a string")
                    .to_owned(),
                canonical_sql(
                    entry["sql"]
                        .as_str()
                        .expect("Go storage oracle schema SQL must be a string"),
                ),
            )
        })
        .collect()
}

fn assert_go_oracle_snapshot(
    snapshot: &serde_json::Value,
    connection: &Connection,
    row_counts: serde_json::Value,
) {
    assert_eq!(snapshot["user_version"], json!(2));
    assert_eq!(
        snapshot["pragmas"],
        json!({
            "busy_timeout": 5_000,
            "foreign_keys": 1,
            "journal_mode": "wal",
        })
    );
    assert_eq!(
        oracle_schema_contract(snapshot),
        schema_contract(connection)
    );
    assert_eq!(snapshot["table_row_counts"], row_counts);
}

fn pragma_value<T>(connection: &Connection, name: &str) -> T
where
    T: rusqlite::types::FromSql,
{
    connection
        .pragma_query_value(None, name, |row| row.get(0))
        .expect("read SQLite pragma")
}

fn schema_contract(connection: &Connection) -> Vec<(String, String, String)> {
    connection
        .prepare(
            "SELECT type, name, COALESCE(sql, '')
             FROM sqlite_master
             WHERE sql IS NOT NULL
             ORDER BY type, name",
        )
        .expect("prepare schema contract query")
        .query_map([], |row| {
            let sql = row.get::<_, String>(2)?;
            let canonical_sql = canonical_sql(&sql);
            Ok((row.get(0)?, row.get(1)?, canonical_sql))
        })
        .expect("query schema contract")
        .collect::<rusqlite::Result<Vec<_>>>()
        .expect("collect schema contract")
}

#[test]
fn fresh_store_matches_go_schema_and_connection_contract() {
    let tree = tempdir().expect("create isolated database directory");
    let database = tree.path().join("nested").join("symeraseme.db");
    let store = Store::open(&database).expect("open fresh store");

    assert_eq!(
        store.user_version().expect("read schema version"),
        2,
        "fresh store must retain the Go v2 on-disk contract"
    );
    assert_eq!(
        pragma_value::<i64>(store.connection(), "busy_timeout"),
        5_000
    );
    assert_eq!(pragma_value::<i64>(store.connection(), "foreign_keys"), 1);
    assert_eq!(
        pragma_value::<String>(store.connection(), "journal_mode").to_ascii_lowercase(),
        "wal"
    );

    let table_names = store
        .connection()
        .prepare("SELECT name FROM sqlite_master WHERE type = 'table' ORDER BY name")
        .expect("prepare schema query")
        .query_map([], |row| row.get::<_, String>(0))
        .expect("query schema")
        .collect::<rusqlite::Result<Vec<_>>>()
        .expect("collect table names");
    assert_eq!(
        table_names,
        vec![
            "campaigns",
            "imap_state",
            "inbox_replies",
            "manual_tasks",
            "removal_requests",
            "reply_drafts",
            "request_events",
            "request_state",
            "sqlite_sequence",
        ]
    );

    let index_names = store
        .connection()
        .prepare(
            "SELECT name FROM sqlite_master
             WHERE type = 'index' AND sql IS NOT NULL
             ORDER BY name",
        )
        .expect("prepare index query")
        .query_map([], |row| row.get::<_, String>(0))
        .expect("query indexes")
        .collect::<rusqlite::Result<Vec<_>>>()
        .expect("collect index names");
    assert_eq!(
        index_names,
        vec![
            "idx_events_occurred_at",
            "idx_events_request",
            "idx_inbox_replies_classified",
            "idx_inbox_replies_request",
            "idx_manual_tasks_request",
            "idx_manual_tasks_status",
            "idx_removal_requests_broker",
            "idx_removal_requests_campaign",
            "idx_removal_requests_jurisdiction",
            "idx_request_state_next_action",
        ]
    );
}

#[test]
fn fresh_schema_sql_matches_the_committed_production_fixture() {
    let original = golden_database();
    let tree = tempdir().expect("create isolated database directory");
    let fixture_copy = tree.path().join("golden-campaign.db");
    fs::write(&fixture_copy, original).expect("copy golden database");

    let fresh = Store::open(tree.path().join("fresh.db")).expect("open fresh store");
    let fixture = Store::open(fixture_copy).expect("open fixture copy");
    assert_eq!(
        schema_contract(fresh.connection()),
        schema_contract(fixture.connection())
    );
}

#[test]
fn go_storage_oracle_binds_immutable_sources_and_matches_db_001_to_db_003() {
    let oracle = run_go_storage_oracle();
    assert_eq!(
        oracle["provenance"],
        json!({
            "schema": STORAGE_ORACLE_SCHEMA,
            "go_source_revision": GO_EVENT_STORE_SOURCE_REVISION,
            "go_source_path": GO_EVENT_STORE_SOURCE_PATH,
            "go_source_git_blob": GO_EVENT_STORE_SOURCE_GIT_BLOB,
            "go_source_sha256": GO_EVENT_STORE_SOURCE_SHA256,
            "fixture_origin_revision": PYTHON_FIXTURE_ORIGIN_REVISION,
            "fixture_archive_tag": PYTHON_FINAL_ARCHIVE_TAG,
            "fixture_archive_tag_oid": PYTHON_FINAL_TAG_OBJECT,
            "fixture_archive_commit": PYTHON_FINAL_ARCHIVE_COMMIT,
            "fixture_generator_path": PYTHON_FIXTURE_GENERATOR_PATH,
            "fixture_generator_git_blob": PYTHON_FIXTURE_GENERATOR_GIT_BLOB,
            "fixture_generator_sha256": PYTHON_FIXTURE_GENERATOR_SHA256,
            "fixture_path": GOLDEN_DATABASE_SOURCE_PATH,
            "fixture_git_blob": GOLDEN_DATABASE_GIT_BLOB,
            "fixture_sha256": GOLDEN_DATABASE_SHA256,
            "fixture_size": GOLDEN_DATABASE.len(),
        })
    );

    let tree = tempdir().expect("create isolated database directory");
    let fresh = Store::open(tree.path().join("fresh.db")).expect("open fresh Rust store");
    assert_go_oracle_snapshot(
        &oracle["fresh"],
        fresh.connection(),
        json!({
            "campaigns": 0,
            "removal_requests": 0,
            "request_events": 0,
            "request_state": 0,
        }),
    );

    let fixture_path = tree.path().join("golden-campaign.db");
    fs::write(&fixture_path, golden_database()).expect("copy golden database");
    let fixture = Store::open(fixture_path).expect("open copied golden Rust store");
    assert_go_oracle_snapshot(
        &oracle["golden"],
        fixture.connection(),
        json!({
            "campaigns": 1,
            "removal_requests": 3,
            "request_events": 12,
            "request_state": 0,
        }),
    );
}

#[test]
fn typed_repository_queries_preserve_go_filters_pagination_and_event_buckets() {
    let original = golden_database();
    let tree = tempdir().expect("create isolated database directory");
    let database = tree.path().join("golden-campaign.db");
    fs::write(&database, original).expect("copy golden database");

    let store = Store::open(&database).expect("open copied golden database");
    let repository = Repository::new(&store);

    let request = repository
        .get_removal_request(2)
        .expect("get request")
        .expect("request exists");
    assert_eq!(request.broker_id, "golden-email-eu");
    assert_eq!(request.channel, "email");
    assert_eq!(
        repository
            .get_removal_request(999)
            .expect("query missing request"),
        None
    );

    assert_eq!(
        repository
            .count_removal_requests(Some("golden-campaign"), None)
            .expect("count requests"),
        3
    );
    let page = repository
        .list_removal_requests(ListRemovalRequestsOptions {
            campaign_id: Some("golden-campaign".to_owned()),
            limit: Some(1),
            offset: Some(1),
            ..Default::default()
        })
        .expect("list request page");
    assert_eq!(page.len(), 1);
    assert_eq!(page[0].id, 2);

    let events_after_two = repository.get_events(1, 2).expect("get events after id");
    assert_eq!(
        events_after_two
            .iter()
            .map(|event| event.id)
            .collect::<Vec<_>>(),
        vec![3, 4]
    );

    let sent = repository
        .get_events_for_requests(&[1, 2], Some(&EventType::Sent))
        .expect("get filtered event buckets");
    assert_eq!(sent[&1].len(), 1);
    assert_eq!(sent[&2].len(), 1);
    assert_eq!(sent[&1][0].event_type, EventType::Sent);
}

#[test]
fn golden_fixture_is_read_from_a_copy_with_nullable_state_fields() {
    let original = golden_database();
    let tree = tempdir().expect("create isolated database directory");
    let database = tree.path().join("golden-campaign.db");
    fs::write(&database, original).expect("copy golden database");

    let store = Store::open(&database).expect("open copied golden database");
    let repository = Repository::new(&store);
    assert_eq!(
        store.user_version().expect("read migrated version"),
        2,
        "opening the v1 fixture must migrate it to the Go v2 on-disk contract"
    );

    let campaigns = repository.list_campaigns().expect("list campaigns");
    assert_eq!(campaigns.len(), 1);
    assert_eq!(campaigns[0].id, "golden-campaign");
    assert_eq!(
        campaigns[0].notes.as_deref(),
        Some("golden fixture campaign")
    );

    let requests = repository
        .list_removal_requests(Default::default())
        .expect("list removal requests");
    assert_eq!(requests.len(), 3);
    assert!(
        requests
            .iter()
            .all(|request| request.current_status.is_none())
    );
    assert!(requests.iter().all(|request| request.sent_at.is_none()));

    let events = repository.get_events(1, 0).expect("read request events");
    assert_eq!(events.len(), 4);
    assert_eq!(events[0].id, 1);
    assert_eq!(events[1].id, 2);
    assert_eq!(events[2].id, 3);
    assert_eq!(events[3].id, 4);

    drop(store);
    assert_eq!(
        fs::read(GOLDEN_DATABASE_PATH).expect("re-read committed fixture"),
        original
    );
}

#[test]
fn event_queries_accept_the_three_go_timestamp_layouts_and_keep_nulls() {
    let original = golden_database();
    let tree = tempdir().expect("create isolated database directory");
    let database = tree.path().join("timestamp-layouts.db");
    fs::write(&database, original).expect("copy golden database");

    let store = Store::open(&database).expect("open copied golden database");
    store
        .connection()
        .execute(
            "UPDATE request_events
             SET occurred_at = CASE id
                 WHEN 1 THEN '2026-08-01T08:00:00'
                 WHEN 2 THEN '2026-08-01T08:05:00+00:00'
                 WHEN 3 THEN '2026-08-03 09:12:00'
             END,
             recorded_at = CASE id
                 WHEN 1 THEN '2026-08-01T08:00:00'
                 WHEN 2 THEN '2026-08-01 08:00:00+00:00'
                 WHEN 3 THEN '2026-08-01T08:00:00'
             END
             WHERE id IN (1, 2, 3)",
            [],
        )
        .expect("write timestamp-layout cases to temporary copy");

    let repository = Repository::new(&store);
    let events = repository
        .get_events(1, 0)
        .expect("read timestamp variants");
    assert_eq!(events.len(), 4);
    assert_eq!(
        events[0].occurred_at.to_rfc3339(),
        "2026-08-01T08:00:00+00:00"
    );
    assert_eq!(
        events[1].occurred_at.to_rfc3339(),
        "2026-08-01T08:05:00+00:00"
    );
    assert_eq!(
        events[2].occurred_at.to_rfc3339(),
        "2026-08-03T09:12:00+00:00"
    );
    assert_eq!(
        repository
            .get_removal_request(1)
            .expect("read request")
            .expect("request exists")
            .created_at,
        "2026-08-01T08:00:00"
    );
}

#[test]
fn golden_database_provenance_rejects_a_mutated_fixture() {
    let provenance: serde_json::Value = serde_json::from_slice(GOLDEN_DATABASE_CRYPTO_PROVENANCE)
        .expect("the committed crypto provenance must be valid JSON");
    let mut mutated = golden_database().to_vec();
    mutated[0] ^= 1;

    let error = validate_golden_database(&mutated, &provenance)
        .expect_err("a mutated golden database must be rejected by provenance validation");
    assert!(error.contains("SHA-256"), "unexpected rejection: {error}");
}

#[test]
fn golden_database_provenance_rejects_a_co_mutated_fixture_and_metadata() {
    let mut provenance: serde_json::Value =
        serde_json::from_slice(GOLDEN_DATABASE_CRYPTO_PROVENANCE)
            .expect("the committed crypto provenance must be valid JSON");
    let mut mutated = golden_database().to_vec();
    mutated[0] ^= 1;
    provenance["golden_campaign_sha256"] =
        serde_json::Value::String(hex::encode(Sha256::digest(&mutated)));
    provenance["golden_campaign_size"] = serde_json::Value::from(mutated.len() as u64);

    let error = validate_golden_database(&mutated, &provenance)
        .expect_err("co-mutated fixture metadata must not replace immutable source provenance");
    assert!(
        error.contains("immutable"),
        "unexpected rejection for co-mutated fixture metadata: {error}"
    );
}

#[test]
fn schema_contract_rejects_case_changed_string_literals() {
    let upper = Connection::open_in_memory().expect("open upper-case schema database");
    let lower = Connection::open_in_memory().expect("open lower-case schema database");
    upper
        .execute_batch(
            "CREATE TABLE request_state (current_status TEXT NOT NULL DEFAULT 'PLANNED')",
        )
        .expect("create upper-case literal schema");
    lower
        .execute_batch(
            "CREATE TABLE request_state (current_status TEXT NOT NULL DEFAULT 'planned')",
        )
        .expect("create lower-case literal schema");

    assert_ne!(
        schema_contract(&upper),
        schema_contract(&lower),
        "case changes inside SQL string literals are semantic differences"
    );
}

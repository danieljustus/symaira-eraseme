use std::fs;

use rusqlite::Connection;
use symeraseme_core::storage::{
    EventType,
    repository::{ListRemovalRequestsOptions, Repository},
    store::{SCHEMA_VERSION, Store},
};
use tempfile::tempdir;

const GOLDEN_DATABASE: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../tests/fixtures/event-store/golden-campaign.db"
);

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
            let canonical_sql = row
                .get::<_, String>(2)?
                .split_whitespace()
                .collect::<Vec<_>>()
                .join(" ")
                .to_ascii_lowercase();
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
        SCHEMA_VERSION
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
    let original = fs::read(GOLDEN_DATABASE).expect("read committed golden database");
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
fn typed_repository_queries_preserve_go_filters_pagination_and_event_buckets() {
    let original = fs::read(GOLDEN_DATABASE).expect("read committed golden database");
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
    let original = fs::read(GOLDEN_DATABASE).expect("read committed golden database");
    let tree = tempdir().expect("create isolated database directory");
    let database = tree.path().join("golden-campaign.db");
    fs::write(&database, &original).expect("copy golden database");

    let store = Store::open(&database).expect("open copied golden database");
    let repository = Repository::new(&store);
    assert_eq!(
        store.user_version().expect("read migrated version"),
        SCHEMA_VERSION
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
        fs::read(GOLDEN_DATABASE).expect("re-read committed fixture"),
        original
    );
}

#[test]
fn event_queries_accept_the_three_go_timestamp_layouts_and_keep_nulls() {
    let original = fs::read(GOLDEN_DATABASE).expect("read committed golden database");
    let tree = tempdir().expect("create isolated database directory");
    let database = tree.path().join("timestamp-layouts.db");
    fs::write(&database, &original).expect("copy golden database");

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

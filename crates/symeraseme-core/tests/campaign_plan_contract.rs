//! DOM-002 plan conformance against the shared golden fixture.
//!
//! Mirrors Go's `TestGoldenPlanConformance`: the same mini registry (three
//! broker YAMLs plus a manifest), the same deliberately missing identity
//! profile (empty snapshot hash) and the same golden
//! `tests/fixtures/event-store/golden-plan.json` — compared down to the
//! removal-request rows and the `PLANNED` event payloads.

use std::fs;
use std::path::Path;

use chrono::{DateTime, Utc};
use serde_json::Value;
use symeraseme_core::campaign::{PlanOpts, plan_campaign};
use symeraseme_core::registry::load_from_dir;
use symeraseme_core::storage::store::Store;
use tempfile::tempdir;

const FIXTURE_DIR: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../tests/fixtures/registry-contract"
);
const GOLDEN_PLAN_PATH: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../tests/fixtures/event-store/golden-plan.json"
);

fn pinned_now() -> DateTime<Utc> {
    DateTime::parse_from_rfc3339("2026-08-06T12:00:00+00:00")
        .expect("pinned instant")
        .with_timezone(&Utc)
}

/// Go's `buildMiniRegistry`: only the three golden brokers, with a manifest and
/// a schema stub, so the invalid fixture in the shared directory is excluded.
fn mini_registry() -> tempfile::TempDir {
    let tree = tempdir().expect("temp dir");
    fs::create_dir_all(tree.path().join("schemas")).expect("schemas dir");
    fs::write(
        tree.path().join("manifest.json"),
        r#"{"schema_version":1,"schemas":{"broker":"schemas/broker.schema.json"}}"#,
    )
    .expect("manifest");
    fs::write(
        tree.path().join("schemas/broker.schema.json"),
        r#"{"schema_version":1}"#,
    )
    .expect("schema");
    for (sub, file) in [
        ("eu", "golden-email-eu.yaml"),
        ("uk", "golden-multi-uk.yaml"),
        ("us", "golden-webform-us.yaml"),
    ] {
        let dir = tree.path().join("brokers").join(sub);
        fs::create_dir_all(&dir).expect("broker dir");
        let source = Path::new(FIXTURE_DIR).join(file);
        fs::copy(&source, dir.join(file)).expect("copy fixture");
    }
    tree
}

fn golden() -> Value {
    let raw = fs::read(GOLDEN_PLAN_PATH).expect("golden readable");
    serde_json::from_slice(&raw).expect("golden parses")
}

/// The removal-request rows the planner wrote, as Go's golden describes them.
fn db_requests(store: &Store) -> Vec<Value> {
    let connection = store.connection();
    let mut statement = connection
        .prepare(
            "SELECT id, broker_id, channel, campaign_id, jurisdiction, template_id, identity_snapshot_hash\n\
             FROM removal_requests ORDER BY id",
        )
        .expect("prepare");
    let rows = statement
        .query_map([], |row| {
            Ok(serde_json::json!({
                "id": row.get::<_, i64>(0)?,
                "broker_id": row.get::<_, String>(1)?,
                "channel": row.get::<_, String>(2)?,
                "campaign_id": row.get::<_, String>(3)?,
                "jurisdiction": row.get::<_, String>(4)?,
                "template_id": row.get::<_, String>(5)?,
                "identity_snapshot_hash": row.get::<_, String>(6)?,
            }))
        })
        .expect("query");
    rows.map(|row| row.expect("row")).collect()
}

fn planned_payloads(store: &Store) -> Vec<(i64, String, Value)> {
    let connection = store.connection();
    let mut statement = connection
        .prepare(
            "SELECT request_id, event_type, source, payload_json FROM request_events ORDER BY id",
        )
        .expect("prepare");
    let rows = statement
        .query_map([], |row| {
            let request_id: i64 = row.get(0)?;
            let event_type: String = row.get(1)?;
            let source: String = row.get(2)?;
            let payload: String = row.get(3)?;
            Ok((request_id, event_type, source, payload))
        })
        .expect("query");
    rows.map(|row| {
        let (request_id, event_type, source, payload) = row.expect("row");
        let payload: Value = serde_json::from_str(&payload).expect("payload JSON");
        (request_id, format!("{event_type}/{source}"), payload)
    })
    .collect()
}

#[test]
fn golden_plan_conformance() {
    let registry_tree = mini_registry();
    let brokers = load_from_dir(registry_tree.path()).expect("load mini registry");
    assert_eq!(brokers.len(), 3, "mini registry broker count");

    let store_tree = tempdir().expect("temp dir");
    let store = Store::open(store_tree.path().join("db.sqlite")).expect("open store");

    let result = plan_campaign(
        &store,
        &brokers,
        // A deliberately missing identity profile records an empty hash,
        // exactly like the Python fixture run.
        "",
        &PlanOpts {
            campaign_id: "golden-plan".to_owned(),
            max_brokers: 30,
            ..PlanOpts::default()
        },
        pinned_now(),
    )
    .expect("plan campaign");

    let golden = golden();
    assert_eq!(result.campaign_id, golden["campaign_id"]);
    assert_eq!(result.total_brokers, golden["total_brokers"]);
    assert_eq!(result.matched, golden["matched"]);
    assert_eq!(result.planned, golden["planned"]);

    let plan_requests = golden["plan_requests"].as_array().expect("plan requests");
    assert_eq!(result.requests.len(), plan_requests.len());
    for (produced, expected) in result.requests.iter().zip(plan_requests) {
        assert_eq!(produced.broker_id, expected["broker_id"]);
        assert_eq!(produced.broker_name, expected["broker_name"]);
        assert_eq!(produced.channel, expected["channel"]);
        assert_eq!(produced.template, expected["template"]);
        // The golden stores request ids as strings.
        assert_eq!(produced.request_id.to_string(), expected["request_id"]);
    }

    let expected_rows = golden["db_requests"].as_array().expect("db requests");
    let rows = db_requests(&store);
    assert_eq!(rows.len(), expected_rows.len());
    for (produced, expected) in rows.iter().zip(expected_rows) {
        for key in [
            "id",
            "broker_id",
            "channel",
            "campaign_id",
            "jurisdiction",
            "template_id",
            "identity_snapshot_hash",
        ] {
            assert_eq!(produced[key], expected[key], "db_requests.{key}");
        }
    }

    let expected_events = golden["events"].as_array().expect("events");
    let events = planned_payloads(&store);
    assert_eq!(events.len(), expected_events.len());
    for (produced, expected) in events.iter().zip(expected_events) {
        assert_eq!(produced.0, expected["request_id"]);
        assert_eq!(produced.1, "PLANNED/system");
        assert_eq!(expected["event_type"], "PLANNED");
        assert_eq!(expected["source"], "system");
        for key in [
            "broker_name",
            "broker_website",
            "channel",
            "endpoint",
            "template",
            "locale",
            "expected_response_days",
        ] {
            assert_eq!(produced.2[key], expected["payload"][key], "payload.{key}");
        }
    }
}

/// `max_brokers` caps the planned set while `matched` keeps the full count.
#[test]
fn max_brokers_caps_planned_but_not_matched() {
    let registry_tree = mini_registry();
    let brokers = load_from_dir(registry_tree.path()).expect("load mini registry");
    let store_tree = tempdir().expect("temp dir");
    let store = Store::open(store_tree.path().join("db.sqlite")).expect("open store");

    let result = plan_campaign(
        &store,
        &brokers,
        "",
        &PlanOpts {
            campaign_id: "capped".to_owned(),
            max_brokers: 1,
            ..PlanOpts::default()
        },
        pinned_now(),
    )
    .expect("plan campaign");

    assert_eq!(result.matched, 3);
    assert_eq!(result.planned, 1);
    assert_eq!(result.requests.len(), 1);
}

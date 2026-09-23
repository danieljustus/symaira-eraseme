//! DOM-002 plan conformance against the shared golden fixture.
//!
//! Mirrors Go's `TestGoldenPlanConformance`: the same mini registry (three
//! broker YAMLs plus a manifest), the same deliberately missing identity
//! profile (empty snapshot hash) and the same golden
//! `tests/fixtures/event-store/golden-plan.json` — compared down to the
//! removal-request rows and the `PLANNED` event payloads.

use std::fs;
use std::path::Path;
use std::process::Command;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};
use symeraseme_core::campaign::{PlanOpts, get_plan, plan_campaign};
use symeraseme_core::identity::{Profile, ProfileAddress, hash_profile};
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
const BYTES_ORACLE_PATH: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../tests/fixtures/event-store/campaign-plan-bytes-oracle.json"
);
const REPO_ROOT: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../..");

#[derive(Debug, Deserialize)]
struct BytesOracle {
    schema: String,
    generator_sha256: String,
    sources: Vec<SourceDigest>,
    plan_campaign_json: String,
    get_plan_json: String,
    campaign_row_json: String,
    request_rows_json: Vec<String>,
    events: Vec<EventBytes>,
}

#[derive(Debug, Deserialize)]
struct SourceDigest {
    path: String,
    sha256: String,
}

#[derive(Debug, Deserialize)]
struct EventBytes {
    request_id: i64,
    occurred_at: String,
    recorded_at: String,
    event_type: String,
    source: String,
    payload_json: String,
}

#[derive(Serialize)]
struct CampaignRow {
    id: String,
    created_at: String,
    kind: String,
    notes: String,
}

#[derive(Serialize)]
struct RequestRow {
    id: i64,
    broker_id: String,
    channel: String,
    campaign_id: String,
    created_at: String,
    jurisdiction: String,
    template_id: String,
    identity_snapshot_hash: String,
}

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

/// The Go fixture is generated by the real PlanCampaign/GetPlan entrypoints on
/// an isolated database. Keep the Go executable test source-bound and compare
/// the exact serialized bytes plus the persisted SQL payloads here.
#[test]
fn go_campaign_plan_bytes_oracle() {
    let raw_fixture = fs::read(BYTES_ORACLE_PATH).expect("bytes oracle readable");
    let oracle: BytesOracle = serde_json::from_slice(&raw_fixture).expect("bytes oracle parses");
    assert_eq!(oracle.schema, "symeraseme.campaign.plan-bytes-oracle.v1");

    let generator_path = Path::new(REPO_ROOT).join("internal/campaign/plan_bytes_oracle_test.go");
    let generator = fs::read(&generator_path).expect("Go oracle generator readable");
    assert_eq!(
        hex::encode(Sha256::digest(&generator)),
        oracle.generator_sha256
    );
    for source in &oracle.sources {
        let bytes = fs::read(Path::new(REPO_ROOT).join(&source.path))
            .unwrap_or_else(|error| panic!("read Go source {}: {error}", source.path));
        assert_eq!(
            hex::encode(Sha256::digest(bytes)),
            source.sha256,
            "Go oracle source changed: {}",
            source.path
        );
    }
    let go = Command::new("go")
        .args([
            "test",
            "./internal/campaign",
            "-run",
            "^TestCampaignPlanBytesOracle$",
            "-count=1",
        ])
        .current_dir(REPO_ROOT)
        .output()
        .expect("run source-bound Go campaign oracle");
    assert!(
        go.status.success(),
        "Go oracle failed:\n{}\n{}",
        String::from_utf8_lossy(&go.stdout),
        String::from_utf8_lossy(&go.stderr)
    );

    let registry_tree = mini_registry();
    let brokers = load_from_dir(registry_tree.path()).expect("load mini registry");
    let store_tree = tempdir().expect("temp dir");
    let store = Store::open(store_tree.path().join("db.sqlite")).expect("open store");
    let profile_hash = hash_profile(&Profile {
        full_name: "Oracle Person".to_owned(),
        date_of_birth: Some("1990-01-01".to_owned()),
        addresses: vec![ProfileAddress {
            street: "1 Test Street".to_owned(),
            city: "Berlin".to_owned(),
            postal_code: "10115".to_owned(),
            country: "DE".to_owned(),
            ..ProfileAddress::default()
        }],
        email_addresses: vec!["oracle@example.invalid".to_owned()],
        ..Profile::default()
    });
    let result = plan_campaign(
        &store,
        &brokers,
        &profile_hash,
        &PlanOpts {
            campaign_id: "campaign-plan-byte-oracle".to_owned(),
            max_brokers: 30,
            notes: "raw byte oracle".to_owned(),
            ..PlanOpts::default()
        },
        pinned_now(),
    )
    .expect("plan campaign");
    assert_eq!(
        serde_json::to_vec(&result).expect("serialize Rust PlanCampaign"),
        oracle.plan_campaign_json.as_bytes(),
        "PlanCampaign JSON bytes"
    );

    const PINNED_SQL_TIME: &str = "2026-08-06 12:00:00";
    let connection = store.connection();
    connection
        .execute(
            "UPDATE campaigns SET created_at = ?1 WHERE id = ?2",
            (PINNED_SQL_TIME, result.campaign_id.as_str()),
        )
        .expect("pin campaign time");
    connection
        .execute(
            "UPDATE removal_requests SET created_at = ?1",
            [PINNED_SQL_TIME],
        )
        .expect("pin request times");
    connection
        .execute(
            "UPDATE request_events SET occurred_at = ?1, recorded_at = ?1",
            [PINNED_SQL_TIME],
        )
        .expect("pin event times");
    connection
        .execute(
            "UPDATE request_state SET last_event_at = ?1",
            [PINNED_SQL_TIME],
        )
        .expect("pin projected event times");

    let get_plan_value = get_plan(&store, &result.campaign_id, "").expect("GetPlan");
    assert_eq!(
        serde_json::to_vec(&get_plan_value).expect("serialize Rust GetPlan"),
        oracle.get_plan_json.as_bytes(),
        "GetPlan JSON bytes"
    );

    let mut campaign = connection
        .prepare("SELECT id, CAST(created_at AS TEXT), kind, notes FROM campaigns WHERE id = ?1")
        .expect("prepare campaign row");
    let campaign_row = campaign
        .query_row([result.campaign_id.as_str()], |row| {
            Ok(CampaignRow {
                id: row.get(0)?,
                created_at: row.get(1)?,
                kind: row.get(2)?,
                notes: row.get(3)?,
            })
        })
        .expect("campaign row");
    assert_eq!(
        serde_json::to_vec(&campaign_row).expect("serialize campaign row"),
        oracle.campaign_row_json.as_bytes(),
        "campaign row JSON bytes"
    );

    let mut requests = connection
        .prepare(
            "SELECT id, broker_id, channel, campaign_id, CAST(created_at AS TEXT), jurisdiction,\
             template_id, identity_snapshot_hash FROM removal_requests ORDER BY id",
        )
        .expect("prepare request rows");
    let request_rows = requests
        .query_map([], |row| {
            Ok(RequestRow {
                id: row.get(0)?,
                broker_id: row.get(1)?,
                channel: row.get(2)?,
                campaign_id: row.get(3)?,
                created_at: row.get(4)?,
                jurisdiction: row.get(5)?,
                template_id: row.get(6)?,
                identity_snapshot_hash: row.get(7)?,
            })
        })
        .expect("query request rows")
        .map(|row| row.expect("request row"))
        .collect::<Vec<_>>();
    assert_eq!(request_rows.len(), oracle.request_rows_json.len());
    for (row, expected) in request_rows.iter().zip(&oracle.request_rows_json) {
        assert_eq!(
            serde_json::to_vec(row).expect("serialize request row"),
            expected.as_bytes(),
            "request row JSON bytes"
        );
    }

    let mut events = connection
        .prepare(
            "SELECT request_id, CAST(occurred_at AS TEXT), CAST(recorded_at AS TEXT), event_type, source, payload_json \
             FROM request_events ORDER BY id",
        )
        .expect("prepare event rows");
    let rust_events = events
        .query_map([], |row| {
            Ok((
                row.get::<_, i64>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, String>(3)?,
                row.get::<_, String>(4)?,
                row.get::<_, String>(5)?,
            ))
        })
        .expect("query event rows")
        .map(|row| row.expect("event row"))
        .collect::<Vec<_>>();
    assert_eq!(rust_events.len(), oracle.events.len());
    for (row, expected) in rust_events.iter().zip(&oracle.events) {
        assert_eq!(row.0, expected.request_id, "event request id");
        assert_eq!(row.1, expected.occurred_at, "event occurred_at");
        assert_eq!(row.2, expected.recorded_at, "event recorded_at");
        assert_eq!(row.3, expected.event_type, "event type");
        assert_eq!(row.4, expected.source, "event source");
        assert_eq!(
            row.5.as_bytes(),
            expected.payload_json.as_bytes(),
            "event payload bytes"
        );
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

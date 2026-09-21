//! DOM-003 reporting conformance against the shared golden fixture.
//!
//! Mirrors Go's `TestGoldenReportingConformance`: the same four entry points,
//! the same pinned instant and the same seeded store, compared against
//! `tests/fixtures/event-store/golden-reporting.json`.
//!
//! Both sides are normalised through the same JSON encoder before comparison,
//! exactly as the Go test does. That keeps a wrong integer/float choice visible
//! while not failing on float formatting differences: the golden carries Python
//! floats (`100.0`) while Go would marshal the same value as `100`.

use std::fs;

use chrono::{DateTime, Utc};
use serde_json::Value;
use symeraseme_core::reporting::{
    ReportOpts, get_calendar, get_campaign_status, get_dashboard_data, get_report_data,
};
use symeraseme_core::storage::store::Store;
use tempfile::tempdir;

const GOLDEN_REPORTING_PATH: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../tests/fixtures/event-store/golden-reporting.json"
);

/// The seed used by Go's `fixtureStore`, copied statement for statement.
const SEED: [&str; 4] = [
    "INSERT INTO campaigns(id,created_at,kind,notes) VALUES ('new','2026-08-02T08:00:00+00:00','initial','new'),('old','2026-07-01T08:00:00+00:00','initial','old')",
    "INSERT INTO removal_requests(id,broker_id,channel,campaign_id,created_at,jurisdiction,template_id,identity_snapshot_hash) VALUES\n\t\t (1,'broker-a','email','new','2026-08-02T08:00:00+00:00','GDPR','gdpr','h'),\n\t\t (2,'broker-b','web_form','new','2026-08-02T09:00:00+00:00','CCPA','ccpa','h'),\n\t\t (3,'broker-a','email','old','2026-07-01T08:00:00+00:00','GDPR','gdpr','h')",
    "INSERT INTO request_state(request_id,current_status,sent_at,resolved_at,deadline_at,next_action_at,reminders_sent,escalation_level,last_event_id,last_event_at)\n\t\t VALUES (1,'CONFIRMED','2026-08-02T08:00:00+00:00','2026-08-04T08:00:00+00:00','2026-09-01T08:00:00+00:00',NULL,1,0,2,'2026-08-04T08:00:00+00:00'),\n\t\t (2,'OVERDUE','2026-08-02T09:00:00+00:00',NULL,'2026-08-03T09:00:00+00:00','2026-08-05T09:00:00+00:00',2,2,4,'2026-08-05T09:00:00+00:00'),\n\t\t (3,'REJECTED_FINAL','2026-07-01T08:00:00+00:00','2026-07-03T08:00:00+00:00','2026-07-31T08:00:00+00:00',NULL,0,0,6,'2026-07-03T08:00:00+00:00')",
    "INSERT INTO request_events(id,request_id,event_type,occurred_at,payload_json,source) VALUES\n\t\t (1,1,'SENT','2026-08-02T08:00:00+00:00','{}','system'),(2,1,'CONFIRMED','2026-08-04T08:00:00+00:00','{}','inbox'),\n\t\t (3,2,'SENT','2026-08-02T09:00:00+00:00','{}','system'),(4,2,'DEADLINE_REACHED','2026-08-05T09:00:00+00:00','{}','scheduler'),\n\t\t (5,3,'SENT','2026-07-01T08:00:00+00:00','{}','system'),(6,3,'REJECTED_FINAL','2026-07-03T08:00:00+00:00','{}','inbox')",
];

fn fixture_store() -> (tempfile::TempDir, Store) {
    let tree = tempdir().expect("temp dir");
    let store = Store::open(tree.path().join("symeraseme.db")).expect("open store");
    for statement in SEED {
        store
            .connection()
            .execute_batch(statement)
            .expect("seed statement");
    }
    (tree, store)
}

fn pinned_now() -> DateTime<Utc> {
    DateTime::parse_from_rfc3339("2026-08-06T12:00:00+00:00")
        .expect("pinned instant")
        .with_timezone(&Utc)
}

/// Normalises a value the way the Go test does before comparing.
fn normalise(value: &Value) -> String {
    serde_json::to_string(value).expect("value serialises")
}

#[test]
fn golden_reporting_conformance() {
    let (_tree, store) = fixture_store();
    let now = pinned_now();
    let golden: Value =
        serde_json::from_slice(&fs::read(GOLDEN_REPORTING_PATH).expect("golden readable"))
            .expect("golden parses");

    let report = get_report_data(
        &store,
        &ReportOpts {
            campaign_id: String::new(),
            all_campaigns: true,
        },
        now,
    )
    .expect("report data");
    let dashboard = get_dashboard_data(&store, "", now).expect("dashboard data");
    let campaign_status = get_campaign_status(&store, "", now).expect("campaign status");
    let calendar = get_calendar(&store, "", 4, now).expect("calendar");

    for (key, produced) in [
        ("report", &report),
        ("dashboard", &dashboard),
        ("campaign_status", &campaign_status),
        ("calendar", &calendar),
    ] {
        assert_eq!(
            normalise(produced),
            normalise(&golden[key]),
            "{key} differs from the shared golden"
        );
    }
}

/// The anchors Go's own test asserts, kept here so a shape-only regression in
/// the aggregation is caught even if the golden were regenerated.
#[test]
fn golden_reporting_anchor_values() {
    let (_tree, store) = fixture_store();
    let now = pinned_now();
    let report = get_report_data(
        &store,
        &ReportOpts {
            campaign_id: String::new(),
            all_campaigns: true,
        },
        now,
    )
    .expect("report data");

    assert_eq!(report["total_campaigns"], 2);
    assert_eq!(report["total_requests"], 3);
    let metrics = &report["success_metrics"];
    assert_eq!(metrics["overall_confirmation_rate"], 33.3);
    assert_eq!(metrics["median_response_time_days"], 2.0);
    assert_eq!(report["broker_leaderboard"][0]["broker_id"], "broker-a");
    assert_eq!(report["broker_leaderboard"][0]["total"], 2);
    assert_eq!(report["jurisdiction_stats"][0]["jurisdiction"], "GDPR");
}

/// An unknown campaign yields Go's `emptyReport` shape rather than an error.
#[test]
fn unknown_campaign_produces_the_empty_report() {
    let (_tree, store) = fixture_store();
    let report = get_report_data(
        &store,
        &ReportOpts {
            campaign_id: "missing".to_owned(),
            all_campaigns: false,
        },
        pinned_now(),
    )
    .expect("empty report");
    assert_eq!(report["total_campaigns"], 0);
    assert_eq!(report["total_requests"], 0);
    assert_eq!(report["campaigns"], serde_json::json!([]));
    assert_eq!(report["error"], "Campaign 'missing' not found or empty");
    assert_eq!(report["historical_comparison"], serde_json::json!({}));
    assert_eq!(report["success_metrics"], serde_json::json!({}));
}

/// The golden fixture has no tie (its totals are 2 and 1), so any ordering
/// looks correct against it. This pins the rule both sides now share: equal
/// totals order by `broker_id` ascending (#963).
#[test]
fn tied_totals_order_by_broker_id() {
    let (_tree, store) = fixture_store();
    // Three brokers with one request each, seeded in an order that is not
    // alphabetical, so first-seen order and the pinned rule disagree.
    for (id, broker) in [(10, "delta"), (11, "alpha"), (12, "charlie")] {
        store
            .connection()
            .execute_batch(&format!(
                "INSERT INTO removal_requests(id,broker_id,channel,campaign_id,created_at,jurisdiction,template_id,identity_snapshot_hash) \
                 VALUES ({id},'{broker}','email','new','2026-08-03T08:00:00+00:00','GDPR','gdpr','h');\
                 INSERT INTO request_state(request_id,current_status,last_event_at) \
                 VALUES ({id},'CONFIRMED','2026-08-03T08:00:00+00:00')"
            ))
            .expect("seed tied broker");
    }
    let report = get_report_data(
        &store,
        &ReportOpts {
            campaign_id: String::new(),
            all_campaigns: true,
        },
        pinned_now(),
    )
    .expect("report");

    let leaderboard: Vec<String> = report["broker_leaderboard"]
        .as_array()
        .expect("leaderboard")
        .iter()
        // Only the tied brokers. The fixture's own `broker-b` also has a total
        // of 1, so it belongs in this run of the ordering.
        .filter(|entry| entry["total"] == 1)
        .map(|entry| entry["broker_id"].as_str().expect("broker id").to_owned())
        .collect();
    assert_eq!(
        leaderboard,
        vec!["alpha", "broker-b", "charlie", "delta"],
        "equal totals must order by broker_id ascending"
    );
}

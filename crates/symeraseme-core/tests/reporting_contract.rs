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
use serde::Deserialize;
use serde_json::Value;
use sha2::{Digest, Sha256};
use symeraseme_core::reporting::{
    ReportOpts, generate_report, get_calendar, get_campaign_status, get_dashboard_data,
    get_report_data,
};
use symeraseme_core::storage::store::Store;
use tempfile::tempdir;

const GOLDEN_REPORTING_PATH: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../tests/fixtures/event-store/golden-reporting.json"
);
const GO_REPORTING_SOURCE: &[u8] = include_bytes!("../../../internal/reporting/reporting.go");
const GO_BYTES_ORACLE_TEST: &[u8] =
    include_bytes!("../../../internal/reporting/reporting_json_oracle_test.go");
const GO_REPORTING_EXPORT_TEST: &[u8] =
    include_bytes!("../../../internal/reporting/reporting_export_oracle_test.go");
const GO_REPORT_TEMPLATE: &[u8] =
    include_bytes!("../../../internal/templating/templates/report.html.gotmpl");
const GO_TEMPLATING_SOURCE: &[u8] = include_bytes!("../../../internal/templating/templating.go");
const GO_REPORTING_BYTES_PATH: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../tests/fixtures/event-store/golden-reporting-bytes.json"
);
const GO_REPORTING_EXPORTS_PATH: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../tests/fixtures/event-store/golden-reporting-exports.json"
);

#[derive(Deserialize)]
struct GoReportingBytes {
    source_sha256: String,
    report: String,
    dashboard: String,
    campaign_status: String,
    calendar: String,
}

#[derive(Deserialize)]
struct GoReportingExports {
    reporting_source_sha256: String,
    template_source_sha256: String,
    templating_source_sha256: String,
    json: String,
    html: String,
    html_single_campaign: String,
    html_empty_campaign: String,
}

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

/// Canonicalises integral floats the way Go's encoding/json does before the
/// Python-authored semantic golden is compared.
fn normalise(value: &Value) -> String {
    serde_json::to_string(&normalise_go_numbers(value)).expect("value serialises")
}

fn normalise_go_numbers(value: &Value) -> Value {
    match value {
        Value::Number(number) if number.is_f64() => {
            let float = number.as_f64().expect("finite JSON float");
            if float.fract() == 0.0 && float >= i64::MIN as f64 && float < i64::MAX as f64 {
                serde_json::json!(float as i64)
            } else {
                value.clone()
            }
        }
        Value::Object(fields) => Value::Object(
            fields
                .iter()
                .map(|(key, nested)| (key.clone(), normalise_go_numbers(nested)))
                .collect(),
        ),
        Value::Array(items) => Value::Array(items.iter().map(normalise_go_numbers).collect()),
        _ => value.clone(),
    }
}

fn assert_go_json_bytes(label: &str, rust_value: &Value, go_bytes: &str) {
    let rust_bytes = serde_json::to_vec(rust_value).expect("value serialises");
    assert_eq!(
        rust_bytes,
        go_bytes.as_bytes(),
        "{label} JSON bytes differ from Go\nRust: {}\nGo:   {go_bytes}",
        String::from_utf8_lossy(&rust_bytes),
    );
}

#[test]
fn reporting_surfaces_match_go_encoding_json_bytes() {
    let fixture: GoReportingBytes = serde_json::from_str(
        &fs::read_to_string(GO_REPORTING_BYTES_PATH).expect("Go byte fixture readable"),
    )
    .expect("Go byte fixture parses");
    assert_eq!(
        fixture.source_sha256,
        hex::encode(Sha256::digest(GO_REPORTING_SOURCE)),
        "Go reporting implementation changed; regenerate and review byte fixture"
    );
    assert_eq!(
        hex::encode(Sha256::digest(GO_BYTES_ORACLE_TEST)),
        "1cf4d3929a39d5ddc11f0e1f081af4ee6bf724813ed8190cffcd8f7719e87871",
        "Go reporting byte generator changed; review and repin it"
    );
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
    let dashboard = get_dashboard_data(&store, "", now).expect("dashboard data");
    let campaign_status = get_campaign_status(&store, "", now).expect("campaign status");
    let calendar = get_calendar(&store, "", 4, now).expect("calendar");

    for (label, value, go_bytes) in [
        ("report", &report, fixture.report.as_str()),
        ("dashboard", &dashboard, fixture.dashboard.as_str()),
        (
            "campaign_status",
            &campaign_status,
            fixture.campaign_status.as_str(),
        ),
        ("calendar", &calendar, fixture.calendar.as_str()),
    ] {
        assert_go_json_bytes(label, value, go_bytes);
    }
}

#[test]
fn generated_report_exports_match_go() {
    let fixture: GoReportingExports = serde_json::from_str(
        &fs::read_to_string(GO_REPORTING_EXPORTS_PATH).expect("Go export fixture readable"),
    )
    .expect("Go export fixture parses");
    assert_eq!(
        fixture.reporting_source_sha256,
        hex::encode(Sha256::digest(GO_REPORTING_SOURCE)),
        "Go reporting implementation changed; regenerate and review export fixture"
    );
    assert_eq!(
        fixture.template_source_sha256,
        hex::encode(Sha256::digest(GO_REPORT_TEMPLATE)),
        "Go report template changed; regenerate and review export fixture"
    );
    assert_eq!(
        fixture.templating_source_sha256,
        hex::encode(Sha256::digest(GO_TEMPLATING_SOURCE)),
        "Go templating helpers changed; regenerate and review export fixture"
    );
    assert_eq!(
        hex::encode(Sha256::digest(GO_REPORTING_EXPORT_TEST)),
        "f4a94713c8bb36ce0cb905a7dbbe2dd25675a676285ba2481c25a51c396a10a7",
        "Go export oracle changed; review and repin it"
    );

    let (_tree, store) = fixture_store();
    let now = pinned_now();
    let mut report = get_report_data(
        &store,
        &ReportOpts {
            campaign_id: String::new(),
            all_campaigns: true,
        },
        now,
    )
    .expect("report data");
    report["success_rate"] = serde_json::json!(42);
    report["caller_integral"] = serde_json::json!(7);
    assert_eq!(report["success_rate"], 42);
    assert_eq!(report["caller_integral"], 7);
    let assert_output = |label: &str, rust: String, go: &str| {
        if rust != go {
            let offset = rust
                .bytes()
                .zip(go.bytes())
                .position(|(left, right)| left != right)
                .unwrap_or(rust.len().min(go.len()));
            let mut start = offset.saturating_sub(80);
            let mut end_rust = rust.len().min(offset.saturating_add(120));
            let mut end_go = go.len().min(offset.saturating_add(120));
            while start > 0 && (!rust.is_char_boundary(start) || !go.is_char_boundary(start)) {
                start -= 1;
            }
            while end_rust < rust.len() && !rust.is_char_boundary(end_rust) {
                end_rust += 1;
            }
            while end_go < go.len() && !go.is_char_boundary(end_go) {
                end_go += 1;
            }
            panic!(
                "{label} export differs at byte {offset} (Rust {} bytes, Go {} bytes)\nRust: {:?}\nGo:   {:?}",
                rust.len(),
                go.len(),
                &rust[start..end_rust],
                &go[start..end_go],
            );
        }
    };
    assert_output(
        "JSON",
        generate_report(&report, "json", now).expect("report JSON export"),
        &fixture.json,
    );
    assert_output(
        "HTML",
        generate_report(&report, "html", now).expect("report HTML export"),
        &fixture.html,
    );

    let single_campaign = get_report_data(
        &store,
        &ReportOpts {
            campaign_id: "new".to_owned(),
            all_campaigns: false,
        },
        now,
    )
    .expect("single campaign report data");
    assert_output(
        "single campaign HTML",
        generate_report(&single_campaign, "html", now).expect("single campaign HTML export"),
        &fixture.html_single_campaign,
    );

    let (_empty_tree, empty_store) = fixture_store();
    empty_store
        .connection()
        .execute_batch(
            "DELETE FROM request_events WHERE request_id=3;
             DELETE FROM request_state WHERE request_id=3;
             DELETE FROM removal_requests WHERE id=3;
             DELETE FROM campaigns WHERE id='old';
             INSERT INTO campaigns(id,created_at,kind,notes)
             VALUES ('empty','2026-08-03T08:00:00+00:00','initial','empty');",
        )
        .expect("seed empty-campaign report");
    let empty_campaign_report = get_report_data(
        &empty_store,
        &ReportOpts {
            campaign_id: String::new(),
            all_campaigns: true,
        },
        now,
    )
    .expect("empty-campaign report data");
    assert_output(
        "two campaigns with one empty HTML",
        generate_report(&empty_campaign_report, "html", now).expect("empty-campaign HTML export"),
        &fixture.html_empty_campaign,
    );
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
    assert_eq!(metrics["median_response_time_days"].as_f64(), Some(2.0));
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

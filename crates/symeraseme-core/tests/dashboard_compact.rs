//! #1296: the compact `get_dashboard_data` and the one overdue definition.
//!
//! Post-cutover contract change (see `docs/rust-port-contract-matrix.md`,
//! "compact dashboard data and one overdue definition"): the legacy full shape
//! stays what the Go-recorded goldens pin (`reporting_contract.rs`); this file
//! pins the new behaviour on a synthetic store far larger than any golden.

use chrono::{DateTime, Utc};
use serde_json::Value;
use symeraseme_core::reporting::{
    DEFAULT_DASHBOARD_BROKER_LIMIT, DashboardOptions, get_campaign_status, get_dashboard_data,
    get_dashboard_data_with,
};
use symeraseme_core::storage::store::Store;
use tempfile::tempdir;

const BROKERS: usize = 300;
const PER_BROKER: usize = 10;
/// Brokers holding one `AWAITING_ACK` request that is past its deadline.
const STALE_BROKERS: [usize; 5] = [7, 42, 99, 150, 250];

fn now() -> DateTime<Utc> {
    DateTime::parse_from_rfc3339("2026-08-06T12:00:00+00:00")
        .expect("pinned instant")
        .with_timezone(&Utc)
}

fn broker_name(index: usize) -> String {
    format!("broker-{index:03}")
}

/// 300 brokers x 10 requests in one campaign. Per broker: one `CONFIRMED`
/// request (resolved, deadline long past: must never count as overdue), eight
/// `AWAITING_RESPONSE` requests with a future deadline and one more request
/// that is `AWAITING_ACK` with a deadline in the past for [`STALE_BROKERS`]
/// (the status the tick engine has not yet moved to `OVERDUE`) and
/// `AWAITING_RESPONSE` with a future deadline otherwise.
fn large_store() -> (tempfile::TempDir, Store) {
    let tree = tempdir().expect("temp dir");
    let store = Store::open(tree.path().join("symeraseme.db")).expect("open store");
    let mut sql = String::from(
        "BEGIN;\nINSERT INTO campaigns(id,created_at,kind,notes) VALUES \
         ('initial-2026-q2','2026-07-01T08:00:00+00:00','initial','n');\n",
    );
    let mut id = 0;
    for broker in 0..BROKERS {
        for slot in 0..PER_BROKER {
            id += 1;
            let name = broker_name(broker);
            sql.push_str(&format!(
                "INSERT INTO removal_requests(id,broker_id,channel,campaign_id,created_at,jurisdiction,template_id,identity_snapshot_hash) \
                 VALUES ({id},'{name}','email','initial-2026-q2','2026-07-01T08:00:00+00:00','GDPR','gdpr','h');\n"
            ));
            let (status, resolved, deadline) = match slot {
                0 => (
                    "CONFIRMED",
                    "'2026-07-10T08:00:00+00:00'",
                    "'2026-07-31T08:00:00+00:00'",
                ),
                1 if STALE_BROKERS.contains(&broker) => {
                    ("AWAITING_ACK", "NULL", "'2026-08-01T08:00:00+00:00'")
                }
                _ => ("AWAITING_RESPONSE", "NULL", "'2026-09-01T08:00:00+00:00'"),
            };
            sql.push_str(&format!(
                "INSERT INTO request_state(request_id,current_status,sent_at,resolved_at,deadline_at,next_action_at,reminders_sent,escalation_level,last_event_id,last_event_at) \
                 VALUES ({id},'{status}','2026-07-02T08:00:00+00:00',{resolved},{deadline},NULL,0,0,{id},'2026-07-02T08:00:00+00:00');\n"
            ));
            if id <= 60 {
                sql.push_str(&format!(
                    "INSERT INTO request_events(id,request_id,event_type,occurred_at,payload_json,source) \
                     VALUES ({id},{id},'SENT','2026-07-02T08:{:02}:00+00:00','{{}}','system');\n",
                    id % 60
                ));
            }
        }
    }
    sql.push_str("COMMIT;\n");
    store
        .connection()
        .execute_batch(&sql)
        .expect("seed large store");
    (tree, store)
}

fn len(value: &Value) -> usize {
    serde_json::to_string(value).expect("serialises").len()
}

fn broker_ids(dashboard: &Value) -> Vec<String> {
    dashboard["broker_status"]
        .as_array()
        .expect("broker_status array")
        .iter()
        .map(|broker| broker["broker_id"].as_str().expect("broker id").to_owned())
        .collect()
}

#[test]
fn compact_default_is_bounded_and_keeps_totals_by_status_and_campaigns() {
    let (_tree, store) = large_store();
    let compact = get_dashboard_data_with(&store, "", now(), DashboardOptions::default())
        .expect("compact dashboard");
    let full = get_dashboard_data(&store, "", now()).expect("full dashboard");

    let total = (BROKERS * PER_BROKER) as i64;
    assert_eq!(compact["total_requests"], total);
    assert_eq!(compact["confirmed"], BROKERS as i64);
    assert_eq!(compact["awaiting_ack"], STALE_BROKERS.len() as i64);
    assert_eq!(
        compact["by_status"]["AWAITING_RESPONSE"],
        total - BROKERS as i64 - STALE_BROKERS.len() as i64
    );
    assert_eq!(compact["by_status"]["CONFIRMED"], BROKERS as i64);
    let campaigns = compact["campaigns"].as_array().expect("campaigns");
    assert_eq!(campaigns.len(), 1);
    assert_eq!(campaigns[0]["id"], "initial-2026-q2");
    assert_eq!(campaigns[0]["total"], total);
    assert!(
        campaigns[0].get("requests").is_none(),
        "the compact default must not list every request"
    );

    assert_eq!(
        compact["broker_status"].as_array().expect("brokers").len(),
        DEFAULT_DASHBOARD_BROKER_LIMIT
    );
    assert_eq!(compact["broker_total"], BROKERS as i64);
    assert_eq!(compact["broker_status_truncated"], true);
    assert_eq!(
        compact["recent_events"].as_array().expect("events").len(),
        50
    );

    // The legacy full shape for the same store is two orders of magnitude
    // larger; the compact one stays within a small, fixed budget.
    assert!(
        len(&compact) < 16 * 1024,
        "compact dashboard too large: {} bytes",
        len(&compact)
    );
    assert!(
        len(&full) > 20 * len(&compact),
        "full {} vs compact {}",
        len(&full),
        len(&compact)
    );
    // The legacy entry point keeps its keys: no new fields leak into the shape
    // the HTML dashboard template and the recorded goldens consume.
    for key in ["by_status", "broker_total", "broker_status_truncated"] {
        assert!(full.get(key).is_none(), "legacy shape gained {key}");
    }
}

#[test]
fn limit_is_honored_and_selects_the_brokers_needing_attention_first() {
    let (_tree, store) = large_store();
    let options = |limit| DashboardOptions {
        broker_limit: limit,
        ..DashboardOptions::default()
    };

    let top5 = get_dashboard_data_with(&store, "", now(), options(Some(5))).expect("limit 5");
    assert_eq!(
        broker_ids(&top5),
        ["007", "042", "099", "150", "250"].map(|n| format!("broker-{n}")),
        "the five brokers with an overdue request, presented by broker_id"
    );
    assert_eq!(top5["broker_status_truncated"], true);
    for broker in top5["broker_status"].as_array().expect("brokers") {
        assert_eq!(broker["overdue"], 1);
    }

    // Past the overdue brokers the tie-break is deterministic (broker_id).
    let top8 = get_dashboard_data_with(&store, "", now(), options(Some(8))).expect("limit 8");
    assert_eq!(
        broker_ids(&top8),
        [
            "broker-000",
            "broker-001",
            "broker-002",
            "broker-007",
            "broker-042",
            "broker-099",
            "broker-150",
            "broker-250"
        ]
    );
    assert_eq!(
        top8,
        get_dashboard_data_with(&store, "", now(), options(Some(8))).expect("repeat"),
        "same data, same output"
    );

    let all = get_dashboard_data_with(&store, "", now(), options(None)).expect("all brokers");
    assert_eq!(broker_ids(&all).len(), BROKERS);
    assert_eq!(all["broker_status_truncated"], false);
    let overdue_sum: i64 = all["broker_status"]
        .as_array()
        .expect("brokers")
        .iter()
        .map(|broker| broker["overdue"].as_i64().expect("overdue"))
        .sum();
    assert_eq!(overdue_sum, all["overdue"].as_i64().expect("kpi"));
}

#[test]
fn summary_only_omits_the_broker_list_events_and_requests() {
    let (_tree, store) = large_store();
    let summary = get_dashboard_data_with(
        &store,
        "",
        now(),
        DashboardOptions {
            summary_only: true,
            // Even an explicit request for everything yields no lists here.
            include_requests: true,
            broker_limit: None,
        },
    )
    .expect("summary");
    for key in ["broker_status", "recent_events", "broker_status_truncated"] {
        assert!(summary.get(key).is_none(), "summary_only returned {key}");
    }
    assert!(summary["campaigns"][0].get("requests").is_none());
    assert_eq!(summary["total_requests"], (BROKERS * PER_BROKER) as i64);
    assert_eq!(summary["broker_total"], BROKERS as i64);
    assert_eq!(summary["by_status"]["AWAITING_ACK"], 5);
    assert!(
        len(&summary) < 2 * 1024,
        "summary is {} bytes",
        len(&summary)
    );
}

#[test]
fn include_requests_restores_the_per_campaign_request_lists() {
    let (_tree, store) = large_store();
    let dashboard = get_dashboard_data_with(
        &store,
        "",
        now(),
        DashboardOptions {
            include_requests: true,
            ..DashboardOptions::default()
        },
    )
    .expect("dashboard with requests");
    assert_eq!(
        dashboard["campaigns"][0]["requests"]
            .as_array()
            .expect("requests")
            .len(),
        BROKERS * PER_BROKER
    );
}

#[test]
fn dashboard_overdue_kpi_equals_status_upcoming_overdue() {
    let (_tree, store) = large_store();
    let dashboard =
        get_dashboard_data_with(&store, "", now(), DashboardOptions::default()).expect("dashboard");
    let status = get_campaign_status(&store, "", now()).expect("status");

    let expected = STALE_BROKERS.len() as i64;
    // Five AWAITING_ACK requests are past their deadline although none has the
    // status OVERDUE yet; resolved requests with a past deadline do not count.
    assert_eq!(dashboard["by_status"].get("OVERDUE"), None);
    assert_eq!(status["upcoming"]["overdue"], expected);
    assert_eq!(dashboard["overdue"], status["upcoming"]["overdue"]);
    assert_eq!(dashboard["campaigns"][0]["overdue"], expected);

    // The same holds for the legacy full shape that feeds the HTML export.
    let full = get_dashboard_data(&store, "", now()).expect("full");
    assert_eq!(full["overdue"], status["upcoming"]["overdue"]);
}

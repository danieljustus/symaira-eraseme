//! Narrow, explicit overrides for the post-cutover contract change #1296
//! ("compact dashboard data and one overdue definition", see
//! `docs/rust-port-contract-matrix.md`).
//!
//! The recorded Go records stay untouched history. Tests that replay a
//! recorded `get_dashboard_data` result or the recorded `tools/list` response
//! derive the expectation from the frozen bytes with exactly the changes the
//! issue made, and nothing else:
//!
//! * `get_dashboard_data` default result: per-campaign `requests` are gone,
//!   `by_status`, `broker_total` and `broker_status_truncated` are new, and
//!   `overdue` (top level, per campaign, per broker) counts unresolved
//!   requests past their deadline instead of the `OVERDUE` status. Every
//!   other field must still equal the recorded value.
//! * `tools/list`: the `get_dashboard_data` entry, and only that entry, is
//!   replaced by the compact schema.
//!
//! The helpers recompute the new fields from the recorded request lists
//! instead of reading them from the implementation under test, and refuse
//! fixtures that the default broker limit would truncate.

#![allow(dead_code)]

use chrono::{DateTime, Utc};
use serde_json::{Map, Value, json};
use std::collections::BTreeMap;

/// `get_dashboard_data` as recorded (`internal/mcp/tools.json`, serialised
/// with sorted keys like the recorded `tools/list` response).
pub const FROZEN_DASHBOARD_TOOL: &str = r#"{"description":"Return aggregated dashboard data: campaigns, request status counts, broker status, and recent events.","inputSchema":{"properties":{},"type":"object"},"name":"get_dashboard_data"}"#;

/// `get_dashboard_data` since #1296, pinned independently of the source.
pub const COMPACT_DASHBOARD_TOOL: &str = r#"{"description":"Return aggregated dashboard data: campaigns, request status counts (totals and by_status), broker status, and recent events. Compact by default: the 20 brokers needing the most attention and no per-campaign request lists. Use limit, summary_only and include_requests to resize.","inputSchema":{"properties":{"include_requests":{"default":false,"description":"Include every request under its campaign (can be very large)","type":"boolean"},"limit":{"default":20,"description":"Most brokers in broker_status, chosen by overdue then pending then total requests; 0 lists every broker","type":"integer"},"summary_only":{"default":false,"description":"Return only totals, by_status and campaign counters: no broker_status, recent_events or requests","type":"boolean"}},"type":"object"},"name":"get_dashboard_data"}"#;

const DEFAULT_BROKER_LIMIT: usize = 20;

/// The recorded `tools/list` response with the one changed tool swapped in.
pub fn with_compact_dashboard_tool(frozen: &[u8]) -> Vec<u8> {
    let text = std::str::from_utf8(frozen).expect("recorded tools/list is UTF-8");
    assert_eq!(
        text.matches(FROZEN_DASHBOARD_TOOL).count(),
        1,
        "the recorded tools/list must carry the frozen get_dashboard_data entry exactly once"
    );
    text.replace(FROZEN_DASHBOARD_TOOL, COMPACT_DASHBOARD_TOOL)
        .into_bytes()
}

/// The recorded `get_dashboard_data` result (a call without arguments)
/// rewritten into what the compact default returns. `now` enables the
/// deadline-based overdue recomputation; without it the recorded `overdue`
/// values are kept and must already be zero everywhere (empty stores).
pub fn compact_default_result(frozen: &Value, now: Option<DateTime<Utc>>) -> Value {
    let mut result = frozen.clone();
    let object = result
        .as_object_mut()
        .expect("dashboard result is an object");
    let mut by_status: BTreeMap<String, i64> = BTreeMap::new();
    let mut overdue_total = 0i64;
    let mut overdue_by_broker: BTreeMap<String, i64> = BTreeMap::new();
    let mut total_requests = 0i64;

    let campaigns = object
        .get_mut("campaigns")
        .and_then(Value::as_array_mut)
        .expect("campaigns array");
    for campaign in campaigns {
        let campaign = campaign.as_object_mut().expect("campaign object");
        let requests = campaign
            .remove("requests")
            .map(|requests| requests.as_array().cloned().expect("requests array"))
            .unwrap_or_default();
        let mut campaign_overdue = 0i64;
        for request in &requests {
            total_requests += 1;
            let status = request["current_status"]
                .as_str()
                .expect("status")
                .to_uppercase();
            let status = if status.is_empty() {
                "PLANNED".to_owned()
            } else {
                status
            };
            *by_status.entry(status).or_insert(0) += 1;
            if let Some(now) = now
                && past_deadline(request, now)
            {
                campaign_overdue += 1;
                *overdue_by_broker
                    .entry(request["broker_id"].as_str().expect("broker").to_owned())
                    .or_insert(0) += 1;
            }
        }
        if now.is_some() {
            campaign.insert("overdue".to_owned(), json!(campaign_overdue));
        }
        overdue_total += campaign_overdue;
    }
    assert_eq!(
        total_requests,
        object["total_requests"].as_i64().expect("total_requests"),
        "recorded campaigns must list every request"
    );
    if now.is_some() {
        object.insert("overdue".to_owned(), json!(overdue_total));
    }

    let brokers = object
        .get_mut("broker_status")
        .and_then(Value::as_array_mut)
        .expect("broker_status array");
    assert!(
        brokers.len() <= DEFAULT_BROKER_LIMIT,
        "this override only covers recorded stores within the default broker limit"
    );
    let broker_total = brokers.len() as i64;
    if now.is_some() {
        for broker in brokers.iter_mut() {
            let id = broker["broker_id"].as_str().expect("broker id").to_owned();
            broker["overdue"] = json!(overdue_by_broker.get(&id).copied().unwrap_or(0));
        }
    }

    object.insert("by_status".to_owned(), status_map(&by_status));
    object.insert("broker_total".to_owned(), json!(broker_total));
    object.insert("broker_status_truncated".to_owned(), json!(false));
    symeraseme_core::jsonorder::go_map_order(result)
}

fn status_map(counts: &BTreeMap<String, i64>) -> Value {
    Value::Object(
        counts
            .iter()
            .map(|(key, value)| (key.clone(), json!(value)))
            .collect::<Map<String, Value>>(),
    )
}

/// An unresolved request whose deadline has passed: the one overdue rule.
fn past_deadline(request: &Value, now: DateTime<Utc>) -> bool {
    request["resolved_at"].is_null()
        && request["deadline_at"]
            .as_str()
            .and_then(|deadline| DateTime::parse_from_rfc3339(deadline).ok())
            .is_some_and(|deadline| deadline <= now)
}

/// The same override for a recorded byte stream that holds one JSON document
/// (the CLI prints it followed by a newline). Wall-clock placeholders are
/// plain strings, so masked recordings parse too; without `now` the overdue
/// values are kept.
pub fn compact_default_bytes(frozen: &[u8], now: Option<DateTime<Utc>>) -> Vec<u8> {
    let newline = frozen.ends_with(b"\n");
    let value: Value = serde_json::from_slice(frozen).expect("recorded dashboard JSON");
    let mut bytes = serde_json::to_vec(&compact_default_result(&value, now))
        .expect("compact dashboard serialises");
    if newline {
        bytes.push(b'\n');
    }
    bytes
}

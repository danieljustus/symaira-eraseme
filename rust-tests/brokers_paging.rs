//! Narrow, explicit overrides for the post-cutover contract change #1342
//! ("bounded `list_brokers` pages", see `docs/rust-port-contract-matrix.md`).
//!
//! The recorded Go catalogue and the frozen `tools/list` / `mcp-tool-gaps`
//! records stay untouched history. Tests derive the current expectation from
//! those bytes with exactly the changes the issue made and nothing else:
//!
//! * `tools/list`: the `list_brokers` entry, and only that entry, gains the
//!   `limit`, `cursor` and `detail` parameters.
//! * a successful recorded `list_brokers` result: the recorded registry is
//!   truncated to the default summary page, `count` becomes the page size,
//!   `total` keeps the recorded count, and `next_cursor` plus `detail` are
//!   added. Every other recorded byte is still compared as is.
//!
//! The helper derives the expected page from the recorded brokers instead of
//! reading it from the implementation under test.

#![allow(dead_code)]

use serde_json::{Value, json};

/// Default page size of the current `list_brokers`.
pub const DEFAULT_BROKER_LIMIT: usize = 50;
/// Maximum accepted `limit`.
pub const MAX_BROKER_LIMIT: usize = 500;

/// `list_brokers` as recorded (`internal/mcp/tools.json`, serialised with
/// sorted keys like the recorded `tools/list` response).
pub const FROZEN_BROKERS_TOOL: &str = r#"{"description":"Return filtered brokers from the registry.","inputSchema":{"properties":{"category":{"description":"Filter by broker category","type":"string"},"include_disabled":{"default":false,"description":"Include disabled brokers","type":"boolean"},"include_inactive":{"default":false,"description":"Include deprecated, merged, and out-of-business brokers","type":"boolean"},"jurisdiction":{"description":"Filter by jurisdiction (e.g. GDPR, CCPA)","type":"string"},"law":{"description":"Filter by specific law","type":"string"},"priority":{"description":"Filter by priority level","type":"string"},"status":{"default":"active","description":"Broker lifecycle status filter (defaults to active)","enum":["active","deprecated","merged","out-of-business"],"type":"string"}},"type":"object"},"name":"list_brokers"}"#;

/// `list_brokers` since #1342, pinned independently of the source.
pub const PAGED_BROKERS_TOOL: &str = r#"{"description":"Return filtered brokers from the registry. Bounded, paged summary by default; use limit, cursor and detail for full records.","inputSchema":{"properties":{"category":{"description":"Filter by broker category","type":"string"},"cursor":{"description":"Opaque continuation cursor returned as next_cursor","type":"string"},"detail":{"default":"summary","description":"summary returns id, name, category, priority and status; full returns complete broker records","enum":["summary","full"],"type":"string"},"include_disabled":{"default":false,"description":"Include disabled brokers","type":"boolean"},"include_inactive":{"default":false,"description":"Include deprecated, merged, and out-of-business brokers","type":"boolean"},"jurisdiction":{"description":"Filter by jurisdiction (e.g. GDPR, CCPA)","type":"string"},"law":{"description":"Filter by specific law","type":"string"},"limit":{"default":50,"description":"Maximum brokers per page (1..500, default 50)","type":"integer"},"priority":{"description":"Filter by priority level","type":"string"},"status":{"default":"active","description":"Broker lifecycle status filter (defaults to active)","enum":["active","deprecated","merged","out-of-business"],"type":"string"}},"type":"object"},"name":"list_brokers"}"#;

/// The recorded `tools/list` response with the one changed tool swapped in.
pub fn with_paged_brokers_tool(frozen: &[u8]) -> Vec<u8> {
    let text = std::str::from_utf8(frozen).expect("recorded tools/list is UTF-8");
    assert_eq!(
        text.matches(FROZEN_BROKERS_TOOL).count(),
        1,
        "the recorded tools/list must carry the frozen list_brokers entry exactly once"
    );
    text.replace(FROZEN_BROKERS_TOOL, PAGED_BROKERS_TOOL)
        .into_bytes()
}

/// A recorded `list_brokers` payload rewritten into the default summary page.
/// The recorded registry is history; only the page envelope changes.
pub fn paged_default_summary(recorded: &Value) -> Value {
    assert_eq!(recorded["schema_version"], 1, "recorded schema version");
    let brokers = recorded["brokers"]
        .as_array()
        .expect("recorded broker array");
    let total = recorded["count"].as_u64().expect("recorded count");
    assert_eq!(
        brokers.len() as u64,
        total,
        "recorded count and rows disagree"
    );

    let page: Vec<Value> = brokers
        .iter()
        .take(DEFAULT_BROKER_LIMIT)
        .map(|broker| {
            json!({
                "id": broker["id"],
                "name": broker["name"],
                "category": broker["category"],
                "priority": broker["priority"],
                "status": broker["status"],
            })
        })
        .collect();
    let next_cursor = if page.len() < total as usize {
        Value::String(format!("brokers:{}", page.len()))
    } else {
        Value::Null
    };
    json!({
        "brokers": page,
        "count": page.len(),
        "filters": recorded["filters"],
        "schema_version": 1,
        "total": total,
        "next_cursor": next_cursor,
        "detail": "summary",
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn frozen_and_paged_tool_entries_are_distinct_valid_json() {
        let frozen: Value = serde_json::from_str(FROZEN_BROKERS_TOOL).expect("frozen tool JSON");
        let paged: Value = serde_json::from_str(PAGED_BROKERS_TOOL).expect("paged tool JSON");
        assert_eq!(frozen["name"], "list_brokers");
        assert_eq!(paged["name"], "list_brokers");
        assert!(frozen["inputSchema"]["properties"]["limit"].is_null());
        assert_eq!(paged["inputSchema"]["properties"]["limit"]["default"], 50);
        assert_ne!(frozen, paged);
    }

    #[test]
    fn default_summary_drops_nested_fields_and_adds_the_page_envelope() {
        let recorded = json!({
            "brokers": [
                {"id":"a","name":"A","website":"https://a","category":"other",
                 "jurisdictions":["DE"],"laws":["GDPR"],"data_sensitivity":3,
                 "priority":"high","opt_out":[],"status":"active"}
            ],
            "count": 1,
            "filters": {"include_disabled": false, "status": "active"},
            "schema_version": 1,
        });
        assert_eq!(
            paged_default_summary(&recorded),
            json!({
                "brokers": [{"id":"a","name":"A","category":"other","priority":"high","status":"active"}],
                "count": 1,
                "filters": {"include_disabled": false, "status": "active"},
                "schema_version": 1,
                "total": 1,
                "next_cursor": null,
                "detail": "summary",
            })
        );
    }
}

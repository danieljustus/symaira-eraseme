//! Text-mode rendering of read-only command results (post-cutover contract
//! change #1302, see `docs/rust-port-contract-matrix.md`).
//!
//! The retired Go CLI printed only `success` in text mode and kept the payload
//! for `--output json`. These pure functions turn the same payload into
//! readable lines; the JSON bytes are never touched.

use serde_json::Value;

/// `manual-tasks list|show`: the tool already carries a human `message`.
pub fn message(result: &Value) -> String {
    result
        .get("message")
        .and_then(Value::as_str)
        .map_or_else(|| "success".to_owned(), str::to_owned)
}

/// The `error` of a tool result that reports `success: false`.
///
/// Tools such as `manual_tasks_show` answer a missing task with a payload, not
/// a transport error; text mode shows that error instead of `success`.
pub fn failure(result: &Value) -> Option<String> {
    if result.get("success").and_then(Value::as_bool) != Some(false) {
        return None;
    }
    result
        .get("error")
        .and_then(Value::as_str)
        .map(str::to_owned)
}

fn text<'a>(value: &'a Value, key: &str) -> &'a str {
    value.get(key).and_then(Value::as_str).unwrap_or("")
}

fn number(value: &Value, key: &str) -> i64 {
    value.get(key).and_then(Value::as_i64).unwrap_or(0)
}

fn or_dash(value: &str) -> &str {
    if value.is_empty() { "-" } else { value }
}

/// `requests list`: a count line, then one line per request.
pub fn requests_list(result: &Value) -> String {
    let rows = result
        .get("requests")
        .and_then(Value::as_array)
        .map_or(&[][..], Vec::as_slice);
    let mut out = format!(
        "Requests: {} of {} (page {}, {} per page)",
        rows.len(),
        number(result, "total"),
        number(result, "page"),
        number(result, "page_size"),
    );
    if rows.is_empty() {
        out.push_str("\nNo requests found.");
    }
    for row in rows {
        out.push_str(&format!(
            "\n  #{} [{}] {} via {} campaign={} created={} deadline={}",
            number(row, "id"),
            text(row, "current_status"),
            text(row, "broker_id"),
            text(row, "channel"),
            or_dash(text(row, "campaign_id")),
            text(row, "created_at"),
            or_dash(text(row, "deadline_at")),
        ));
    }
    out
}

/// `events show`: the tool result is the event list as JSON text, or null when
/// the request has no events.
pub fn events(result: &Value) -> String {
    let parsed: Vec<Value> = match result {
        Value::String(rendered) => serde_json::from_str(rendered).unwrap_or_default(),
        Value::Array(list) => list.clone(),
        _ => Vec::new(),
    };
    if parsed.is_empty() {
        return "No events found.".to_owned();
    }
    let mut out = format!("Events ({}):", parsed.len());
    for event in &parsed {
        out.push_str(&format!(
            "\n  #{} {} {} ({})",
            number(event, "ID"),
            text(event, "OccurredAt"),
            text(event, "EventType"),
            text(event, "Source"),
        ));
    }
    out
}

/// `calendar`: a header with the totals, then the entries grouped by week.
pub fn calendar(result: &Value) -> String {
    let totals = result.get("totals").unwrap_or(&Value::Null);
    let scope = result
        .get("scope")
        .map_or("all", |scope| text(scope, "campaign_id"));
    let mut out = format!(
        "Calendar for {scope}: {} entries, {} overdue, next {} weeks (until {})",
        number(totals, "entries"),
        number(totals, "overdue"),
        number(result, "horizon_weeks"),
        text(result, "horizon_until"),
    );
    let weeks = result
        .get("weeks")
        .and_then(Value::as_array)
        .map_or(&[][..], Vec::as_slice);
    if weeks.is_empty() {
        out.push_str("\nNo unresolved actions in this period.");
    }
    for week in weeks {
        out.push_str(&format!("\n{}", text(week, "week")));
        let entries = week
            .get("entries")
            .and_then(Value::as_array)
            .map_or(&[][..], Vec::as_slice);
        for entry in entries {
            let days = number(entry, "days_from_now");
            let when = if entry.get("overdue").and_then(Value::as_bool) == Some(true) {
                format!("{} days overdue", -days)
            } else {
                format!("in {days} days")
            };
            out.push_str(&format!(
                "\n  #{} {} [{}] {} {} ({when})",
                number(entry, "request_id"),
                text(entry, "broker_id"),
                text(entry, "current_status"),
                text(entry, "marker"),
                text(entry, "marker_at"),
            ));
        }
    }
    out
}

/// `dashboard`: the request counters on one line plus the campaign count.
pub fn dashboard(result: &Value) -> String {
    let campaigns = result
        .get("campaigns")
        .and_then(Value::as_array)
        .map_or(0, Vec::len);
    format!(
        "Dashboard: {} requests (planned {}, sent {}, awaiting ack {}, awaiting response {}, confirmed {}, rejected {}, overdue {})\nCampaigns: {campaigns}",
        number(result, "total_requests"),
        number(result, "planned"),
        number(result, "sent"),
        number(result, "awaiting_ack"),
        number(result, "awaiting_response"),
        number(result, "confirmed"),
        number(result, "rejected"),
        number(result, "overdue"),
    )
}

/// `schedule status`: the platform, then one line per scheduler unit.
pub fn schedule_status(result: &Value) -> String {
    let mut out = format!("Scheduler platform: {}", text(result, "Platform"));
    let entries = result
        .get("Entries")
        .and_then(Value::as_array)
        .map_or(&[][..], Vec::as_slice);
    for entry in entries {
        let flag = |key: &str| entry.get(key).and_then(Value::as_bool) == Some(true);
        out.push_str(&format!(
            "\n  {}: {}, {}{} ({})",
            text(entry, "Label"),
            if flag("Installed") {
                "installed"
            } else {
                "not installed"
            },
            if flag("Active") { "active" } else { "inactive" },
            if flag("Legacy") { ", legacy" } else { "" },
            text(entry, "Path"),
        ));
        let error = text(entry, "Error");
        if !error.is_empty() {
            out.push_str(&format!(" - {error}"));
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn requests_list_prints_counts_and_one_line_per_request() {
        let result = json!({
            "page": 1, "page_size": 100, "total": 2,
            "requests": [
                {"id": 7, "broker_id": "acme", "channel": "email", "campaign_id": "pop",
                 "created_at": "2026-01-02T03:04:05Z", "current_status": "SENT",
                 "deadline_at": "2026-02-01T00:00:00Z"},
                {"id": 8, "broker_id": "beta", "channel": "web_form", "campaign_id": "",
                 "created_at": "2026-01-03T00:00:00Z", "current_status": "PLANNED",
                 "deadline_at": null},
            ],
        });
        assert_eq!(
            requests_list(&result),
            "Requests: 2 of 2 (page 1, 100 per page)\n  #7 [SENT] acme via email campaign=pop created=2026-01-02T03:04:05Z deadline=2026-02-01T00:00:00Z\n  #8 [PLANNED] beta via web_form campaign=- created=2026-01-03T00:00:00Z deadline=-"
        );
        let empty = json!({"page": 1, "page_size": 100, "total": 0, "requests": null});
        assert_eq!(
            requests_list(&empty),
            "Requests: 0 of 0 (page 1, 100 per page)\nNo requests found."
        );
    }

    #[test]
    fn events_render_the_rendered_json_list_or_report_none() {
        let list = json!(
            r#"[{"ID":1,"RequestID":1,"OccurredAt":"2026-08-07T08:59:00Z","EventType":"PLANNED","Source":"system"},{"ID":2,"RequestID":1,"OccurredAt":"2026-08-07T09:00:00Z","EventType":"SENT","Source":"user"}]"#
        );
        assert_eq!(
            events(&list),
            "Events (2):\n  #1 2026-08-07T08:59:00Z PLANNED (system)\n  #2 2026-08-07T09:00:00Z SENT (user)"
        );
        assert_eq!(events(&Value::Null), "No events found.");
    }

    #[test]
    fn calendar_groups_entries_by_week_and_marks_overdue() {
        let result = json!({
            "horizon_weeks": 4, "horizon_until": "2026-03-01T00:00:00Z",
            "scope": {"campaign_id": "all"},
            "totals": {"entries": 2, "overdue": 1, "weeks_with_actions": 1},
            "weeks": [{"week": "2026-W06", "entries": [
                {"request_id": 3, "broker_id": "acme", "current_status": "SENT",
                 "marker": "deadline", "marker_at": "2026-02-02T00:00:00Z",
                 "days_from_now": -2, "overdue": true},
                {"request_id": 4, "broker_id": "beta", "current_status": "SENT",
                 "marker": "next_action", "marker_at": "2026-02-09T00:00:00Z",
                 "days_from_now": 5, "overdue": false},
            ]}],
        });
        assert_eq!(
            calendar(&result),
            "Calendar for all: 2 entries, 1 overdue, next 4 weeks (until 2026-03-01T00:00:00Z)\n2026-W06\n  #3 acme [SENT] deadline 2026-02-02T00:00:00Z (2 days overdue)\n  #4 beta [SENT] next_action 2026-02-09T00:00:00Z (in 5 days)"
        );
        let empty = json!({"horizon_weeks": 4, "horizon_until": "u", "scope": {"campaign_id": "all"},
            "totals": {"entries": 0, "overdue": 0}, "weeks": []});
        assert!(calendar(&empty).ends_with("\nNo unresolved actions in this period."));
    }

    #[test]
    fn schedule_status_lists_every_unit() {
        let result = json!({"Platform": "launchd", "Entries": [
            {"Label": "symeraseme-tick", "Installed": true, "Active": true,
             "Path": "/a/tick.plist", "Legacy": false, "Error": ""},
            {"Label": "symeraseme-poll", "Installed": false, "Active": false,
             "Path": "/a/poll.plist", "Legacy": true, "Error": "launchctl failed"},
        ]});
        assert_eq!(
            schedule_status(&result),
            "Scheduler platform: launchd\n  symeraseme-tick: installed, active (/a/tick.plist)\n  symeraseme-poll: not installed, inactive, legacy (/a/poll.plist) - launchctl failed"
        );
    }

    #[test]
    fn message_and_failure_read_the_tool_payload() {
        assert_eq!(
            message(&json!({"message": "Manual tasks (1):"})),
            "Manual tasks (1):"
        );
        assert_eq!(message(&json!({"success": true})), "success");
        assert_eq!(
            failure(&json!({"success": false, "error": "not found"})),
            Some("not found".to_owned())
        );
        assert_eq!(failure(&json!({"success": true, "error": "x"})), None);
    }

    #[test]
    fn dashboard_summarises_counters() {
        let result = json!({"total_requests": 5, "planned": 1, "sent": 2, "awaiting_ack": 1,
            "awaiting_response": 0, "confirmed": 1, "rejected": 0, "overdue": 1,
            "campaigns": [{}, {}]});
        assert_eq!(
            dashboard(&result),
            "Dashboard: 5 requests (planned 1, sent 2, awaiting ack 1, awaiting response 0, confirmed 1, rejected 0, overdue 1)\nCampaigns: 2"
        );
    }
}

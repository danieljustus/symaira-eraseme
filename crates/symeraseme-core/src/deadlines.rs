//! Pure deadline decisions for the event-store tick contract.
//!
//! Database selection and event application stay outside this slice.  The
//! caller supplies the same projected row shape used by the Go tick engine.

use crate::{storage::TickCandidate, timeutil};
use chrono::{DateTime, Duration, Utc};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value, json};

pub const REMINDER_DAYS: i64 = 7;
pub const DPA_ESCALATION_DAYS: i64 = 14;
pub const RESCAN_DAYS: i64 = 90;

/// One scheduler decision, matching the Go `deadlines.Action` JSON shape.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct TickAction {
    pub action_type: String,
    pub broker_id: String,
    pub campaign_id: String,
    pub current_status: String,
    pub description: String,
    pub dry_run: bool,
    pub event_type: String,
    pub payload: Map<String, Value>,
    pub request_id: i64,
}

/// Evaluate one projected request without reading or writing SQLite.
pub fn actions_for_candidate(
    request: &TickCandidate,
    now: DateTime<Utc>,
    dry_run: bool,
) -> Vec<TickAction> {
    let status = if request.current_status.is_empty() {
        "PLANNED"
    } else {
        request.current_status.as_str()
    };
    match status {
        "AWAITING_ACK" => reminder(request, now, dry_run).into_iter().collect(),
        "AWAITING_RESPONSE" => deadline(request, now, dry_run).into_iter().collect(),
        "OVERDUE" => dpa(request, now, dry_run).into_iter().collect(),
        "CONFIRMED" => rescan(request, now, dry_run).into_iter().collect(),
        _ => Vec::new(),
    }
}

fn jurisdiction_days(jurisdiction: &str) -> i64 {
    match jurisdiction {
        "GDPR" | "LGPD" | "PIPEDA" => 30,
        "CCPA" | "CPRA" => 45,
        _ => 30,
    }
}

fn reminder(request: &TickCandidate, now: DateTime<Utc>, dry_run: bool) -> Option<TickAction> {
    let sent = parsed(&request.sent_at)?;
    let days = days_between(now, sent);
    if days < REMINDER_DAYS {
        return None;
    }
    let shift = u32::try_from(request.reminders_sent).ok()?;
    let threshold = REMINDER_DAYS.checked_mul(1_i64.checked_shl(shift)?)?;
    if days < threshold {
        return None;
    }
    let count = request.reminders_sent.saturating_add(1);
    Some(TickAction {
        action_type: "send_reminder".into(),
        broker_id: request.broker_id.clone(),
        campaign_id: request.campaign_id.clone(),
        current_status: "AWAITING_ACK".into(),
        description: format!("Send reminder #{count} ({days}d since sent)"),
        dry_run,
        event_type: "REMINDER_SENT".into(),
        payload: Map::from_iter([
            ("count".into(), json!(count)),
            ("days_since_sent".into(), json!(days)),
        ]),
        request_id: request.id,
    })
}

fn deadline(request: &TickCandidate, now: DateTime<Utc>, dry_run: bool) -> Option<TickAction> {
    let days = jurisdiction_days(&request.jurisdiction);
    let explicit = parsed(&request.deadline_at);
    let deadline =
        explicit.or_else(|| parsed(&request.sent_at).map(|sent| sent + Duration::days(days)))?;
    if now < deadline {
        return None;
    }
    let detail = if explicit.is_some() {
        format!(", passed {}", python_timedelta(now - deadline))
    } else {
        String::new()
    };
    Some(TickAction {
        action_type: "mark_overdue".into(),
        broker_id: request.broker_id.clone(),
        campaign_id: request.campaign_id.clone(),
        current_status: "AWAITING_RESPONSE".into(),
        description: format!(
            "Deadline reached ({}d{}{})",
            days,
            detail,
            if detail.is_empty() { " from sent" } else { "" }
        ),
        dry_run,
        event_type: "DEADLINE_REACHED".into(),
        payload: Map::from_iter([
            ("deadline_days".into(), json!(days)),
            ("deadline_at".into(), json!(timeutil::format_iso(deadline))),
        ]),
        request_id: request.id,
    })
}

fn dpa(request: &TickCandidate, now: DateTime<Utc>, dry_run: bool) -> Option<TickAction> {
    let deadline = parsed(&request.deadline_at)?;
    if request.escalation_level >= 2 {
        return None;
    }
    let days = days_between(now, deadline);
    if days < DPA_ESCALATION_DAYS {
        return None;
    }
    Some(TickAction {
        action_type: "draft_dpa_complaint".into(),
        broker_id: request.broker_id.clone(),
        campaign_id: request.campaign_id.clone(),
        current_status: "OVERDUE".into(),
        description: format!("DPA complaint ready ({days}d since deadline)"),
        dry_run,
        event_type: "DPA_COMPLAINT_DRAFTED".into(),
        payload: Map::from_iter([("days_since_deadline".into(), json!(days))]),
        request_id: request.id,
    })
}

fn rescan(request: &TickCandidate, now: DateTime<Utc>, dry_run: bool) -> Option<TickAction> {
    let resolved = parsed(&request.resolved_at)?;
    let days = days_between(now, resolved);
    if days < RESCAN_DAYS {
        return None;
    }
    Some(TickAction {
        action_type: "trigger_rescan".into(),
        broker_id: request.broker_id.clone(),
        campaign_id: request.campaign_id.clone(),
        current_status: "CONFIRMED".into(),
        description: format!("Re-scan due ({days}d since resolution)"),
        dry_run,
        event_type: "RE_SCAN_TRIGGERED".into(),
        payload: Map::from_iter([("days_since_resolved".into(), json!(days))]),
        request_id: request.id,
    })
}

fn parsed(value: &str) -> Option<DateTime<Utc>> {
    (!value.is_empty())
        .then(|| timeutil::parse_timestamp(value).ok())
        .flatten()
}

fn days_between(now: DateTime<Utc>, then: DateTime<Utc>) -> i64 {
    (now - then).num_hours() / 24
}

fn python_timedelta(duration: Duration) -> String {
    let seconds = duration.num_seconds();
    let days = seconds / 86_400;
    let remainder = seconds % 86_400;
    let hours = remainder / 3_600;
    let minutes = remainder % 3_600 / 60;
    let seconds = remainder % 60;
    if days > 0 {
        let unit = if days == 1 { "day" } else { "days" };
        format!("{days} {unit}, {hours}:{minutes:02}:{seconds:02}")
    } else {
        format!("{hours}:{minutes:02}:{seconds:02}")
    }
}

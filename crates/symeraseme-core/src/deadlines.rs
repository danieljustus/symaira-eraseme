//! Pure deadline decisions for the event-store tick contract.
//!
//! Database selection and event application stay outside this slice.  The
//! caller supplies the same projected row shape used by the Go tick engine.

use crate::storage::Store;
use crate::storage::projection::ProjectionError;
use crate::storage::repository::Repository;
use crate::storage::types::{EventType, Source};
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
    pub request_id: i64,
    pub broker_id: String,
    pub campaign_id: String,
    pub current_status: String,
    pub action_type: String,
    pub event_type: String,
    pub description: String,
    pub payload: Map<String, Value>,
    pub dry_run: bool,
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
    let shift = request.reminders_sent as u64;
    let shifted = if shift >= i64::BITS as u64 {
        0
    } else {
        1_i64.wrapping_shl(shift as u32)
    };
    let threshold = REMINDER_DAYS.wrapping_mul(shifted);
    if days < threshold {
        return None;
    }
    let count = request.reminders_sent.wrapping_add(1);
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

/// Mirrors Go's `RunOpts`. The instant is injected rather than passed in the
/// options, matching this crate's clock convention.
#[derive(Debug, Clone, Default)]
pub struct RunOpts {
    pub dry_run: bool,
    /// `<= 0` disables the batch limit, as in Go.
    pub batch_size: i64,
}

#[derive(Debug)]
pub enum DeadlinesError {
    Database(rusqlite::Error),
    Projection(ProjectionError),
}

impl std::fmt::Display for DeadlinesError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Database(error) => write!(formatter, "{error}"),
            Self::Projection(error) => write!(formatter, "{error}"),
        }
    }
}

impl std::error::Error for DeadlinesError {}

impl From<rusqlite::Error> for DeadlinesError {
    fn from(error: rusqlite::Error) -> Self {
        Self::Database(error)
    }
}

/// Mirrors Go's `RunTick`: scans the requests whose `next_action_at` is due (or
/// unset) and evaluates each against its status. Nothing is written here; the
/// caller decides between dry run and apply.
pub fn run_tick(
    store: &Store,
    opts: &RunOpts,
    now: DateTime<Utc>,
) -> Result<Vec<TickAction>, DeadlinesError> {
    let repository = Repository::new(store);
    let candidates =
        repository.fetch_tick_candidates(&timeutil::format_iso(now), opts.batch_size)?;
    let mut actions = Vec::new();
    for candidate in &candidates {
        actions.extend(actions_for_candidate(candidate, now, opts.dry_run));
    }
    Ok(actions)
}

/// One applied (or skipped) action, matching Go's `ApplyResult` JSON shape.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ApplyResult {
    pub request_id: i64,
    pub action: String,
    pub event_type: String,
    pub description: String,
    pub executed: bool,
    #[serde(skip_serializing_if = "is_false")]
    pub dry_run: bool,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub error: String,
}

fn is_false(value: &bool) -> bool {
    !*value
}

/// Mirrors Go's `ApplyTickActions`: appends each action's event with source
/// `scheduler`, records a dry run without writing, keeps per-action failures
/// instead of aborting the batch, and finally rebuilds every projection.
pub fn apply_tick_actions(
    store: &Store,
    actions: &[TickAction],
    now: DateTime<Utc>,
) -> Result<Vec<ApplyResult>, DeadlinesError> {
    if actions.is_empty() {
        return Ok(Vec::new());
    }
    let mut results = Vec::with_capacity(actions.len());
    for action in actions {
        if action.dry_run {
            results.push(ApplyResult {
                request_id: action.request_id,
                action: action.action_type.clone(),
                event_type: action.event_type.clone(),
                description: action.description.clone(),
                executed: false,
                dry_run: true,
                error: String::new(),
            });
            continue;
        }
        let applied = store.append_and_project(
            action.request_id,
            &EventType::from_wire(action.event_type.clone()),
            &action.payload,
            &Source::Scheduler,
            now,
        );
        match applied {
            Ok(_) => results.push(ApplyResult {
                request_id: action.request_id,
                action: action.action_type.clone(),
                event_type: action.event_type.clone(),
                description: action.description.clone(),
                executed: true,
                dry_run: false,
                error: String::new(),
            }),
            Err(error) => results.push(ApplyResult {
                request_id: action.request_id,
                action: action.action_type.clone(),
                event_type: action.event_type.clone(),
                description: action.description.clone(),
                executed: false,
                dry_run: false,
                error: error.to_string(),
            }),
        }
    }
    // Go ignores the rebuild outcome here (Python rebuild_all_states).
    let _ = store.rebuild_all_states(500);
    Ok(results)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parsed_instant(value: &str) -> DateTime<Utc> {
        timeutil::parse_timestamp(value).expect("instant parses")
    }

    /// Mirrors Go's `TestDeadlineDSTAndMonthBoundary`: Python's
    /// `timedelta(days=n)` is exactly n*86400s, so a DST transition must never
    /// shift a deadline, and month/year boundaries follow fixed days.
    #[test]
    fn deadline_arithmetic_ignores_dst_and_calendar_boundaries() {
        let sent = parsed_instant("2026-03-20T12:00:00+00:00");
        assert_eq!(
            sent + Duration::days(30),
            parsed_instant("2026-04-19T12:00:00+00:00"),
            "EU spring-forward must not shift the deadline"
        );

        let january = parsed_instant("2026-01-31T12:00:00+00:00");
        let march = january + Duration::days(30);
        assert_eq!(march, parsed_instant("2026-03-02T12:00:00+00:00"));

        let december = parsed_instant("2026-12-15T08:30:00+00:00");
        assert_eq!(
            december + Duration::days(30),
            parsed_instant("2027-01-14T08:30:00+00:00")
        );
    }

    /// Mirrors the `pyTimedelta` renderings Go asserts: hours are not
    /// zero-padded, minutes and seconds are.
    #[test]
    fn python_timedelta_renderings_match_python() {
        assert_eq!(python_timedelta(Duration::days(10)), "10 days, 0:00:00");
        assert_eq!(python_timedelta(Duration::days(1)), "1 day, 0:00:00");
        assert_eq!(
            python_timedelta(Duration::hours(5) + Duration::minutes(3) + Duration::seconds(4)),
            "5:03:04"
        );
    }
}

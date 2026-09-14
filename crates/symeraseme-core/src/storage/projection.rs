//! Replay-only `request_state` projection support.
//!
//! The Go event-store remains the executable oracle. This module deliberately
//! covers only DB-004: reading an existing event log in SQLite's
//! `(occurred_at ASC, id ASC)` order and folding it into an in-memory state.
//! Appending events, persisting projections, and transactional projection
//! updates remain separate slices.

use super::{repository::Repository, store::Store, types};
use crate::timeutil;
use chrono::Duration;
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

/// The serialized shape of a rebuilt `request_state` row.
///
/// Declaration order is intentional: it matches Go's `StateJSON` and the
/// committed `golden-projection.json` fixture byte-for-byte after compact JSON
/// serialization.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct ProjectionState {
    pub acknowledged_at: Option<String>,
    pub current_status: String,
    pub deadline_at: Option<String>,
    pub escalation_level: i64,
    pub last_event_at: Option<String>,
    pub last_event_id: i64,
    pub next_action_at: Option<String>,
    pub reminders_sent: i64,
    pub request_id: i64,
    pub resolved_at: Option<String>,
    pub sent_at: Option<String>,
}

impl ProjectionState {
    /// Returns the Go oracle's state before any replayable event exists.
    #[must_use]
    pub fn new(request_id: i64) -> Self {
        Self {
            acknowledged_at: None,
            current_status: "PLANNED".to_owned(),
            deadline_at: None,
            escalation_level: 0,
            last_event_at: None,
            last_event_id: 0,
            next_action_at: None,
            reminders_sent: 0,
            request_id,
            resolved_at: None,
            sent_at: None,
        }
    }
}

impl Store {
    /// Rebuilds one request's state without writing `request_state`.
    ///
    /// [`Repository::get_events`] owns the production SQLite query and its
    /// per-row decoder. It orders rows by `(occurred_at, id)` and skips rows
    /// whose timestamp or object payload cannot be materialized, matching the
    /// Go replay loader.
    pub fn rebuild_state(&self, request_id: i64) -> rusqlite::Result<ProjectionState> {
        let events = Repository::new(self).get_events(request_id, 0)?;
        Ok(fold_events(request_id, &events))
    }
}

/// Folds an already replay-ordered event slice into a projection state.
///
/// Database callers must use [`Store::rebuild_state`] so SQLite establishes
/// the oracle's `(occurred_at ASC, id ASC)` ordering. This pure seam exists to
/// make the fold independently inspectable and testable.
#[must_use]
pub fn fold_events(request_id: i64, events: &[types::EventRecord]) -> ProjectionState {
    let mut state = ProjectionState::new(request_id);
    for event in events {
        // Go logs a malformed direct-fold event and continues replay. The
        // repository decoder has already removed timestamp/payload failures;
        // direct-fold errors are an empty event type or Go's zero time.
        let _ = apply_event(&mut state, event);
    }
    state
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum ReplayEventError {
    EmptyEventType,
    EmptyOccurredAt,
}

fn apply_event(
    state: &mut ProjectionState,
    event: &types::EventRecord,
) -> Result<(), ReplayEventError> {
    if matches!(&event.event_type, types::EventType::Unknown(value) if value.is_empty()) {
        return Err(ReplayEventError::EmptyEventType);
    }
    // `time.Time{}.IsZero()` is a separate Go replay error from a timestamp
    // that failed parsing. The shared decoder preserves this syntactically
    // valid year-one value, then the projection skips it here.
    if is_go_zero_time(event.occurred_at) {
        return Err(ReplayEventError::EmptyOccurredAt);
    }

    if let Some(status) = status_for(&event.event_type) {
        state.current_status = status.to_owned();
    }
    // This intentionally precedes event-specific effects. The executable Go
    // source retains a parseable, nonempty unknown event in replay bookkeeping
    // even though it has no status transition or other side effect.
    state.last_event_id = event.id;
    state.last_event_at = Some(timeutil::format_iso(event.occurred_at));

    match event.event_type {
        types::EventType::Sent => {
            state.sent_at = Some(timeutil::format_iso(event.occurred_at));
            let days = payload_integer(&event.payload, "expected_response_days")
                .unwrap_or(30)
                .max(0);
            // Go evaluates `time.Duration(days) * 24 * time.Hour` using
            // signed integer arithmetic. Keep that wrapping duration contract
            // instead of turning oversized historical payloads into errors.
            const NANOSECONDS_PER_DAY: i64 = 86_400_000_000_000;
            let duration = Duration::nanoseconds(days.wrapping_mul(NANOSECONDS_PER_DAY));
            if let Some(deadline) = event.occurred_at.checked_add_signed(duration) {
                state.deadline_at = Some(timeutil::format_iso(deadline));
            }
        }
        types::EventType::Ack => {
            state.acknowledged_at = Some(timeutil::format_iso(event.occurred_at));
        }
        types::EventType::Confirmed | types::EventType::RejectedFinal => {
            state.resolved_at = Some(timeutil::format_iso(event.occurred_at));
        }
        types::EventType::ReminderSent => {
            let count = payload_integer(&event.payload, "count").unwrap_or(0);
            state.reminders_sent = if count == 0 { 1 } else { count };
        }
        types::EventType::DeadlineReached => {
            state.escalation_level = 1;
        }
        types::EventType::DpaComplaintDrafted => {
            state.escalation_level = 2;
        }
        types::EventType::Planned
        | types::EventType::SendFailed
        | types::EventType::Bounce
        | types::EventType::Autoresponder
        | types::EventType::VerificationRequested
        | types::EventType::VerificationProvided
        | types::EventType::HumanActionRequired
        | types::EventType::ConfirmationLinkClicked
        | types::EventType::ReplyDrafted
        | types::EventType::RebuttalSent
        | types::EventType::DpaComplaintFiled
        | types::EventType::RescanTriggered
        | types::EventType::NoteAdded
        | types::EventType::Unknown(_) => {}
    }
    Ok(())
}

fn status_for(event_type: &types::EventType) -> Option<&'static str> {
    match event_type {
        types::EventType::Planned => Some("PLANNED"),
        types::EventType::Sent => Some("AWAITING_ACK"),
        types::EventType::SendFailed => Some("SEND_FAILED"),
        types::EventType::Bounce => Some("BOUNCE"),
        types::EventType::Ack => Some("ACK"),
        types::EventType::Autoresponder => Some("AWAITING_ACK"),
        types::EventType::VerificationRequested => Some("AWAITING_USER_ACTION"),
        types::EventType::VerificationProvided => Some("AWAITING_RESPONSE"),
        types::EventType::HumanActionRequired => Some("AWAITING_USER_ACTION"),
        types::EventType::Confirmed => Some("CONFIRMED"),
        types::EventType::RejectedFinal => Some("REJECTED_FINAL"),
        types::EventType::ConfirmationLinkClicked => Some("CONFIRMED"),
        types::EventType::RebuttalSent => Some("AWAITING_RESPONSE"),
        types::EventType::ReminderSent => Some("AWAITING_ACK"),
        types::EventType::DeadlineReached => Some("OVERDUE"),
        types::EventType::DpaComplaintDrafted => Some("ESCALATED"),
        types::EventType::DpaComplaintFiled => Some("DPA_FILED"),
        types::EventType::RescanTriggered => Some("RE_SCAN_DUE"),
        types::EventType::ReplyDrafted
        | types::EventType::NoteAdded
        | types::EventType::Unknown(_) => None,
    }
}

/// Whether a UTC instant is Go's `time.Time{}` zero value.
fn is_go_zero_time(timestamp: chrono::DateTime<chrono::Utc>) -> bool {
    timestamp.timestamp() == -62_135_596_800 && timestamp.timestamp_subsec_nanos() == 0
}

/// Extracts the numeric JSON values Go's `toInt` accepts on replay.
///
/// Go decodes generic JSON numbers as `float64`, then truncates toward zero
/// when converting them to an integer. Strings and booleans are not numeric.
fn payload_integer(payload: &Map<String, Value>, key: &str) -> Option<i64> {
    payload
        .get(key)
        .and_then(Value::as_f64)
        .map(go_float_to_int)
}

#[allow(clippy::cast_possible_truncation, clippy::cast_possible_wrap)]
fn go_float_to_int(value: f64) -> i64 {
    // Conversion of an out-of-range float to Go's `int` is target-specific.
    // The x86_64 instruction used by the Go oracle yields MinInt64 at +2^63;
    // retain that observable behavior instead of using Rust's saturating cast.
    #[cfg(target_arch = "x86_64")]
    if value >= 9_223_372_036_854_775_808.0 {
        return i64::MIN;
    }
    value as i64
}

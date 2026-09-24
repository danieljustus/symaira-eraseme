//! Event-log append and `request_state` projection.
//!
//! The event log is authoritative. A projection is rebuilt by replaying rows
//! in `(occurred_at ASC, id ASC)` order, matching the Go event-store oracle.
//! Append-time validation is strict, while replay is deliberately tolerant of
//! unknown or malformed historical rows so a newer writer cannot make an
//! existing database unreadable. Parseable unknown rows retain their replay
//! bookkeeping position; malformed rows are skipped entirely.

use super::{repository::Repository, store::Store, types};
use crate::timeutil;
use chrono::{DateTime, Duration, TimeZone, Utc};
use rusqlite::{Row, Transaction, params};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use std::{
    collections::BTreeMap,
    error::Error,
    fmt,
    time::{SystemTime, UNIX_EPOCH},
};

/// Errors returned by event append and projection operations.
#[derive(Debug)]
pub enum ProjectionError {
    /// An event type outside the closed append catalogue was supplied.
    UnknownEventType(String),
    /// A source outside the closed append catalogue was supplied.
    UnknownSource(String),
    /// SQLite rejected a query or transaction operation.
    Database(rusqlite::Error),
    /// The event payload could not be serialized as JSON.
    PayloadSerialization(serde_json::Error),
}

impl fmt::Display for ProjectionError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::UnknownEventType(value) => {
                write!(formatter, "eventstore: unknown event type: {value:?}")
            }
            Self::UnknownSource(value) => {
                write!(formatter, "eventstore: unknown source: {value:?}")
            }
            Self::Database(error) => error.fmt(formatter),
            Self::PayloadSerialization(error) => {
                write!(formatter, "eventstore: marshal payload: {error}")
            }
        }
    }
}

impl Error for ProjectionError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Database(error) => Some(error),
            Self::PayloadSerialization(error) => Some(error),
            Self::UnknownEventType(_) | Self::UnknownSource(_) => None,
        }
    }
}

impl From<rusqlite::Error> for ProjectionError {
    fn from(error: rusqlite::Error) -> Self {
        Self::Database(error)
    }
}

impl From<serde_json::Error> for ProjectionError {
    fn from(error: serde_json::Error) -> Self {
        Self::PayloadSerialization(error)
    }
}

/// Result type for event append and projection operations.
pub type ProjectionResult<T> = Result<T, ProjectionError>;

/// The serialized shape of one `request_state` projection.
///
/// Field declaration order is intentional: it matches the Go `StateJSON`
/// struct and therefore the compact JSON emitted by the production fixture
/// conformance tests.
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
    /// Creates the blank state used before a request has any replayable event.
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

/// Appends an event within a caller-owned transaction and rebuilds its
/// projection before returning. The caller must commit the transaction to
/// make either write visible; dropping or rolling it back leaves no partial
/// event or projection.
pub fn append_and_project_tx(
    transaction: &Transaction<'_>,
    request_id: i64,
    event_type: &types::EventType,
    payload: &Map<String, Value>,
    source: &types::Source,
    occurred_at: DateTime<Utc>,
) -> ProjectionResult<(i64, ProjectionState)> {
    let event_id = append_event_tx(
        transaction,
        request_id,
        event_type,
        payload,
        source,
        occurred_at,
    )?;
    let state = upsert_state_tx(transaction, request_id)?;
    Ok((event_id, state))
}

impl Store {
    /// Appends one validated event and commits it.
    pub fn append_event(
        &self,
        request_id: i64,
        event_type: &types::EventType,
        payload: &Map<String, Value>,
        source: &types::Source,
        occurred_at: DateTime<Utc>,
    ) -> ProjectionResult<i64> {
        let transaction = self.connection().unchecked_transaction()?;
        let event_id = append_event_tx(
            &transaction,
            request_id,
            event_type,
            payload,
            source,
            occurred_at,
        )?;
        transaction.commit()?;
        Ok(event_id)
    }

    /// Alias matching the Go store's `Append` operation.
    pub fn append(
        &self,
        request_id: i64,
        event_type: &types::EventType,
        payload: &Map<String, Value>,
        source: &types::Source,
        occurred_at: DateTime<Utc>,
    ) -> ProjectionResult<i64> {
        self.append_event(request_id, event_type, payload, source, occurred_at)
    }

    /// Appends an event and updates its projection in one transaction.
    pub fn append_and_project(
        &self,
        request_id: i64,
        event_type: &types::EventType,
        payload: &Map<String, Value>,
        source: &types::Source,
        occurred_at: DateTime<Utc>,
    ) -> ProjectionResult<(i64, ProjectionState)> {
        let transaction = self.connection().unchecked_transaction()?;
        let result = append_and_project_tx(
            &transaction,
            request_id,
            event_type,
            payload,
            source,
            occurred_at,
        )?;
        transaction.commit()?;
        Ok(result)
    }

    /// Replays a request's event log without changing `request_state`.
    ///
    /// [`Repository::get_events`] orders rows by `(occurred_at, id)` and skips rows
    /// whose timestamp or object payload cannot be materialized, matching the
    /// Go replay loader.
    pub fn rebuild_state(&self, request_id: i64) -> ProjectionResult<ProjectionState> {
        let events = Repository::new(self).get_events(request_id, 0)?;
        Ok(fold_events(request_id, &events))
    }

    /// Rebuilds and persists one request's projection.
    pub fn upsert_state(&self, request_id: i64) -> ProjectionResult<ProjectionState> {
        let transaction = self.connection().unchecked_transaction()?;
        let state = upsert_state_tx(&transaction, request_id)?;
        transaction.commit()?;
        Ok(state)
    }

    /// Rebuilds every request whose latest event is newer than its projection.
    /// Returns the number of projections written.
    pub fn rebuild_all_states(&self, chunk_size: usize) -> ProjectionResult<usize> {
        let chunk_size = if chunk_size == 0 { 100 } else { chunk_size };
        let dirty = {
            let mut statement = self.connection().prepare(
                "SELECT DISTINCT r.id
                 FROM removal_requests r
                 JOIN request_events e ON e.request_id = r.id
                 LEFT JOIN request_state s ON s.request_id = r.id
                 WHERE s.last_event_id IS NULL OR e.id > s.last_event_id",
            )?;
            statement
                .query_map([], |row| row.get::<_, i64>(0))?
                .collect::<rusqlite::Result<Vec<_>>>()?
        };

        let mut rebuilt = 0;
        for chunk in dirty.chunks(chunk_size) {
            for request_id in chunk {
                self.upsert_state(*request_id)?;
                rebuilt += 1;
            }
        }
        Ok(rebuilt)
    }

    /// Returns all request ids in ascending database order.
    pub fn request_ids(&self) -> ProjectionResult<Vec<i64>> {
        let mut statement = self
            .connection()
            .prepare("SELECT id FROM removal_requests ORDER BY id ASC")?;
        Ok(statement
            .query_map([], |row| row.get::<_, i64>(0))?
            .collect::<rusqlite::Result<Vec<_>>>()?)
    }

    /// Rebuilds every request, keyed by its numeric request id.
    pub fn all_projections(&self) -> ProjectionResult<BTreeMap<i64, ProjectionState>> {
        let mut request_ids = self.request_ids()?;
        request_ids.sort_unstable();
        request_ids
            .into_iter()
            .map(|request_id| {
                self.rebuild_state(request_id)
                    .map(|state| (request_id, state))
            })
            .collect()
    }
}

fn append_event_tx(
    transaction: &Transaction<'_>,
    request_id: i64,
    event_type: &types::EventType,
    payload: &Map<String, Value>,
    source: &types::Source,
    occurred_at: DateTime<Utc>,
) -> ProjectionResult<i64> {
    validate_append(event_type, source)?;
    let payload_json = if payload.is_empty() {
        "{}".to_owned()
    } else {
        serde_json::to_string(&crate::jsonorder::go_map_order(Value::Object(
            payload.clone(),
        )))
        .map_err(ProjectionError::PayloadSerialization)?
    };
    transaction.execute(
        "INSERT INTO request_events
         (request_id, occurred_at, recorded_at, event_type, payload_json, source)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
        params![
            request_id,
            timeutil::format_sql(occurred_at),
            timeutil::format_sql(utc_now()),
            event_type.as_str(),
            payload_json,
            source.as_str(),
        ],
    )?;
    Ok(transaction.last_insert_rowid())
}

fn validate_append(event_type: &types::EventType, source: &types::Source) -> ProjectionResult<()> {
    if matches!(event_type, types::EventType::Unknown(_)) {
        return Err(ProjectionError::UnknownEventType(
            event_type.as_str().to_owned(),
        ));
    }
    if matches!(source, types::Source::Unknown(_)) {
        return Err(ProjectionError::UnknownSource(source.as_str().to_owned()));
    }
    Ok(())
}

fn upsert_state_tx(
    transaction: &Transaction<'_>,
    request_id: i64,
) -> ProjectionResult<ProjectionState> {
    let events = load_events(transaction, request_id)?;
    let state = fold_events(request_id, &events);
    transaction.execute(
        "INSERT OR REPLACE INTO request_state
         (request_id, current_status, last_event_id, last_event_at,
          sent_at, acknowledged_at, resolved_at, deadline_at,
          next_action_at, reminders_sent, escalation_level)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)",
        params![
            state.request_id,
            state.current_status.as_str(),
            state.last_event_id,
            state.last_event_at.as_deref(),
            state.sent_at.as_deref(),
            state.acknowledged_at.as_deref(),
            state.resolved_at.as_deref(),
            state.deadline_at.as_deref(),
            state.next_action_at.as_deref(),
            state.reminders_sent,
            state.escalation_level,
        ],
    )?;
    Ok(state)
}

fn load_events(
    connection: &rusqlite::Connection,
    request_id: i64,
) -> ProjectionResult<Vec<types::EventRecord>> {
    let mut statement = connection.prepare(
        "SELECT id, request_id, occurred_at, recorded_at, event_type, payload_json, source
         FROM request_events
         WHERE request_id = ?1
         ORDER BY occurred_at ASC, id ASC",
    )?;
    let mut rows = statement.query(params![request_id])?;
    let mut events = Vec::new();
    while let Some(row) = rows.next()? {
        if let Some(event) = decode_replay_event(row) {
            events.push(event);
        }
    }
    Ok(events)
}

/// Decodes only rows that the Go projection can materialise. Historical rows
/// that fail timestamp or object-payload decoding are skipped by design.
fn decode_replay_event(row: &Row<'_>) -> Option<types::EventRecord> {
    let occurred_at: String = row.get(2).ok()?;
    let recorded_at: String = row.get(3).ok()?;
    let occurred_at = timeutil::parse_timestamp(&occurred_at).ok()?;
    let recorded_at = timeutil::parse_timestamp(&recorded_at).ok()?;
    let payload_json: String = row.get(5).ok()?;
    let payload = match payload_json.trim() {
        "" | "null" => Map::new(),
        value => match serde_json::from_str::<Value>(value).ok()? {
            Value::Object(payload) => payload,
            _ => return None,
        },
    };
    Some(types::EventRecord {
        id: row.get(0).ok()?,
        request_id: row.get(1).ok()?,
        occurred_at,
        recorded_at,
        event_type: types::EventType::from_wire(row.get(4).ok()?),
        payload,
        source: types::Source::from_wire(row.get(6).ok()?),
    })
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

fn utc_now() -> DateTime<Utc> {
    let elapsed = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default();
    Utc.timestamp_opt(elapsed.as_secs() as i64, elapsed.subsec_nanos())
        .single()
        .expect("system clock must produce a representable UTC timestamp")
}

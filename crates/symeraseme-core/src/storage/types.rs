//! Typed rows shared by the SQLite store and repository query layer.

use chrono::{DateTime, Utc};
use serde_json::{Map, Value};
use std::fmt;

/// A row from the `campaigns` table.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Campaign {
    pub id: String,
    /// Kept as database text so the query layer does not normalize timestamp
    /// layouts that callers may need to round-trip.
    pub created_at: String,
    pub kind: String,
    pub notes: Option<String>,
}

/// A row from the `removal_requests` table without projected state.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RemovalRequest {
    pub id: i64,
    pub broker_id: String,
    pub channel: String,
    pub campaign_id: String,
    pub created_at: String,
    pub jurisdiction: String,
    pub template_id: String,
    pub identity_snapshot_hash: String,
}

/// A removal request joined with the nullable `request_state` columns.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RemovalRequestRow {
    pub id: i64,
    pub broker_id: String,
    pub channel: String,
    pub campaign_id: String,
    pub created_at: String,
    pub jurisdiction: String,
    pub template_id: String,
    pub identity_snapshot_hash: String,
    pub current_status: Option<String>,
    pub last_event_at: Option<String>,
    pub sent_at: Option<String>,
    pub acknowledged_at: Option<String>,
    pub resolved_at: Option<String>,
    pub deadline_at: Option<String>,
    pub next_action_at: Option<String>,
    pub reminders_sent: i64,
    pub escalation_level: i64,
}

/// A decoded `request_events` row.
///
/// Event timestamps are parsed to UTC because the Go event-store oracle
/// materialises them as `time.Time`. Request/campaign timestamp columns remain
/// raw strings in their row types because those queries preserve NULL and
/// storage-layout information.
#[derive(Clone, Debug, PartialEq)]
pub struct EventRecord {
    pub id: i64,
    pub request_id: i64,
    pub occurred_at: DateTime<Utc>,
    pub recorded_at: DateTime<Utc>,
    pub event_type: EventType,
    pub payload: Map<String, Value>,
    pub source: Source,
}

/// The closed event catalogue used by appenders and projection code.
///
/// `Unknown` is retained on read so a query does not silently discard a row
/// merely because a newer Go writer introduced an event type.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum EventType {
    Planned,
    Sent,
    SendFailed,
    Bounce,
    Autoresponder,
    Ack,
    VerificationRequested,
    VerificationProvided,
    HumanActionRequired,
    ConfirmationLinkClicked,
    ReplyDrafted,
    RebuttalSent,
    ReminderSent,
    DeadlineReached,
    DpaComplaintDrafted,
    DpaComplaintFiled,
    Confirmed,
    RejectedFinal,
    RescanTriggered,
    NoteAdded,
    Unknown(String),
}

impl EventType {
    pub(crate) fn from_wire(value: String) -> Self {
        match value.as_str() {
            "PLANNED" => Self::Planned,
            "SENT" => Self::Sent,
            "SEND_FAILED" => Self::SendFailed,
            "BOUNCE" => Self::Bounce,
            "AUTORESPONDER" => Self::Autoresponder,
            "ACK" => Self::Ack,
            "VERIFICATION_REQUESTED" => Self::VerificationRequested,
            "VERIFICATION_PROVIDED" => Self::VerificationProvided,
            "HUMAN_ACTION_REQUIRED" => Self::HumanActionRequired,
            "CONFIRMATION_LINK_CLICKED" => Self::ConfirmationLinkClicked,
            "REPLY_DRAFTED" => Self::ReplyDrafted,
            "REBUTTAL_SENT" => Self::RebuttalSent,
            "REMINDER_SENT" => Self::ReminderSent,
            "DEADLINE_REACHED" => Self::DeadlineReached,
            "DPA_COMPLAINT_DRAFTED" => Self::DpaComplaintDrafted,
            "DPA_COMPLAINT_FILED" => Self::DpaComplaintFiled,
            "CONFIRMED" => Self::Confirmed,
            "REJECTED_FINAL" => Self::RejectedFinal,
            "RE_SCAN_TRIGGERED" => Self::RescanTriggered,
            "NOTE_ADDED" => Self::NoteAdded,
            _ => Self::Unknown(value),
        }
    }

    pub fn as_str(&self) -> &str {
        match self {
            Self::Planned => "PLANNED",
            Self::Sent => "SENT",
            Self::SendFailed => "SEND_FAILED",
            Self::Bounce => "BOUNCE",
            Self::Autoresponder => "AUTORESPONDER",
            Self::Ack => "ACK",
            Self::VerificationRequested => "VERIFICATION_REQUESTED",
            Self::VerificationProvided => "VERIFICATION_PROVIDED",
            Self::HumanActionRequired => "HUMAN_ACTION_REQUIRED",
            Self::ConfirmationLinkClicked => "CONFIRMATION_LINK_CLICKED",
            Self::ReplyDrafted => "REPLY_DRAFTED",
            Self::RebuttalSent => "REBUTTAL_SENT",
            Self::ReminderSent => "REMINDER_SENT",
            Self::DeadlineReached => "DEADLINE_REACHED",
            Self::DpaComplaintDrafted => "DPA_COMPLAINT_DRAFTED",
            Self::DpaComplaintFiled => "DPA_COMPLAINT_FILED",
            Self::Confirmed => "CONFIRMED",
            Self::RejectedFinal => "REJECTED_FINAL",
            Self::RescanTriggered => "RE_SCAN_TRIGGERED",
            Self::NoteAdded => "NOTE_ADDED",
            Self::Unknown(value) => value,
        }
    }
}

impl fmt::Display for EventType {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.as_str())
    }
}

/// The writer identity recorded on an event.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Source {
    System,
    Inbox,
    User,
    Scheduler,
    Unknown(String),
}

impl Source {
    pub(crate) fn from_wire(value: String) -> Self {
        match value.as_str() {
            "system" => Self::System,
            "inbox" => Self::Inbox,
            "user" => Self::User,
            "scheduler" => Self::Scheduler,
            _ => Self::Unknown(value),
        }
    }

    pub fn as_str(&self) -> &str {
        match self {
            Self::System => "system",
            Self::Inbox => "inbox",
            Self::User => "user",
            Self::Scheduler => "scheduler",
            Self::Unknown(value) => value,
        }
    }
}

impl fmt::Display for Source {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.as_str())
    }
}

/// A request selected by the deadline/tick query.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TickCandidate {
    pub id: i64,
    pub broker_id: String,
    pub campaign_id: String,
    pub jurisdiction: String,
    pub current_status: String,
    pub sent_at: String,
    pub deadline_at: String,
    pub next_action_at: String,
    pub acknowledged_at: String,
    pub resolved_at: String,
    pub reminders_sent: i64,
    pub escalation_level: i64,
}

/// Compatibility aliases for callers that prefer the query-oriented names.
pub type RequestRow = RemovalRequestRow;
pub type RemovalRequestWithState = RemovalRequestRow;

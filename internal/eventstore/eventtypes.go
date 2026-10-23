// Package eventstore is the Go port of the Python
// symeraseme.core.eventstore (events, projection, repositories,
// db_connection, db_encryption, db_cleanup) onto a pure-Go SQLite
// driver (modernc.org/sqlite, no CGO).  See docs/event-store.md for the
// on-disk format contract.
//
// The package exposes three sub-packages:
//   - ./timeutil         -- UTC-aware ISO timestamp parsing
//   - ./projection       -- event-log -> request_state fold
//   - ./encryption       -- V1/V2/V3 Fernet envelope (PBKDF2 + HKDF)
//
// and a flat root namespace that mirrors the Python facade: AppendEvent,
// CreateCampaign, CreateRemovalRequest, InitDB, OpenAt, CloseAt, …
package eventstore

// EventType enumerates the closed catalogue defined in
// docs/event-store.md §3.  AppendEvent validates on the write path;
// rebuild_state() tolerates unknown types (logs + skips) for forward
// compatibility.
type EventType string

const (
	EvtPlanned                 EventType = "PLANNED"
	EvtSent                    EventType = "SENT"
	EvtSendFailed              EventType = "SEND_FAILED"
	EvtBounce                  EventType = "BOUNCE"
	EvtAutoresponder           EventType = "AUTORESPONDER"
	EvtAck                     EventType = "ACK"
	EvtVerificationRequested   EventType = "VERIFICATION_REQUESTED"
	EvtVerificationProvided    EventType = "VERIFICATION_PROVIDED"
	EvtHumanActionRequired     EventType = "HUMAN_ACTION_REQUIRED"
	EvtConfirmationLinkClicked EventType = "CONFIRMATION_LINK_CLICKED"
	EvtReplyDrafted            EventType = "REPLY_DRAFTED"
	EvtRebuttalSent            EventType = "REBUTTAL_SENT"
	EvtReminderSent            EventType = "REMINDER_SENT"
	EvtDeadlineReached         EventType = "DEADLINE_REACHED"
	EvtDPAComplaintDrafted     EventType = "DPA_COMPLAINT_DRAFTED"
	EvtDPAComplaintFiled       EventType = "DPA_COMPLAINT_FILED"
	EvtConfirmed               EventType = "CONFIRMED"
	EvtRejectedFinal           EventType = "REJECTED_FINAL"
	EvtRescanTriggered         EventType = "RE_SCAN_TRIGGERED"
	EvtNoteAdded               EventType = "NOTE_ADDED"
)

// ValidEventTypes is the closed set used for append-time validation.
var ValidEventTypes = map[EventType]struct{}{
	EvtPlanned: {}, EvtSent: {}, EvtSendFailed: {}, EvtBounce: {},
	EvtAutoresponder: {}, EvtAck: {}, EvtVerificationRequested: {},
	EvtVerificationProvided: {}, EvtHumanActionRequired: {},
	EvtConfirmationLinkClicked: {}, EvtReplyDrafted: {},
	EvtRebuttalSent: {}, EvtReminderSent: {}, EvtDeadlineReached: {},
	EvtDPAComplaintDrafted: {}, EvtDPAComplaintFiled: {},
	EvtConfirmed: {}, EvtRejectedFinal: {}, EvtRescanTriggered: {},
	EvtNoteAdded: {},
}

// IsValid reports whether t is in ValidEventTypes.  Used by AppendEvent.
func (t EventType) IsValid() bool {
	_, ok := ValidEventTypes[t]
	return ok
}

// Source enumerates the four writer identities.  AppendEvent enforces
// this; replay does not.
type Source string

const (
	SrcSystem    Source = "system"
	SrcInbox     Source = "inbox"
	SrcUser      Source = "user"
	SrcScheduler Source = "scheduler"
)

// ValidSources mirrors VALID_SOURCES in core/events.py.
var ValidSources = map[Source]struct{}{
	SrcSystem: {}, SrcInbox: {}, SrcUser: {}, SrcScheduler: {},
}

// IsValid reports whether s is in ValidSources.
func (s Source) IsValid() bool {
	_, ok := ValidSources[s]
	return ok
}

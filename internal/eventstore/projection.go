// Package eventstore (projection.go) is the Go port of
// symeraseme.core.projection: rebuild_state / upsert_state /
// append_event_and_project / rebuild_all_states.
//
// The fold mirrors the Python _accumulate_state: replays the event
// log in (occurred_at ASC, id ASC) order, applies the §4 status
// transition table, and writes the resulting request_state. Unknown
// event types, unparseable timestamps, and bad payloads are logged
// and skipped (never abort the rebuild — see docs/event-store.md §7).
package eventstore

import (
	"context"
	"database/sql"
	"fmt"
	"log/slog"
	"sort"
	"time"

	"github.com/danieljustus/symaira-eraseme/internal/eventstore/timeutil"
)

// StateJSON is the on-the-wire shape of a rebuilt request_state row.
// Field order MUST match the Python projection: it is the contract
// the conformance test compares byte-for-byte against
// tests/fixtures/event-store/golden-projection.json.
type StateJSON struct {
	AcknowledgedAt  *string `json:"acknowledged_at"`
	CurrentStatus   string  `json:"current_status"`
	DeadlineAt      *string `json:"deadline_at"`
	EscalationLevel int     `json:"escalation_level"`
	LastEventAt     *string `json:"last_event_at"`
	LastEventID     int64   `json:"last_event_id"`
	NextActionAt    *string `json:"next_action_at"`
	RemindersSent   int     `json:"reminders_sent"`
	RequestID       int64   `json:"request_id"`
	ResolvedAt      *string `json:"resolved_at"`
	SentAt          *string `json:"sent_at"`
}

// nextStatus is the §4 status transition table.  nil means "no
// change to current_status" (used by NOTE_ADDED).
func nextStatus(et EventType) (string, bool) {
	switch et {
	case EvtPlanned:
		return "PLANNED", true
	case EvtSent:
		return "AWAITING_ACK", true
	case EvtSendFailed:
		return "SEND_FAILED", true
	case EvtBounce:
		return "BOUNCE", true
	case EvtAck:
		return "ACK", true
	case EvtAutoresponder:
		return "AWAITING_ACK", true
	case EvtVerificationRequested:
		return "AWAITING_USER_ACTION", true
	case EvtVerificationProvided:
		return "AWAITING_RESPONSE", true
	case EvtHumanActionRequired:
		return "AWAITING_USER_ACTION", true
	case EvtConfirmed:
		return "CONFIRMED", true
	case EvtRejectedFinal:
		return "REJECTED_FINAL", true
	case EvtConfirmationLinkClicked:
		return "CONFIRMED", true
	case EvtRebuttalSent:
		return "AWAITING_RESPONSE", true
	case EvtReminderSent:
		return "AWAITING_ACK", true
	case EvtDeadlineReached:
		return "OVERDUE", true
	case EvtDPAComplaintDrafted:
		return "ESCALATED", true
	case EvtDPAComplaintFiled:
		return "DPA_FILED", true
	case EvtRescanTriggered:
		return "RE_SCAN_DUE", true
	case EvtReplyDrafted:
		// Python mapping doesn't include REPLY_DRAFTED; treat as no
		// status change so the projection stays in whatever it was.
		return "", false
	case EvtNoteAdded:
		return "", false
	}
	return "", false
}

// newBlankState is the §4 blank state used before any events.
func newBlankState(requestID int64) StateJSON {
	return StateJSON{
		CurrentStatus:   "PLANNED",
		LastEventID:     0,
		RemindersSent:   0,
		EscalationLevel: 0,
		RequestID:       requestID,
	}
}

// accumulateState folds an ordered list of events into a state
// value. The order is "as queried" (replay ordering); callers must
// feed it events pre-sorted by (occurred_at ASC, id ASC).  Per-event
// errors (unparseable timestamps / bad JSON) are logged and skipped
// per docs §7.
func accumulateState(requestID int64, events []Event) StateJSON {
	state := newBlankState(requestID)
	for _, ev := range events {
		if err := applyEvent(&state, ev); err != nil {
			slog.Default().Error(
				"eventstore: event replay failed",
				"request_id", requestID,
				"event_id", ev.ID,
				"event_type", string(ev.EventType),
				"err", err,
			)
			continue
		}
	}
	return state
}

// applyEvent is the per-event side-effect block, factored out so
// the accumulator can skip-and-continue on errors.
func applyEvent(state *StateJSON, ev Event) error {
	if ev.EventType == "" {
		return fmt.Errorf("empty event type")
	}
	occurred := ev.OccurredAt
	if occurred.IsZero() {
		return fmt.Errorf("empty occurred_at")
	}
	if newStatus, ok := nextStatus(ev.EventType); ok {
		state.CurrentStatus = newStatus
	}
	state.LastEventID = ev.ID
	state.LastEventAt = isoPtr(occurred)

	switch ev.EventType {
	case EvtSent:
		state.SentAt = isoPtr(occurred)
		days := 30
		if v, ok := ev.Payload["expected_response_days"]; ok {
			if n, ok := toInt(v); ok {
				days = n
			}
		}
		if days < 0 {
			days = 0
		}
		state.DeadlineAt = isoPtr(occurred.Add(time.Duration(days) * 24 * time.Hour))
	case EvtAck:
		state.AcknowledgedAt = isoPtr(occurred)
	case EvtConfirmed, EvtRejectedFinal:
		state.ResolvedAt = isoPtr(occurred)
	case EvtReminderSent:
		count := 0
		if v, ok := ev.Payload["count"]; ok {
			if n, ok := toInt(v); ok {
				count = n
			}
		}
		if count == 0 {
			count = 1
		}
		state.RemindersSent = count
	case EvtDeadlineReached:
		state.EscalationLevel = 1
	case EvtDPAComplaintDrafted:
		state.EscalationLevel = 2
	}
	return nil
}

// isoPtr formats t as the Python iso-format string used by the
// golden fixture. Returns nil when t is the zero value.
func isoPtr(t time.Time) *string {
	if t.IsZero() {
		s := ""
		return &s
	}
	s := timeutil.FormatISO(t)
	return &s
}

// toInt accepts a JSON-decoded value (float64, int, int64, string)
// and tries to extract an integer. Strings are not supported here
// (the Python loader uses int() with no fallback). Booleans are
// not integers.
func toInt(v any) (int, bool) {
	switch x := v.(type) {
	case int:
		return x, true
	case int32:
		return int(x), true
	case int64:
		return int(x), true
	case float64:
		return int(x), true
	case float32:
		return int(x), true
	}
	return 0, false
}

// RebuildState replays the event log for one request and returns the
// rebuilt state. It does NOT write the result — call UpsertState
// (or AppendAndProject) to persist.
func (s *Store) RebuildState(ctx context.Context, requestID int64) (StateJSON, error) {
	events, err := s.LoadEventsForReplay(ctx, requestID)
	if err != nil {
		return StateJSON{}, err
	}
	return accumulateState(requestID, events), nil
}

// UpsertState rebuilds and writes the projection for one request.
func (s *Store) UpsertState(ctx context.Context, requestID int64) (StateJSON, error) {
	tx, err := s.db.BeginTx(ctx, nil)
	if err != nil {
		return StateJSON{}, err
	}
	state, err := upsertStateTx(ctx, tx, requestID)
	if err != nil {
		_ = tx.Rollback()
		return StateJSON{}, err
	}
	if err := tx.Commit(); err != nil {
		return StateJSON{}, err
	}
	return state, nil
}

// upsertStateTx is the shared core: rebuild + INSERT OR REPLACE in
// the 11 named columns in the fixed order from docs/event-store.md
// §7 (so the Go port's projection never silently reorders).
func upsertStateTx(ctx context.Context, tx *sql.Tx, requestID int64) (StateJSON, error) {
	events, err := loadEventsTx(ctx, tx, requestID)
	if err != nil {
		return StateJSON{}, err
	}
	state := accumulateState(requestID, events)
	_, err = tx.ExecContext(ctx,
		`INSERT OR REPLACE INTO request_state
		 (request_id, current_status, last_event_id, last_event_at,
		  sent_at, acknowledged_at, resolved_at, deadline_at,
		  next_action_at, reminders_sent, escalation_level)
		 VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)`,
		state.RequestID,
		state.CurrentStatus,
		state.LastEventID,
		state.LastEventAtString(),
		state.SentAtString(),
		state.AcknowledgedAtString(),
		state.ResolvedAtString(),
		state.DeadlineAtString(),
		state.NextActionAtString(),
		state.RemindersSent,
		state.EscalationLevel,
	)
	if err != nil {
		return StateJSON{}, fmt.Errorf("eventstore: upsert state: %w", err)
	}
	return state, nil
}

func loadEventsTx(ctx context.Context, tx *sql.Tx, requestID int64) ([]Event, error) {
	rows, err := tx.QueryContext(ctx,
		`SELECT id, request_id, occurred_at, recorded_at, event_type, payload_json, source
		 FROM request_events
		 WHERE request_id = ?
		 ORDER BY occurred_at ASC, id ASC`, requestID)
	if err != nil {
		return nil, err
	}
	defer rows.Close()
	var out []Event
	for rows.Next() {
		var r rawEvent
		if err := rows.Scan(&r.ID, &r.RequestID, &r.OccurredAt, &r.RecordedAt, &r.EventType, &r.Payload, &r.Source); err != nil {
			return nil, err
		}
		ev, err := r.toEvent()
		if err != nil {
			slog.Default().Error("eventstore: skipping unparseable event in tx",
				"request_id", requestID, "id", r.ID, "err", err)
			continue
		}
		out = append(out, ev)
	}
	return out, rows.Err()
}

// RebuildAllStates rebuilds request_state for every request that is
// "dirty" (state.last_event_id is null OR behind the latest event).
// Mirrors the Python rebuild_all_states.  Returns the number of
// states rebuilt.
func (s *Store) RebuildAllStates(ctx context.Context, chunkSize int) (int, error) {
	if chunkSize <= 0 {
		chunkSize = 100
	}
	rows, err := s.db.QueryContext(ctx,
		`SELECT DISTINCT r.id AS request_id
		 FROM removal_requests r
		 JOIN request_events e ON e.request_id = r.id
		 LEFT JOIN request_state s ON s.request_id = r.id
		 WHERE s.last_event_id IS NULL OR e.id > s.last_event_id`)
	if err != nil {
		return 0, err
	}
	var dirty []int64
	for rows.Next() {
		var id int64
		if err := rows.Scan(&id); err != nil {
			_ = rows.Close()
			return 0, err
		}
		dirty = append(dirty, id)
	}
	_ = rows.Close()
	if len(dirty) == 0 {
		return 0, nil
	}
	total := 0
	for start := 0; start < len(dirty); start += chunkSize {
		end := start + chunkSize
		if end > len(dirty) {
			end = len(dirty)
		}
		chunk := dirty[start:end]
		for _, rid := range chunk {
			if _, err := s.UpsertState(ctx, rid); err != nil {
				return total, err
			}
			total++
		}
	}
	return total, nil
}

// RequestIDs returns the list of removal_requests.id values, sorted
// ascending.  Used by tests and the conformance check.
func (s *Store) RequestIDs(ctx context.Context) ([]int64, error) {
	rows, err := s.db.QueryContext(ctx, `SELECT id FROM removal_requests ORDER BY id ASC`)
	if err != nil {
		return nil, err
	}
	defer rows.Close()
	var ids []int64
	for rows.Next() {
		var id int64
		if err := rows.Scan(&id); err != nil {
			return nil, err
		}
		ids = append(ids, id)
	}
	return ids, rows.Err()
}

// AllProjections rebuilds every request's projection (ignoring
// dirty-state semantics) — used by the conformance test to compare
// against the golden JSON.
func (s *Store) AllProjections(ctx context.Context) (map[string]StateJSON, error) {
	ids, err := s.RequestIDs(ctx)
	if err != nil {
		return nil, err
	}
	sort.Slice(ids, func(i, j int) bool { return ids[i] < ids[j] })
	out := make(map[string]StateJSON, len(ids))
	for _, id := range ids {
		st, err := s.RebuildState(ctx, id)
		if err != nil {
			return nil, err
		}
		out[fmt.Sprintf("%d", id)] = st
	}
	return out, nil
}

// Pointer helpers for the SQL upsert (sqlite returns nil when the
// column was NULL; we pass nil for that, or the formatted string).
func (s StateJSON) LastEventAtString() any {
	if s.LastEventAt == nil {
		return nil
	}
	return *s.LastEventAt
}
func (s StateJSON) SentAtString() any {
	if s.SentAt == nil {
		return nil
	}
	return *s.SentAt
}
func (s StateJSON) AcknowledgedAtString() any {
	if s.AcknowledgedAt == nil {
		return nil
	}
	return *s.AcknowledgedAt
}
func (s StateJSON) ResolvedAtString() any {
	if s.ResolvedAt == nil {
		return nil
	}
	return *s.ResolvedAt
}
func (s StateJSON) DeadlineAtString() any {
	if s.DeadlineAt == nil {
		return nil
	}
	return *s.DeadlineAt
}
func (s StateJSON) NextActionAtString() any {
	if s.NextActionAt == nil {
		return nil
	}
	return *s.NextActionAt
}

// Package deadlines is the Go port of src/symeraseme/core/deadlines.py —
// the tick engine that drives proactive lifecycle management: due-date
// checks per jurisdiction, reminder backoff, DPA escalation and periodic
// re-scans.
package deadlines

import (
	"context"
	"fmt"
	"strconv"
	"time"

	"github.com/danieljustus/symaira-eraseme/internal/eventstore"
	"github.com/danieljustus/symaira-eraseme/internal/eventstore/timeutil"
)

// Constants mirror the Python module.
const (
	// ReminderDays is the base reminder interval (exponential backoff).
	ReminderDays = 7
	// DPAEscalationDays is how long a request may sit OVERDUE before the
	// DPA complaint is drafted.
	DPAEscalationDays = 14
	// ReScanDays is how long after confirmation a re-scan is due.
	ReScanDays = 90
)

// JurisdictionDeadlines mirrors JURISDICTION_DEADLINES.  Note the Python
// reference keys these by LAW name despite the variable name — parity
// means we keep the identical lookup (get with fallback 30).
var JurisdictionDeadlines = map[string]int{
	"GDPR":   30,
	"CCPA":   45,
	"CPRA":   45,
	"LGPD":   30,
	"PIPEDA": 30,
}

// Action mirrors the Python TickAction dataclass.
type Action struct {
	RequestID     int64
	BrokerID      string
	CampaignID    string
	CurrentStatus string
	ActionType    string
	EventType     string
	Description   string
	Payload       map[string]any
	DryRun        bool
}

// RunTick mirrors core/deadlines.run_tick(): scans removal requests
// whose next_action_at is due (or NULL), evaluates each against its
// current status and returns the actions to perform.  apply is NOT
// called here — the caller decides (dry-run vs apply).
func RunTick(ctx context.Context, repo *eventstore.Repository, opts RunOpts) ([]Action, error) {
	now := opts.ReferenceDate
	if now == nil {
		n := time.Now().UTC()
		now = &n
	}
	batch := opts.BatchSize
	candidates, err := repo.FetchTickCandidates(ctx, timeutil.FormatISO(*now), batch)
	if err != nil {
		return nil, err
	}
	var actions []Action
	for _, c := range candidates {
		actions = append(actions, tickForRequest(c, *now, opts.DryRun)...)
	}
	return actions, nil
}

// RunOpts mirrors the keyword arguments of run_tick.
type RunOpts struct {
	DryRun        bool
	ReferenceDate *time.Time
	BatchSize     int
}

// TickCandidate is the row shape the checks need (aliased from the
// repository so the checks stay decoupled from SQL).
type TickCandidate = eventstore.TickCandidate

// tickForRequest mirrors Python _tick_for_request.
func tickForRequest(req TickCandidate, now time.Time, dryRun bool) []Action {
	status := req.CurrentStatus
	if status == "" {
		status = "PLANNED"
	}
	deadlineDays := JurisdictionDeadlines[req.Jurisdiction]
	if deadlineDays == 0 {
		deadlineDays = 30
	}

	var actions []Action
	sentAt := parse(req.SentAt)
	deadlineAt := parse(req.DeadlineAt)
	resolvedAt := parse(req.ResolvedAt)

	switch status {
	case "AWAITING_ACK":
		if a := checkReminder(req, sentAt, now, req.RemindersSent, dryRun); a != nil {
			actions = append(actions, *a)
		}
	case "AWAITING_RESPONSE":
		if a := checkDeadline(req, sentAt, deadlineAt, deadlineDays, now, dryRun); a != nil {
			actions = append(actions, *a)
		}
	case "OVERDUE":
		if a := checkDPAEscalation(req, deadlineAt, now, req.EscalationLevel, dryRun); a != nil {
			actions = append(actions, *a)
		}
	case "CONFIRMED":
		if a := checkRescan(req, resolvedAt, now, dryRun); a != nil {
			actions = append(actions, *a)
		}
	}
	return actions
}

// checkReminder mirrors Python _check_reminder (exponential backoff).
func checkReminder(req TickCandidate, sentAt *time.Time, now time.Time, remindersSent int, dryRun bool) *Action {
	if sentAt == nil {
		return nil
	}
	daysElapsed := int(now.Sub(*sentAt).Hours() / 24)
	if daysElapsed < ReminderDays {
		return nil
	}
	// Exponential backoff: next reminder at 2^n * REMINDER_DAYS.
	nextThreshold := ReminderDays * (1 << uint(remindersSent)) // 2**reminders_sent
	if daysElapsed < nextThreshold {
		return nil
	}
	return &Action{
		RequestID:     req.ID,
		BrokerID:      req.BrokerID,
		CampaignID:    req.CampaignID,
		CurrentStatus: "AWAITING_ACK",
		ActionType:    "send_reminder",
		EventType:     "REMINDER_SENT",
		Description:   fmt.Sprintf("Send reminder #%d (%dd since sent)", remindersSent+1, daysElapsed),
		Payload:       map[string]any{"days_since_sent": daysElapsed, "count": remindersSent + 1},
		DryRun:        dryRun,
	}
}

// checkDeadline mirrors Python _check_deadline (OVERDUE when the deadline
// passed or is implied from sent_at).
func checkDeadline(req TickCandidate, sentAt, deadlineAt *time.Time, deadlineDays int, now time.Time, dryRun bool) *Action {
	if deadlineAt != nil && !now.Before(*deadlineAt) {
		return &Action{
			RequestID:     req.ID,
			BrokerID:      req.BrokerID,
			CampaignID:    req.CampaignID,
			CurrentStatus: "AWAITING_RESPONSE",
			ActionType:    "mark_overdue",
			EventType:     "DEADLINE_REACHED",
			Description:   fmt.Sprintf("Deadline reached (%dd, passed %s)", deadlineDays, pyTimedelta(now.Sub(*deadlineAt))),
			Payload: map[string]any{
				"deadline_days": deadlineDays,
				"deadline_at":   timeutil.FormatISO(*deadlineAt),
			},
			DryRun: dryRun,
		}
	}
	if sentAt != nil && deadlineAt == nil {
		// Parity: Python timedelta(days=n) is exactly n*86400s — NOT
		// calendar-based, so DST transitions never shift the deadline.
		effective := sentAt.Add(time.Duration(deadlineDays) * 24 * time.Hour)
		if !now.Before(effective) {
			return &Action{
				RequestID:     req.ID,
				BrokerID:      req.BrokerID,
				CampaignID:    req.CampaignID,
				CurrentStatus: "AWAITING_RESPONSE",
				ActionType:    "mark_overdue",
				EventType:     "DEADLINE_REACHED",
				Description:   fmt.Sprintf("Deadline reached (%dd from sent)", deadlineDays),
				Payload: map[string]any{
					"deadline_days": deadlineDays,
					"deadline_at":   timeutil.FormatISO(effective),
				},
				DryRun: dryRun,
			}
		}
	}
	return nil
}

// checkDPAEscalation mirrors Python _check_dpa_escalation.
func checkDPAEscalation(req TickCandidate, deadlineAt *time.Time, now time.Time, escalationLevel int, dryRun bool) *Action {
	if escalationLevel >= 2 {
		return nil
	}
	if deadlineAt == nil {
		return nil
	}
	daysSince := int(now.Sub(*deadlineAt).Hours() / 24)
	if daysSince < DPAEscalationDays {
		return nil
	}
	return &Action{
		RequestID:     req.ID,
		BrokerID:      req.BrokerID,
		CampaignID:    req.CampaignID,
		CurrentStatus: "OVERDUE",
		ActionType:    "draft_dpa_complaint",
		EventType:     "DPA_COMPLAINT_DRAFTED",
		Description:   fmt.Sprintf("DPA complaint ready (%dd since deadline)", daysSince),
		Payload:       map[string]any{"days_since_deadline": daysSince},
		DryRun:        dryRun,
	}
}

// checkRescan mirrors Python _check_rescan.
func checkRescan(req TickCandidate, resolvedAt *time.Time, now time.Time, dryRun bool) *Action {
	if resolvedAt == nil {
		return nil
	}
	daysSince := int(now.Sub(*resolvedAt).Hours() / 24)
	if daysSince < ReScanDays {
		return nil
	}
	return &Action{
		RequestID:     req.ID,
		BrokerID:      req.BrokerID,
		CampaignID:    req.CampaignID,
		CurrentStatus: "CONFIRMED",
		ActionType:    "trigger_rescan",
		EventType:     "RE_SCAN_TRIGGERED",
		Description:   fmt.Sprintf("Re-scan due (%dd since resolution)", daysSince),
		Payload:       map[string]any{"days_since_resolved": daysSince},
		DryRun:        dryRun,
	}
}

// ApplyResult mirrors one entry of Python apply_tick_actions.
type ApplyResult struct {
	RequestID   int64  `json:"request_id"`
	Action      string `json:"action"`
	EventType   string `json:"event_type"`
	Description string `json:"description"`
	Executed    bool   `json:"executed"`
	DryRun      bool   `json:"dry_run,omitempty"`
	Error       string `json:"error,omitempty"`
}

// ApplyTickActions mirrors core/deadlines.apply_tick_actions(): appends
// each action's event (source scheduler), then batch-rebuilds all
// projections.  Dry-run actions are recorded without writing.
func ApplyTickActions(ctx context.Context, store *eventstore.Store, actions []Action) ([]ApplyResult, error) {
	if len(actions) == 0 {
		return []ApplyResult{}, nil
	}
	results := make([]ApplyResult, 0, len(actions))
	for _, a := range actions {
		if a.DryRun {
			results = append(results, ApplyResult{
				RequestID:   a.RequestID,
				Action:      a.ActionType,
				EventType:   a.EventType,
				Description: a.Description,
				Executed:    false,
				DryRun:      true,
			})
			continue
		}
		// AppendAndProject mirrors append_event_and_project with
		// source="scheduler".
		_, _, err := store.AppendAndProject(ctx, a.RequestID, eventstore.EventType(a.EventType), a.Payload, eventstore.SrcScheduler, time.Now().UTC())
		if err != nil {
			results = append(results, ApplyResult{
				RequestID:   a.RequestID,
				Action:      a.ActionType,
				EventType:   a.EventType,
				Description: a.Description,
				Executed:    false,
				Error:       err.Error(),
			})
			continue
		}
		results = append(results, ApplyResult{
			RequestID:   a.RequestID,
			Action:      a.ActionType,
			EventType:   a.EventType,
			Description: a.Description,
			Executed:    true,
		})
	}
	// Batch-rebuild all states (Python rebuild_all_states).
	_, _ = store.RebuildAllStates(ctx, 500)
	return results, nil
}

// parse parses a timestamp string into a UTC time (nil on empty).
func parse(s string) *time.Time {
	if s == "" {
		return nil
	}
	t, err := timeutil.Parse(s)
	if err != nil {
		return nil
	}
	tt := t
	return &tt
}

// pyTimedelta renders a duration the way Python's str(timedelta) does:
// "10 days, 0:00:00" (or "1 day, ..."), and bare "H:MM:SS" under a day.
// The tick descriptions embed this exact representation — byte parity
// with the Python fixture depends on it.
func pyTimedelta(d time.Duration) string {
	total := int64(d / time.Second)
	days := total / 86400
	rem := total % 86400
	h := rem / 3600
	m := (rem % 3600) / 60
	s := rem % 60
	if days > 0 {
		unit := "days"
		if days == 1 {
			unit = "day"
		}
		return fmt.Sprintf("%d %s, %d:%02d:%02d", days, unit, h, m, s)
	}
	return fmt.Sprintf("%d:%02d:%02d", h, m, s)
}

// helper keeping strconv referenced by future tick payload parsing.
var _ = strconv.Itoa

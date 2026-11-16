// Package eventstore (repo.go) is the Go port of
// symeraseme.core.repositories. It exposes a Repository façade
// over Store that mirrors the Python repository surface: campaigns,
// removal requests, events.  The goal is "good enough for the
// event store port" — additional repos land as the corresponding
// Go services are ported.
package eventstore

import (
	"context"
	"database/sql"
	"errors"
	"fmt"
	"time"
)

// Repository wraps a Store with the small set of query helpers
// the Python repositories/ module exposes.  All methods take a
// context.Context for cancellation.
type Repository struct {
	store *Store
}

// NewRepository wraps a Store.
func NewRepository(s *Store) *Repository { return &Repository{store: s} }

// Store returns the wrapped Store.  Useful when callers need a
// raw *sql.DB.
func (r *Repository) Store() *Store { return r.store }

// --------------------------------------------------------------------
// Campaigns
// --------------------------------------------------------------------

// CreateCampaign inserts a campaign; returns false if the id is
// already present.
func (r *Repository) CreateCampaign(ctx context.Context, id, kind, notes string) (bool, error) {
	return r.store.CreateCampaign(ctx, id, kind, notes)
}

// ListCampaigns returns all campaigns, newest first.
func (r *Repository) ListCampaigns(ctx context.Context) ([]map[string]any, error) {
	return r.store.ListCampaigns(ctx)
}

// --------------------------------------------------------------------
// Removal requests
// --------------------------------------------------------------------

// CreateRemovalRequest inserts a removal_request and returns its id.
func (r *Repository) CreateRemovalRequest(
	ctx context.Context,
	brokerID, channel, campaignID, jurisdiction, templateID, identitySnapshotHash string,
) (int64, error) {
	return r.store.CreateRemovalRequest(ctx, brokerID, channel, campaignID, jurisdiction, templateID, identitySnapshotHash)
}

// GetRemovalRequest fetches one removal_request.
func (r *Repository) GetRemovalRequest(ctx context.Context, id int64) (map[string]any, error) {
	return r.store.GetRemovalRequest(ctx, id)
}

// listRequestsQuery is a fixed string (no f-string interpolation) —
// matches Python repositories.requests.list_removal_requests.
const listRequestsQuery = `SELECT r.id, r.broker_id, r.channel, r.campaign_id, r.created_at,
		r.jurisdiction, r.template_id, r.identity_snapshot_hash,
		s.current_status, s.last_event_at, s.sent_at, s.acknowledged_at,
		s.resolved_at, s.deadline_at, s.next_action_at, s.reminders_sent,
		s.escalation_level
	FROM removal_requests r
	LEFT JOIN request_state s ON s.request_id = r.id
	WHERE (? IS NULL OR r.campaign_id = ?)
	  AND (? IS NULL OR s.current_status = ?)
	  AND (? IS NULL OR r.broker_id = ?)
	ORDER BY r.created_at ASC`

// ListRemovalRequests lists requests, with optional filters.
type ListRemovalRequestsOpts struct {
	CampaignID *string
	Status     *string
	BrokerID   *string
	Limit      *int
	Offset     *int
}

func (r *Repository) ListRemovalRequests(ctx context.Context, opts ListRemovalRequestsOpts) ([]map[string]any, error) {
	campaignID := nullableString(opts.CampaignID)
	status := nullableString(opts.Status)
	brokerID := nullableString(opts.BrokerID)
	q := listRequestsQuery
	args := []any{campaignID, campaignID, status, status, brokerID, brokerID}
	if opts.Limit != nil {
		q += " LIMIT ?"
		args = append(args, *opts.Limit)
		if opts.Offset != nil {
			q += " OFFSET ?"
			args = append(args, *opts.Offset)
		}
	} else if opts.Offset != nil {
		q += " LIMIT -1 OFFSET ?"
		args = append(args, *opts.Offset)
	}
	rows, err := r.store.db.QueryContext(ctx, q, args...)
	if err != nil {
		return nil, err
	}
	defer rows.Close()
	var out []map[string]any
	for rows.Next() {
		out = append(out, scanListRow(rows))
	}
	return out, rows.Err()
}

// CountRemovalRequests counts requests, with optional filters.
func (r *Repository) CountRemovalRequests(ctx context.Context, campaignID, status *string) (int, error) {
	q := `SELECT COUNT(*) FROM removal_requests r
	      LEFT JOIN request_state s ON s.request_id = r.id
	      WHERE (? IS NULL OR r.campaign_id = ?)
	        AND (? IS NULL OR s.current_status = ?)`
	c := nullableString(campaignID)
	s := nullableString(status)
	row := r.store.db.QueryRowContext(ctx, q, c, c, s, s)
	var n int
	if err := row.Scan(&n); err != nil {
		return 0, err
	}
	return n, nil
}

// GetActiveMatchableRequests returns requests not in a terminal
// state.  Mirrors the Python get_active_matchable_requests.
func (r *Repository) GetActiveMatchableRequests(ctx context.Context, campaignID *string) ([]map[string]any, error) {
	c := nullableString(campaignID)
	q := `SELECT r.id, r.broker_id, r.channel, r.campaign_id, r.created_at,
		r.jurisdiction, r.template_id, r.identity_snapshot_hash,
		s.current_status, s.last_event_at, s.sent_at, s.acknowledged_at,
		s.resolved_at, s.deadline_at, s.next_action_at, s.reminders_sent,
		s.escalation_level
	FROM removal_requests r
	LEFT JOIN request_state s ON s.request_id = r.id
	WHERE (? IS NULL OR r.campaign_id = ?)
	  AND (s.current_status IS NULL
	       OR s.current_status NOT IN ('CONFIRMED', 'REJECTED_FINAL'))
	ORDER BY r.created_at ASC`
	rows, err := r.store.db.QueryContext(ctx, q, c, c)
	if err != nil {
		return nil, err
	}
	defer rows.Close()
	var out []map[string]any
	for rows.Next() {
		out = append(out, scanListRow(rows))
	}
	return out, rows.Err()
}

// --------------------------------------------------------------------
// Events
// --------------------------------------------------------------------

// AppendEvent validates + appends one event, returning the new id.
func (r *Repository) AppendEvent(
	ctx context.Context,
	requestID int64,
	eventType EventType,
	payload map[string]any,
	source Source,
	occurredAt time.Time,
) (int64, error) {
	return r.store.Append(ctx, requestID, eventType, payload, source, occurredAt)
}

// GetEventsForRequests returns the events for a list of request
// ids, bucketed by request id.  Mirrors the Python
// get_events_for_requests.
func (r *Repository) GetEventsForRequests(ctx context.Context, requestIDs []int64, eventType *EventType) (map[int64][]Event, error) {
	out := make(map[int64][]Event, len(requestIDs))
	for _, id := range requestIDs {
		out[id] = nil
	}
	if len(requestIDs) == 0 {
		return out, nil
	}
	q := `SELECT id, request_id, occurred_at, recorded_at, event_type, payload_json, source
	      FROM request_events
	      WHERE request_id IN (`
	args := make([]any, 0, len(requestIDs)+1)
	for i, id := range requestIDs {
		if i > 0 {
			q += ","
		}
		q += "?"
		args = append(args, id)
	}
	q += ")"
	if eventType != nil {
		q += " AND event_type = ?"
		args = append(args, string(*eventType))
	}
	q += " ORDER BY occurred_at ASC, id ASC"
	rows, err := r.store.db.QueryContext(ctx, q, args...)
	if err != nil {
		return nil, err
	}
	defer rows.Close()
	for rows.Next() {
		var raw rawEvent
		if err := rows.Scan(&raw.ID, &raw.RequestID, &raw.OccurredAt, &raw.RecordedAt, &raw.EventType, &raw.Payload, &raw.Source); err != nil {
			return nil, err
		}
		ev, err := raw.toEvent()
		if err != nil {
			continue
		}
		out[raw.RequestID] = append(out[raw.RequestID], ev)
	}
	return out, rows.Err()
}

// GetEvents returns events for one request, replay-ordered.
func (r *Repository) GetEvents(ctx context.Context, requestID int64, afterEventID int64) ([]Event, error) {
	return r.store.GetEvents(ctx, requestID, afterEventID)
}

// --------------------------------------------------------------------
// Tick candidates (deadline engine)
// --------------------------------------------------------------------

// TickCandidate mirrors one row of repositories/deadlines.fetch_tick_candidates.
type TickCandidate struct {
	ID              int64
	BrokerID        string
	CampaignID      string
	Jurisdiction    string
	CurrentStatus   string
	SentAt          string
	DeadlineAt      string
	NextActionAt    string
	AcknowledgedAt  string
	ResolvedAt      string
	RemindersSent   int
	EscalationLevel int
}

// FetchTickCandidates mirrors repositories/deadlines.fetch_tick_candidates:
// requests whose next_action_at is NULL or due by now_iso, earliest first.
func (r *Repository) FetchTickCandidates(ctx context.Context, nowISO string, batchSize int) ([]TickCandidate, error) {
	q := `SELECT r.id, r.broker_id, r.campaign_id, r.jurisdiction,
	             s.current_status, s.sent_at, s.deadline_at, s.next_action_at,
	             s.acknowledged_at, s.resolved_at, s.reminders_sent,
	             s.escalation_level
	      FROM removal_requests r
	      JOIN request_state s ON s.request_id = r.id
	      WHERE s.next_action_at IS NULL
	         OR s.next_action_at <= ?
	      ORDER BY s.next_action_at ASC`
	args := []any{nowISO}
	if batchSize > 0 {
		q += " LIMIT ?"
		args = append(args, batchSize)
	}
	rows, err := r.store.db.QueryContext(ctx, q, args...)
	if err != nil {
		return nil, err
	}
	defer rows.Close()
	var out []TickCandidate
	for rows.Next() {
		var (
			c          TickCandidate
			nextAction sql.NullString
			sentAt     sql.NullString
			deadlineAt sql.NullString
			ackAt      sql.NullString
			resolvedAt sql.NullString
		)
		if err := rows.Scan(
			&c.ID, &c.BrokerID, &c.CampaignID, &c.Jurisdiction,
			&c.CurrentStatus, &sentAt, &deadlineAt, &nextAction,
			&ackAt, &resolvedAt, &c.RemindersSent, &c.EscalationLevel,
		); err != nil {
			return nil, err
		}
		c.SentAt = nextActionString(sentAt)
		c.DeadlineAt = nextActionString(deadlineAt)
		c.NextActionAt = nextActionString(nextAction)
		c.AcknowledgedAt = nextActionString(ackAt)
		c.ResolvedAt = nextActionString(resolvedAt)
		out = append(out, c)
	}
	return out, rows.Err()
}

func nextActionString(ns sql.NullString) string {
	if ns.Valid {
		return ns.String
	}
	return ""
}

// --------------------------------------------------------------------
// Helpers
// --------------------------------------------------------------------

func nullableString(p *string) any {
	if p == nil {
		return nil
	}
	return *p
}

func nullInt(value sql.NullInt64) int {
	if value.Valid {
		return int(value.Int64)
	}
	return 0
}

func scanListRow(rows *sql.Rows) map[string]any {
	var (
		id                  int64
		brokerID, channel   string
		campaignID, created string
		jurisdiction        string
		templateID          string
		hash                string
		status              sql.NullString
		lastEventAt         sql.NullString
		sentAt              sql.NullString
		ackAt               sql.NullString
		resolvedAt          sql.NullString
		deadlineAt          sql.NullString
		nextActionAt        sql.NullString
		remindersSent       sql.NullInt64
		escalation          sql.NullInt64
	)
	if err := rows.Scan(
		&id, &brokerID, &channel, &campaignID, &created, &jurisdiction,
		&templateID, &hash, &status, &lastEventAt, &sentAt, &ackAt,
		&resolvedAt, &deadlineAt, &nextActionAt, &remindersSent, &escalation,
	); err != nil && !errors.Is(err, sql.ErrNoRows) {
		return map[string]any{"error": fmt.Sprintf("scan: %v", err)}
	}
	return map[string]any{
		"id":                     id,
		"broker_id":              brokerID,
		"channel":                channel,
		"campaign_id":            campaignID,
		"created_at":             created,
		"jurisdiction":           jurisdiction,
		"template_id":            templateID,
		"identity_snapshot_hash": hash,
		"current_status":         nullStr(status),
		"last_event_at":          nullStr(lastEventAt),
		"sent_at":                nullStr(sentAt),
		"acknowledged_at":        nullStr(ackAt),
		"resolved_at":            nullStr(resolvedAt),
		"deadline_at":            nullStr(deadlineAt),
		"next_action_at":         nullStr(nextActionAt),
		"reminders_sent":         nullInt(remindersSent),
		"escalation_level":       nullInt(escalation),
	}
}

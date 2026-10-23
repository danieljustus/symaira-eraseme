// Package replies provides the SQLite reply/draft repository used by triage
// and rebuttal orchestration. Snippets are bounded before persistence.
package replies

import (
	"context"
	"database/sql"
	"strings"

	"github.com/danieljustus/symaira-eraseme/internal/email"
	"github.com/danieljustus/symaira-eraseme/internal/eventstore"
)

var ClassificationsNeedingReply = map[string]struct{}{
	"rejected": {}, "verification": {}, "human_required": {}, "unclear": {},
}

type Reply struct {
	ID                   int64
	RequestID            *int64
	MessageID            string
	ThreadID             string
	ReceivedAt           string
	From                 string
	Subject              string
	Snippet              string
	ClassifiedAs         string
	ClassifierConfidence *float64
	LLMSummary           string
}

type Draft struct {
	ID        int64
	ReplyID   int64
	RequestID *int64
	Body      string
	Subject   string
	CreatedAt string
	SentAt    *string
	Account   string
}

type Repository struct{ Store *eventstore.Store }

func NewRepository(store *eventstore.Store) *Repository { return &Repository{Store: store} }

// Insert implements email.ReplyStore and is idempotent by RFC Message-ID.
func (r *Repository) Insert(ctx context.Context, matched email.MatchedMessage, snippet string) error {
	var requestID any
	if matched.RequestID != nil {
		requestID = *matched.RequestID
	}
	_, err := r.Store.DB().ExecContext(ctx, `INSERT OR IGNORE INTO inbox_replies
		(request_id, message_id, thread_id, from_addr, subject, snippet)
		VALUES (?, ?, ?, ?, ?, ?)`, requestID, messageKey(matched.Message), matched.Message.ThreadID,
		matched.Message.From, matched.Message.Subject, boundSnippet(snippet))
	return err
}

func (r *Repository) InsertReply(ctx context.Context, requestID *int64, messageID, threadID, from, subject, snippet string, classified string) (int64, error) {
	var req any
	if requestID != nil {
		req = *requestID
	}
	res, err := r.Store.DB().ExecContext(ctx, `INSERT OR IGNORE INTO inbox_replies
		(request_id, message_id, thread_id, from_addr, subject, snippet, classified_as)
		VALUES (?, ?, ?, ?, ?, ?, ?)`, req, messageID, threadID, from, subject, boundSnippet(snippet), nullable(classified))
	if err != nil {
		return 0, err
	}
	return res.LastInsertId()
}

func (r *Repository) Get(ctx context.Context, id int64) (*Reply, error) {
	row := r.Store.DB().QueryRowContext(ctx, `SELECT id, request_id, message_id, thread_id,
		received_at, from_addr, subject, snippet, classified_as, classifier_confidence, llm_summary
		FROM inbox_replies WHERE id = ?`, id)
	reply, err := scanReply(row)
	if err == sql.ErrNoRows {
		return nil, nil
	}
	if err != nil {
		return nil, err
	}
	return &reply, nil
}

func (r *Repository) Latest(ctx context.Context, requestID int64, unclassifiedOnly bool) (*Reply, error) {
	query := `SELECT id, request_id, message_id, thread_id, received_at, from_addr, subject,
		snippet, classified_as, classifier_confidence, llm_summary FROM inbox_replies WHERE request_id = ?`
	if unclassifiedOnly {
		query += " AND classified_as IS NULL"
	}
	query += " ORDER BY received_at DESC, id DESC LIMIT 1"
	row := r.Store.DB().QueryRowContext(ctx, query, requestID)
	reply, err := scanReply(row)
	if err == sql.ErrNoRows {
		return nil, nil
	}
	if err != nil {
		return nil, err
	}
	return &reply, nil
}

func (r *Repository) Classify(ctx context.Context, id int64, resultLabel string, confidence float64, summary string) error {
	_, err := r.Store.DB().ExecContext(ctx, `UPDATE inbox_replies SET classified_as = ?,
		classifier_confidence = ?, llm_summary = ? WHERE id = ?`, resultLabel, confidence, summary, id)
	return err
}

func (r *Repository) List(ctx context.Context, status string, requestID *int64) ([]Reply, error) {
	query := `SELECT id, request_id, message_id, thread_id, received_at, from_addr, subject,
		snippet, classified_as, classifier_confidence, llm_summary FROM inbox_replies WHERE 1=1`
	args := []any{}
	if requestID != nil {
		query += " AND request_id = ?"
		args = append(args, *requestID)
	}
	switch status {
	case "needs_reply":
		query += " AND classified_as IN (?, ?, ?, ?) AND id NOT IN (SELECT reply_id FROM reply_drafts WHERE sent_at IS NOT NULL)"
		args = append(args, "rejected", "verification", "human_required", "unclear")
	case "needs_verification":
		query += " AND classified_as = ? AND id NOT IN (SELECT reply_id FROM reply_drafts WHERE sent_at IS NOT NULL)"
		args = append(args, "verification")
	case "drafted":
		query += " AND id IN (SELECT reply_id FROM reply_drafts WHERE sent_at IS NULL)"
	case "sent":
		query += " AND id IN (SELECT reply_id FROM reply_drafts WHERE sent_at IS NOT NULL)"
	case "classified":
		query += " AND classified_as IS NOT NULL"
	case "unclassified":
		query += " AND classified_as IS NULL"
	}
	query += " ORDER BY received_at DESC, id DESC"
	rows, err := r.Store.DB().QueryContext(ctx, query, args...)
	if err != nil {
		return nil, err
	}
	defer rows.Close()
	out := []Reply{}
	for rows.Next() {
		reply, err := scanReply(rows)
		if err != nil {
			return nil, err
		}
		out = append(out, reply)
	}
	return out, rows.Err()
}

func (r *Repository) ExistingDraft(ctx context.Context, replyID int64) (*Draft, error) {
	row := r.Store.DB().QueryRowContext(ctx, `SELECT id, reply_id, request_id, draft_body, subject,
		created_at, sent_at, account FROM reply_drafts WHERE reply_id = ? AND sent_at IS NULL
		ORDER BY created_at DESC, id DESC LIMIT 1`, replyID)
	return scanDraftOrNil(row)
}
func (r *Repository) LatestDraft(ctx context.Context, replyID int64) (*Draft, error) {
	row := r.Store.DB().QueryRowContext(ctx, `SELECT id, reply_id, request_id, draft_body, subject,
		created_at, sent_at, account FROM reply_drafts WHERE reply_id = ?
		ORDER BY created_at DESC, id DESC LIMIT 1`, replyID)
	return scanDraftOrNil(row)
}
func (r *Repository) InsertDraft(ctx context.Context, replyID, requestID int64, body, subject, account string) (int64, error) {
	res, err := r.Store.DB().ExecContext(ctx, `INSERT INTO reply_drafts
		(reply_id, request_id, draft_body, subject, account) VALUES (?, ?, ?, ?, ?)`, replyID, requestID, body, subject, nullable(account))
	if err != nil {
		return 0, err
	}
	return res.LastInsertId()
}
func (r *Repository) MarkDraftSent(ctx context.Context, draftID int64, account string) error {
	_, err := r.Store.DB().ExecContext(ctx, "UPDATE reply_drafts SET sent_at = datetime('now'), account = ? WHERE id = ?", nullable(account), draftID)
	return err
}

func scanReply(row interface{ Scan(...any) error }) (Reply, error) {
	var reply Reply
	var requestID sql.NullInt64
	var confidence sql.NullFloat64
	var classified, summary, thread, from, subject, snippet sql.NullString
	if err := row.Scan(&reply.ID, &requestID, &reply.MessageID, &thread, &reply.ReceivedAt, &from, &subject, &snippet, &classified, &confidence, &summary); err != nil {
		return Reply{}, err
	}
	if requestID.Valid {
		reply.RequestID = &requestID.Int64
	}
	reply.ThreadID, reply.From, reply.Subject, reply.Snippet = thread.String, from.String, subject.String, snippet.String
	reply.ClassifiedAs, reply.LLMSummary = classified.String, summary.String
	if confidence.Valid {
		reply.ClassifierConfidence = &confidence.Float64
	}
	return reply, nil
}
func scanDraftOrNil(row interface{ Scan(...any) error }) (*Draft, error) {
	var d Draft
	var req sql.NullInt64
	var sent, account sql.NullString
	if err := row.Scan(&d.ID, &d.ReplyID, &req, &d.Body, &d.Subject, &d.CreatedAt, &sent, &account); err != nil {
		if err == sql.ErrNoRows {
			return nil, nil
		}
		return nil, err
	}
	if req.Valid {
		d.RequestID = &req.Int64
	}
	if sent.Valid {
		d.SentAt = &sent.String
	}
	d.Account = account.String
	return &d, nil
}
func messageKey(message email.Message) string {
	if message.MessageID != "" {
		return message.MessageID
	}
	return message.ID
}
func boundSnippet(value string) string {
	if len(value) > 2000 {
		return value[:2000]
	}
	return value
}
func nullable(value string) any {
	if strings.TrimSpace(value) == "" {
		return nil
	}
	return value
}

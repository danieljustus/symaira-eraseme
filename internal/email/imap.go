package email

import (
	"bytes"
	"context"
	"errors"
	"fmt"
	"mime"
	"net/mail"
	"regexp"
	"sort"
	"strconv"
	"strings"
	"sync"
	"time"
)

var ErrIMAP = errors.New("email: imap error")

// IMAPDialer and IMAPSession are the complete IMAP side-effect boundary. A
// production adapter can use any maintained IMAP library; tests use a fake
// session and never require credentials or a live mailbox.
type IMAPDialer interface {
	Dial(ctx context.Context, cfg IMAPConfig) (IMAPSession, error)
}

type IMAPSession interface {
	Select(ctx context.Context, folder string) (uidValidity uint32, err error)
	SearchUID(ctx context.Context, uidRange string, since ...time.Time) ([]uint32, error)
	Fetch(ctx context.Context, uids []uint32) ([]FetchedMessage, error)
	Close() error
}

// FetchedMessage is the bounded result of one batch FETCH. Header should be
// RFC 5322 header bytes and Body is the requested plain-text snippet.
type FetchedMessage struct {
	UID          uint32
	Header       []byte
	Body         []byte
	Flags        []string
	InternalDate time.Time
}

// HWMStore persists the last processed UID per host/folder and its
// UIDVALIDITY. UIDVALIDITY changes force a cold start.
type HWMStore interface {
	Get(ctx context.Context, host, folder string) (uidValidity, lastUID *uint32, err error)
	Set(ctx context.Context, host, folder string, uidValidity, lastUID uint32) error
}

type hwmRecord struct{ uidValidity, lastUID uint32 }

// MemoryHWMStore is deterministic and concurrency-safe. It is useful for
// tests and for callers that persist state elsewhere.
type MemoryHWMStore struct {
	mu      sync.Mutex
	records map[string]hwmRecord
}

func NewMemoryHWMStore() *MemoryHWMStore {
	return &MemoryHWMStore{records: make(map[string]hwmRecord)}
}

func (s *MemoryHWMStore) Get(_ context.Context, host, folder string) (*uint32, *uint32, error) {
	s.mu.Lock()
	defer s.mu.Unlock()
	record, ok := s.records[host+"\x00"+folder]
	if !ok {
		return nil, nil, nil
	}
	validity, uid := record.uidValidity, record.lastUID
	return &validity, &uid, nil
}

func (s *MemoryHWMStore) Set(_ context.Context, host, folder string, uidValidity, lastUID uint32) error {
	s.mu.Lock()
	defer s.mu.Unlock()
	if s.records == nil {
		s.records = make(map[string]hwmRecord)
	}
	s.records[host+"\x00"+folder] = hwmRecord{uidValidity: uidValidity, lastUID: lastUID}
	return nil
}

// PollInbox performs one UID-based poll. It selects once, searches from the
// persisted HWM, performs one bounded batch fetch, parses messages, and then
// advances the HWM only after a successful fetch. Callers that also persist
// replies should use InboxService so the HWM is staged until persistence ends.
func PollInbox(ctx context.Context, cfg IMAPConfig, dialer IMAPDialer, state HWMStore) ([]Message, error) {
	if dialer == nil {
		return nil, fmt.Errorf("%w: dialer is nil", ErrIMAP)
	}
	if cfg.Host == "" {
		return nil, fmt.Errorf("%w: host is empty", ErrIMAP)
	}
	if cfg.Folder == "" {
		cfg.Folder = "INBOX"
	}
	if cfg.MaxMessages <= 0 {
		cfg.MaxMessages = 50
	}
	if state == nil {
		state = NewMemoryHWMStore()
	}
	return pollInbox(ctx, cfg, dialer, state)
}

func pollInbox(ctx context.Context, cfg IMAPConfig, dialer IMAPDialer, state HWMStore) ([]Message, error) {
	if cfg.Folder == "" {
		cfg.Folder = "INBOX"
	}
	if cfg.MaxMessages <= 0 {
		cfg.MaxMessages = 50
	}
	session, err := dialer.Dial(ctx, cfg)
	if err != nil {
		return nil, imapError("connect/login failed", err, imapSecrets(cfg)...)
	}
	defer session.Close()

	uidValidity, err := session.Select(ctx, cfg.Folder)
	if err != nil {
		return nil, imapError("folder select failed", err, imapSecrets(cfg)...)
	}
	storedValidity, lastUID, err := state.Get(ctx, cfg.Host, cfg.Folder)
	if err != nil {
		return nil, imapError("read high-water mark failed", err)
	}
	start := uint32(1)
	if storedValidity != nil && lastUID != nil && *storedValidity == uidValidity {
		if *lastUID == ^uint32(0) {
			return nil, imapError("UID range exhausted", errors.New("last UID is UINT32_MAX"))
		}
		start = *lastUID + 1
	}
	uidRange := strconv.FormatUint(uint64(start), 10) + ":*"
	var sinceTime time.Time
	if cfg.SinceDays > 0 {
		sinceTime = time.Now().UTC().AddDate(0, 0, -cfg.SinceDays)
	}
	var uids []uint32
	if sinceTime.IsZero() {
		uids, err = session.SearchUID(ctx, uidRange)
	} else {
		uids, err = session.SearchUID(ctx, uidRange, sinceTime)
	}
	if err != nil {
		return nil, imapError("UID search failed", err, imapSecrets(cfg)...)
	}
	if len(uids) == 0 {
		if err := state.Set(ctx, cfg.Host, cfg.Folder, uidValidity, valueOr(lastUID, 0)); err != nil {
			return nil, imapError("write high-water mark failed", err)
		}
		return nil, nil
	}
	sort.Slice(uids, func(i, j int) bool { return uids[i] < uids[j] })
	if len(uids) > cfg.MaxMessages {
		// Consume the oldest pending window first. Keeping the highest UIDs here
		// would advance the HWM past messages that were never fetched.
		uids = uids[:cfg.MaxMessages]
	}
	messages, err := session.Fetch(ctx, append([]uint32(nil), uids...))
	if err != nil {
		// Do not advance the HWM on a failed fetch. Retrying the same UIDs is
		// required to avoid permanent message loss.
		return nil, imapError("UID fetch failed", err, imapSecrets(cfg)...)
	}
	fetchedByUID := make(map[uint32]FetchedMessage, len(messages))
	for _, fetched := range messages {
		fetchedByUID[fetched.UID] = fetched
	}
	parsed := make([]Message, 0, len(messages))
	var lastHandled uint32
	for _, uid := range uids {
		fetched, ok := fetchedByUID[uid]
		if !ok {
			return nil, imapError("UID fetch omitted requested message", fmt.Errorf("UID %d missing from FETCH response", uid))
		}
		message, parseErr := ParseFetchedMessage(fetched)
		if parseErr != nil {
			return nil, imapError("parse fetched message failed", fmt.Errorf("UID %d: %w", uid, parseErr))
		}
		lastHandled = uid
		if !sinceTime.IsZero() {
			if message.Date == nil || message.Date.Before(sinceTime) {
				continue
			}
		}
		parsed = append(parsed, message)
	}
	if err := state.Set(ctx, cfg.Host, cfg.Folder, uidValidity, lastHandled); err != nil {
		return nil, imapError("write high-water mark failed", err)
	}
	return parsed, nil
}

func valueOr(value *uint32, fallback uint32) uint32 {
	if value != nil {
		return *value
	}
	return fallback
}

var messageIDPattern = regexp.MustCompile(`<[^>]+>`)

// ParseFetchedMessage decodes selected headers and body without logging or
// retaining raw transport bytes.
func ParseFetchedMessage(fetched FetchedMessage) (Message, error) {
	reader, err := mail.ReadMessage(bytes.NewReader(fetched.Header))
	if err != nil {
		return Message{}, err
	}
	decode := func(name string) string {
		value := reader.Header.Get(name)
		if value == "" {
			return ""
		}
		decoded, err := (&mime.WordDecoder{}).DecodeHeader(value)
		if err != nil {
			return value
		}
		return decoded
	}
	messageID := decode("Message-ID")
	references := decode("References")
	threadID := firstMessageID(references)
	if threadID == "" {
		threadID = firstMessageID(decode("In-Reply-To"))
	}
	if threadID == "" {
		threadID = firstMessageID(messageID)
	}
	var parsedDate *time.Time
	if dateValue := reader.Header.Get("Date"); dateValue != "" {
		if value, parseErr := mail.ParseDate(dateValue); parseErr == nil {
			parsedDate = &value
		}
	}
	if parsedDate == nil && !fetched.InternalDate.IsZero() {
		d := fetched.InternalDate
		parsedDate = &d
	}
	return Message{
		ID:        strconv.FormatUint(uint64(fetched.UID), 10),
		Subject:   decode("Subject"),
		From:      decode("From"),
		To:        decode("To"),
		Date:      parsedDate,
		Body:      string(fetched.Body),
		Flags:     append([]string(nil), fetched.Flags...),
		MessageID: messageID,
		ThreadID:  threadID,
		IMAPUID:   fetched.UID,
	}, nil
}

func firstMessageID(value string) string { return messageIDPattern.FindString(value) }

// NormalizeSubject removes the reply/forward prefixes used by common mail
// clients before matching request subjects.
func NormalizeSubject(subject string) string {
	cleaned := strings.TrimSpace(subject)
	for {
		lower := strings.ToLower(cleaned)
		matched := false
		for _, prefix := range []string{"re", "fwd", "aw", "antwort", "réf", "sv", "vs", "wg", "ref"} {
			needle := prefix + ":"
			if strings.HasPrefix(lower, needle) {
				cleaned = strings.TrimSpace(cleaned[len(needle):])
				matched = true
				break
			}
		}
		if !matched {
			return cleaned
		}
	}
}

func SubjectMatches(baseSubject, replySubject string) bool {
	return strings.EqualFold(NormalizeSubject(baseSubject), NormalizeSubject(replySubject))
}

// ParseEmailBody trims and bounds text for inbox persistence.
func ParseEmailBody(body string, maxLength int) string {
	body = strings.TrimSpace(body)
	if maxLength <= 0 {
		maxLength = 500
	}
	if len(body) > maxLength {
		return body[:maxLength] + "..."
	}
	return body
}

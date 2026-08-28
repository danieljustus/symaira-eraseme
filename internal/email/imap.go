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
	SearchUID(ctx context.Context, uidRange string) ([]uint32, error)
	Fetch(ctx context.Context, uids []uint32) ([]FetchedMessage, error)
	Close() error
}

// FetchedMessage is the bounded result of one batch FETCH. Header should be
// RFC 5322 header bytes and Body is the requested plain-text snippet.
type FetchedMessage struct {
	UID    uint32
	Header []byte
	Body   []byte
	Flags  []string
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

func NewMemoryHWMStore() *MemoryHWMStore { return &MemoryHWMStore{records: make(map[string]hwmRecord)} }

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
// advances the HWM. A failed fetch still advances to the requested UID range,
// matching the Python adapter's explicit "UIDs discovered" policy.
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
	session, err := dialer.Dial(ctx, cfg)
	if err != nil {
		return nil, fmt.Errorf("%w: connect/login failed", ErrIMAP)
	}
	defer session.Close()
	uidValidity, err := session.Select(ctx, cfg.Folder)
	if err != nil {
		return nil, fmt.Errorf("%w: folder select failed", ErrIMAP)
	}
	storedValidity, lastUID, err := state.Get(ctx, cfg.Host, cfg.Folder)
	if err != nil {
		return nil, fmt.Errorf("%w: read high-water mark failed", ErrIMAP)
	}
	start := uint32(1)
	if storedValidity != nil && lastUID != nil && *storedValidity == uidValidity {
		start = *lastUID + 1
	}
	uidRange := strconv.FormatUint(uint64(start), 10) + ":*"
	uids, err := session.SearchUID(ctx, uidRange)
	if err != nil {
		return nil, fmt.Errorf("%w: UID search failed", ErrIMAP)
	}
	if len(uids) == 0 {
		return nil, state.Set(ctx, cfg.Host, cfg.Folder, uidValidity, valueOr(lastUID, 0))
	}
	sort.Slice(uids, func(i, j int) bool { return uids[i] < uids[j] })
	if len(uids) > cfg.MaxMessages {
		uids = uids[len(uids)-cfg.MaxMessages:]
	}
	messages, err := session.Fetch(ctx, append([]uint32(nil), uids...))
	maxUID := uids[len(uids)-1]
	if err != nil {
		_ = state.Set(ctx, cfg.Host, cfg.Folder, uidValidity, maxUID)
		return nil, fmt.Errorf("%w: UID fetch failed", ErrIMAP)
	}
	parsed := make([]Message, 0, len(messages))
	for _, fetched := range messages {
		message, parseErr := ParseFetchedMessage(fetched)
		if parseErr != nil {
			continue
		}
		parsed = append(parsed, message)
	}
	if err := state.Set(ctx, cfg.Host, cfg.Folder, uidValidity, maxUID); err != nil {
		return nil, fmt.Errorf("%w: write high-water mark failed", ErrIMAP)
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
	var dateValue = reader.Header.Get("Date")
	var parsedDate *time.Time
	if dateValue != "" {
		if value, parseErr := mail.ParseDate(dateValue); parseErr == nil {
			parsedDate = &value
		}
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

func firstMessageID(value string) string {
	return messageIDPattern.FindString(value)
}

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

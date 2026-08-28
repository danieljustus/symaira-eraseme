package email

import (
	"context"
	"fmt"
	"sort"
	"strings"
)

// PollFolders polls each configured folder and deduplicates by RFC Message-ID,
// matching the Python inbox service's cross-folder behavior.
func PollFolders(ctx context.Context, cfg IMAPConfig, folders []string, dialer IMAPDialer, state HWMStore) ([]Message, error) {
	if len(folders) == 0 {
		folders = []string{"INBOX"}
	}
	seen := make(map[string]struct{})
	var all []Message
	for _, folder := range folders {
		cfg.Folder = folder
		messages, err := PollInbox(ctx, cfg, dialer, state)
		if err != nil {
			return nil, err
		}
		for _, message := range messages {
			key := message.MessageID
			if key == "" {
				key = message.ID
			}
			if _, ok := seen[key]; ok {
				continue
			}
			seen[key] = struct{}{}
			all = append(all, message)
		}
	}
	return all, nil
}

// MatchReplyToRequest prioritizes a recorded thread Message-ID and falls back
// to normalized request subjects. Every input message is returned with an
// explicit thread, subject, or unmatched decision.
func MatchReplyToRequest(messages []Message, requests []RemovalRequest, threadMap map[string]int64) []MatchedMessage {
	subjectIndex := make(map[string]int64, len(requests))
	for _, request := range requests {
		subject := "Data Deletion Request — " + request.BrokerID
		subjectIndex[strings.ToLower(NormalizeSubject(subject))] = request.ID
	}
	out := make([]MatchedMessage, 0, len(messages))
	for _, message := range messages {
		matched := MatchedMessage{Message: message, MatchMethod: "unmatched"}
		if requestID, ok := threadMap[message.ThreadID]; ok && message.ThreadID != "" {
			matched.RequestID = &requestID
			matched.MatchMethod = "thread"
		} else if requestID, ok := subjectIndex[strings.ToLower(NormalizeSubject(message.Subject))]; ok {
			matched.RequestID = &requestID
			matched.MatchMethod = "subject"
		}
		out = append(out, matched)
	}
	return out
}

// ReplyStore is the persistence boundary for inbox replies. Implementations
// should store only bounded snippets and parsed metadata, not credentials or
// raw authentication responses.
type ReplyStore interface {
	Insert(ctx context.Context, reply MatchedMessage, snippet string) error
}

// PollAndMatch is the inbox service core: fetch, correlate, and persist only
// matched/annotated metadata. It is intentionally adapter-agnostic.
func PollAndMatch(ctx context.Context, cfg IMAPConfig, folders []string, dialer IMAPDialer, state HWMStore, requests []RemovalRequest, threadMap map[string]int64, store ReplyStore) ([]MatchedMessage, error) {
	messages, err := PollFolders(ctx, cfg, folders, dialer, state)
	if err != nil {
		return nil, err
	}
	matched := MatchReplyToRequest(messages, requests, threadMap)
	if store != nil {
		for _, reply := range matched {
			if err := store.Insert(ctx, reply, ParseEmailBody(reply.Message.Body, 200)); err != nil {
				return nil, fmt.Errorf("email: persist inbox reply: %w", err)
			}
		}
	}
	return matched, nil
}

// SortMessagesByUID is useful to make fakes and callers deterministic.
func SortMessagesByUID(messages []Message) {
	sort.SliceStable(messages, func(i, j int) bool { return messages[i].IMAPUID < messages[j].IMAPUID })
}

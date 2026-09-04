package email

import (
	"context"
	"fmt"
)

// InboxService coordinates polling, correlation, reply persistence, and HWM
// advancement. The HWM is staged while replies are inserted: a failed insert
// leaves the persisted HWM unchanged, so a retry can safely ingest the batch.
type InboxService struct {
	Dialer IMAPDialer
	State  HWMStore
}

func NewInboxService(dialer IMAPDialer, state HWMStore) *InboxService {
	return &InboxService{Dialer: dialer, State: state}
}

// PollAndMatch fetches and correlates replies, persists them, and commits the
// staged HWM only after every persistence operation succeeds.
func (s *InboxService) PollAndMatch(ctx context.Context, cfg IMAPConfig, folders []string, requests []RemovalRequest, threadMap map[string]int64, store ReplyStore) ([]MatchedMessage, error) {
	if s == nil || s.Dialer == nil {
		return nil, fmt.Errorf("%w: dialer is nil", ErrIMAP)
	}
	state := s.State
	if state == nil {
		state = NewMemoryHWMStore()
	}
	staged := NewStagingHWMStore(state)
	messages, err := pollFoldersWithState(ctx, cfg, folders, s.Dialer, staged)
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
	if err := staged.Commit(ctx); err != nil {
		return nil, fmt.Errorf("email: commit inbox high-water mark: %w", err)
	}
	return matched, nil
}

func pollFoldersWithState(ctx context.Context, cfg IMAPConfig, folders []string, dialer IMAPDialer, state HWMStore) ([]Message, error) {
	if len(folders) == 0 {
		folders = []string{"INBOX"}
	}
	seen := make(map[string]struct{})
	var all []Message
	for _, folder := range folders {
		cfg.Folder = folder
		messages, err := pollInbox(ctx, cfg, dialer, state)
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

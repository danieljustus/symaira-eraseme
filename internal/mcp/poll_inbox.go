package mcp

import (
	"context"
	"encoding/json"
	"fmt"
	"strconv"
	"strings"

	"github.com/danieljustus/symaira-eraseme/internal/email"
	"github.com/danieljustus/symaira-eraseme/internal/eventstore"
	"github.com/danieljustus/symaira-eraseme/internal/identity"
	"github.com/danieljustus/symaira-eraseme/internal/replies"
)

// handlePollInbox is the MCP adapter for the email inbox service. Parsing
// transport arguments and loading repositories stays here; polling, matching,
// persistence ordering, and HWM staging belong to email.InboxService.
func handlePollInbox(ctx context.Context, args map[string]any, opts ContractHandlerOptions) (any, error) {
	usernameArg := getStr(args, "username", "")
	oauthUsernameArg := getStr(args, "oauth2_username", "")
	oauthUsernameOverride := oauthUsernameArg
	if oauthUsernameOverride == "" {
		oauthUsernameOverride = usernameArg
	}
	cfg, err := email.LoadIMAPConfigWithOptions(email.IMAPConfigOptions{
		OAuth2AccessToken: getStr(args, "oauth2_access_token", ""),
		OAuth2Username:    oauthUsernameOverride,
	})
	if err != nil {
		return nil, err
	}
	if h := getStr(args, "host", ""); h != "" {
		cfg.Host = h
	}
	if p := getInt(args, "port", 0); p > 0 {
		cfg.Port = p
	}
	if usernameArg != "" {
		cfg.Username = usernameArg
	}
	if cfg.OAuth2 != nil {
		if oauthUsernameArg != "" {
			cfg.OAuth2.Username = oauthUsernameArg
		} else if usernameArg != "" {
			cfg.OAuth2.Username = usernameArg
		}
	}
	if p := getStr(args, "password", ""); p != "" {
		resolved := p
		if cfg.OAuth2 == nil {
			var resolveErr error
			resolved, resolveErr = identity.ResolveSecret(p, identity.SecretResolver{
				EnvFallback:     "IMAP_PASSWORD",
				KeyringService:  "symeraseme-imap",
				KeyringUsername: "IMAP_PASSWORD",
			})
			if resolveErr != nil {
				return nil, fmt.Errorf("email: cannot resolve IMAP password: %w", resolveErr)
			}
		}
		cfg.Password = resolved
	}
	if val, ok := args["ssl"]; ok {
		if b, ok := val.(bool); ok {
			cfg.UseTLS = b
		}
	} else if val, ok := args["use_tls"]; ok {
		if b, ok := val.(bool); ok {
			cfg.UseTLS = b
		}
	}
	if sd := getInt(args, "since_days", 0); sd > 0 {
		cfg.SinceDays = sd
	} else if s := getInt(args, "since", 0); s > 0 {
		cfg.SinceDays = s
	}
	if mm := getInt(args, "max_messages", 0); mm > 0 {
		cfg.MaxMessages = mm
	}

	var folders []string
	if fVal, ok := args["folders"]; ok && fVal != nil {
		switch v := fVal.(type) {
		case []string:
			folders = append(folders, v...)
		case []any:
			for _, item := range v {
				if s, ok := item.(string); ok && strings.TrimSpace(s) != "" {
					folders = append(folders, strings.TrimSpace(s))
				}
			}
		case string:
			v = strings.TrimSpace(v)
			if v != "" {
				var parsed []string
				if json.Unmarshal([]byte(v), &parsed) == nil && len(parsed) > 0 {
					folders = parsed
				} else {
					for _, part := range strings.Split(v, ",") {
						if s := strings.TrimSpace(part); s != "" {
							folders = append(folders, s)
						}
					}
				}
			}
		}
	}
	if len(folders) == 0 {
		if cfg.Folder != "" {
			folders = []string{cfg.Folder}
		} else {
			folders = []string{"INBOX"}
		}
	}

	campaignID := getStr(args, "campaign_id", "")
	var campaignIDPtr *string
	if campaignID != "" {
		campaignIDPtr = &campaignID
	}

	dialer := opts.IMAPDialer
	if dialer == nil {
		dialer = email.NewNetIMAPDialer()
	}

	store, storeErr := dataStore()
	if storeErr != nil && opts.HWMStore == nil {
		return nil, storeErr
	}
	if store != nil {
		defer store.Close()
	}

	state := opts.HWMStore
	if state == nil && store != nil {
		state = store
	}
	if state == nil {
		state = email.NewMemoryHWMStore()
	}

	var removalReqs []email.RemovalRequest
	threadMap := make(map[string]int64)
	var replyStore email.ReplyStore
	if store != nil {
		replyStore = replies.NewRepository(store)
		repo := eventstore.NewRepository(store)
		activeReqs, err := repo.GetActiveMatchableRequests(ctx, campaignIDPtr)
		if err != nil {
			return nil, err
		}
		reqIDs := make([]int64, 0, len(activeReqs))
		removalReqs = make([]email.RemovalRequest, 0, len(activeReqs))
		for _, req := range activeReqs {
			id, _ := req["id"].(int64)
			brokerID, _ := req["broker_id"].(string)
			reqIDs = append(reqIDs, id)
			removalReqs = append(removalReqs, email.RemovalRequest{ID: id, BrokerID: brokerID})
		}
		if len(reqIDs) > 0 {
			evtSent := eventstore.EvtSent
			eventsByReq, err := repo.GetEventsForRequests(ctx, reqIDs, &evtSent)
			if err != nil {
				return nil, err
			}
			for rid, evts := range eventsByReq {
				for _, ev := range evts {
					if msgID, ok := ev.Payload["message_id"].(string); ok && msgID != "" {
						threadMap[msgID] = rid
					}
				}
			}
		}
	}

	matched, err := email.NewInboxService(dialer, state).PollAndMatch(ctx, cfg, folders, removalReqs, threadMap, replyStore)
	if err != nil {
		return nil, err
	}

	matchedCount := 0
	for _, m := range matched {
		if m.RequestID != nil {
			matchedCount++
		}
	}
	var lines []string
	lines = append(lines, fmt.Sprintf("Fetched %d messages from inbox", len(matched)))
	lines = append(lines, fmt.Sprintf("Matched to requests: %d", matchedCount))
	for _, m := range matched {
		reqIDStr := "unmatched"
		if m.RequestID != nil {
			reqIDStr = strconv.FormatInt(*m.RequestID, 10)
		}
		subj := m.Message.Subject
		if subj == "" {
			subj = "(no subject)"
		}
		lines = append(lines, fmt.Sprintf("  [%s] %s", reqIDStr, subj))
	}
	if len(matched) == 0 {
		lines = append(lines, "No new messages found.")
	}
	return map[string]any{
		"total_fetched": len(matched),
		"total_matched": matchedCount,
		"messages":      matched,
		"message":       strings.Join(lines, "\n"),
	}, nil
}

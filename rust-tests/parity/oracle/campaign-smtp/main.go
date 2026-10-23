// Execute real campaign SMTP transactions in an explicitly owned root.
package main

import (
	"bytes"
	"context"
	"crypto/rand"
	"crypto/sha256"
	"encoding/hex"
	"encoding/json"
	"fmt"
	"os"
	"path/filepath"
	"runtime"
	"strconv"
	"time"

	"github.com/danieljustus/symaira-eraseme/internal/campaign"
	"github.com/danieljustus/symaira-eraseme/internal/email"
	"github.com/danieljustus/symaira-eraseme/internal/eventstore"
	"github.com/danieljustus/symaira-eraseme/internal/identity"
)

func must(err error) {
	if err != nil {
		panic(err)
	}
}

func main() {
	if len(os.Args) == 2 && os.Args[1] == "--transport" {
		var in struct {
			Config     email.SMTPConfig
			Recipients []string
			Raw        []byte
		}
		must(json.NewDecoder(os.Stdin).Decode(&in))
		err := (email.NetSMTPTransport{}).Send(context.Background(), in.Config, in.Recipients, in.Raw)
		message := ""
		if err != nil {
			message = err.Error()
		}
		must(json.NewEncoder(os.Stdout).Encode(map[string]any{"ok": err == nil, "error": message}))
		return
	}
	if len(os.Args) != 3 {
		panic("explicit private directory and SMTP port required")
	}
	root := os.Args[1]
	port, err := strconv.Atoi(os.Args[2])
	must(err)
	ctx := context.Background()
	rand.Reader = bytes.NewReader(bytes.Repeat([]byte{7}, 4096))
	time.Local = time.UTC
	cfg := email.SMTPConfig{Host: "127.0.0.1", Port: port, From: "sender@example.invalid", Timeout: 3 * time.Second}
	store, err := eventstore.Open(filepath.Join(root, "store.sqlite"))
	must(err)
	defer store.Close()
	repo := eventstore.NewRepository(store)
	const id = "smtp-campaign"
	_, err = repo.CreateCampaign(ctx, id, "initial", "")
	must(err)
	ids := []int64{}
	for _, broker := range []string{"failure", "success"} {
		rid, err := repo.CreateRemovalRequest(ctx, broker, "email", id, "DE", "", "")
		must(err)
		ids = append(ids, rid)
		_, _, err = store.AppendAndProject(ctx, rid, eventstore.EvtPlanned, map[string]any{"broker_name": broker, "endpoint": broker + "@example.invalid"}, eventstore.SrcSystem, time.Date(2026, 1, 1, 0, 0, 0, 0, time.UTC))
		must(err)
	}
	identity.SetMasterKey([]byte("0123456789abcdef0123456789abcdef"))
	profile := filepath.Join(root, "identity.enc")
	_, err = identity.SaveProfile(&identity.Profile{FullName: "Oracle Person", EmailAddresses: []string{"oracle@example.invalid"}}, profile)
	must(err)
	result, err := campaign.ExecuteCampaign(ctx, store, id, campaign.ExecuteOpts{ProfilePath: profile, Email: func(ctx context.Context, to, subject, body string) (map[string]string, error) {
		mid, err := email.SendMessage(ctx, cfg, email.EmailMessage{To: to, Subject: subject, Body: body}, nil)
		if err != nil {
			return nil, err
		}
		return map[string]string{"message_id": mid}, nil
	}}, 5)
	must(err)
	events := []map[string]any{}
	for _, rid := range ids {
		rows, err := store.GetEvents(ctx, rid, 0)
		must(err)
		for _, e := range rows {
			events = append(events, map[string]any{"type": e.EventType, "request_id": e.RequestID, "payload": e.Payload, "source": e.Source})
		}
		_, err = store.DB().ExecContext(ctx, "UPDATE removal_requests SET created_at='2026-02-03 04:05:06' WHERE id=?", rid)
		must(err)
		_, err = store.DB().ExecContext(ctx, "UPDATE request_state SET last_event_at='2026-02-04 05:06:07',sent_at='2026-02-05 06:07:08',acknowledged_at='2026-02-06 07:08:09',resolved_at='2026-02-07 08:09:10',deadline_at='2026-02-08 09:10:11',next_action_at='2026-02-09 10:11:12' WHERE request_id=?", rid)
		must(err)
	}
	plan, err := campaign.GetPlan(ctx, repo, id, "")
	must(err)
	sources := map[string]string{}
	for _, p := range []string{"go.mod", "internal/campaign/execution.go", "internal/email/smtp.go", "internal/email/types.go", "internal/eventstore/projection.go", "rust-tests/parity/oracle/campaign-smtp/main.go"} {
		body, err := os.ReadFile(filepath.Join(os.Getenv("SYMERASEME_ORACLE_SOURCE_ROOT"), p))
		must(err)
		h := sha256.Sum256(body)
		sources[p] = hex.EncodeToString(h[:])
	}
	must(json.NewEncoder(os.Stdout).Encode(map[string]any{"schema": "symeraseme.go-oracle.campaign-smtp.v1", "go_version": runtime.Version(), "sources_sha256": sources, "result": result, "events": events, "plan": plan}))
	fmt.Fprintln(os.Stderr, "real Go SMTP campaign complete")
}

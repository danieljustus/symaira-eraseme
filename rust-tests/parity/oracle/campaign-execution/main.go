package main

import (
	"context"
	"encoding/json"
	"fmt"
	"os"
	"path/filepath"
	"runtime"
	"time"

	"github.com/danieljustus/symaira-eraseme/internal/campaign"
	"github.com/danieljustus/symaira-eraseme/internal/eventstore"
	"github.com/danieljustus/symaira-eraseme/internal/identity"
	"github.com/danieljustus/symaira-eraseme/internal/registry"
)

type input struct {
	CampaignID    string `json:"campaign_id"`
	BrokerID      string `json:"broker_id"`
	BrokerName    string `json:"broker_name"`
	Endpoint      string `json:"endpoint"`
	EmailBrokerID string `json:"email_broker_id"`
	EmailEndpoint string `json:"email_endpoint"`
}

type output struct {
	Plan     map[string]any    `json:"plan"`
	Result   map[string]any    `json:"result"`
	Events   []eventSnapshot   `json:"events"`
	Statuses map[string]string `json:"statuses"`
}

type eventSnapshot struct {
	Type      string         `json:"type"`
	RequestID int64          `json:"request_id"`
	Payload   map[string]any `json:"payload"`
}

func main() {
	_, source, _, ok := runtime.Caller(0)
	if !ok {
		fatal("oracle source path unavailable")
	}
	inputBytes, err := os.ReadFile(filepath.Join(filepath.Dir(source), "cases.json"))
	if err != nil {
		fatal(err.Error())
	}
	var in input
	if err := json.Unmarshal(inputBytes, &in); err != nil {
		fatal(err.Error())
	}
	root, err := os.MkdirTemp("", "symeraseme-dom002-oracle-*")
	if err != nil {
		fatal(err.Error())
	}
	defer os.RemoveAll(root)
	if err := os.Setenv("SYMERASEME_DATA_DIR", filepath.Join(root, "data")); err != nil {
		fatal(err.Error())
	}
	ctx := context.Background()
	store, err := eventstore.Open(filepath.Join(root, "store.sqlite"))
	if err != nil {
		fatal(err.Error())
	}
	defer store.Close()
	repo := eventstore.NewRepository(store)
	if _, err := repo.CreateCampaign(ctx, in.CampaignID, "initial", ""); err != nil {
		fatal(err.Error())
	}
	requestID, err := repo.CreateRemovalRequest(ctx, in.BrokerID, "web_form", in.CampaignID, "DE", "", "")
	if err != nil {
		fatal(err.Error())
	}
	emailID, err := repo.CreateRemovalRequest(ctx, in.EmailBrokerID, "email", in.CampaignID, "DE", "", "")
	if err != nil {
		fatal(err.Error())
	}
	_, _, err = store.AppendAndProject(ctx, requestID, eventstore.EvtPlanned,
		map[string]any{"broker_name": in.BrokerName, "endpoint": in.Endpoint}, eventstore.SrcSystem, fixedTime())
	if err != nil {
		fatal(err.Error())
	}
	_, _, err = store.AppendAndProject(ctx, emailID, eventstore.EvtPlanned,
		map[string]any{"broker_name": in.EmailBrokerID, "endpoint": in.EmailEndpoint}, eventstore.SrcSystem, fixedTime())
	if err != nil {
		fatal(err.Error())
	}
	plan, err := campaign.GetPlan(ctx, repo, in.CampaignID, "")
	if err != nil {
		fatal(err.Error())
	}
	normalizePlan(plan)
	brokers, err := registry.LoadFromDir("registry")
	if err != nil {
		fatal(err.Error())
	}
	webForm := campaign.NewWebFormAdapter(brokers, nil)
	webForm.DeferManualTask = true
	identity.SetMasterKey([]byte("0123456789abcdef0123456789abcdef"))
	profilePath := filepath.Join(root, "identity.enc")
	if _, err := identity.SaveProfile(&identity.Profile{
		FullName: "Oracle Person", EmailAddresses: []string{"oracle@example.invalid"},
	}, profilePath); err != nil {
		fatal(err.Error())
	}
	result, err := campaign.ExecuteCampaign(ctx, store, in.CampaignID, campaign.ExecuteOpts{
		WebForm: webForm.Run, ProfilePath: profilePath,
	}, 5)
	if err != nil {
		fatal(err.Error())
	}
	snapshots := make([]eventSnapshot, 0, 5)
	for _, id := range []int64{requestID, emailID} {
		events, err := store.GetEvents(ctx, id, 0)
		if err != nil {
			fatal(err.Error())
		}
		for _, event := range events {
			snapshots = append(snapshots, eventSnapshot{
				Type: string(event.EventType), RequestID: event.RequestID, Payload: event.Payload,
			})
		}
	}
	listed, err := repo.ListRemovalRequests(ctx, eventstore.ListRemovalRequestsOpts{
		CampaignID: &in.CampaignID,
		Status:     stringPtr("SEND_FAILED"),
	})
	if err != nil {
		fatal(err.Error())
	}
	statuses := map[string]string{}
	for _, request := range listed {
		id := request["id"].(int64)
		statuses[fmt.Sprint(id)], _ = request["current_status"].(string)
	}
	resultDoc := output{Plan: plan, Result: result, Events: snapshots, Statuses: statuses}
	if err := json.NewEncoder(os.Stdout).Encode(resultDoc); err != nil {
		fatal(err.Error())
	}
}

func fixedTime() time.Time {
	return time.Date(2026, 1, 1, 0, 0, 0, 0, time.UTC)
}

func stringPtr(value string) *string { return &value }

func normalizePlan(plan map[string]any) {
	rows, _ := plan["requests"].([]map[string]any)
	for _, row := range rows {
		for _, key := range []string{"created_at", "last_event_at", "sent_at", "acknowledged_at", "resolved_at", "deadline_at", "next_action_at"} {
			row[key] = nil
		}
	}
}

func fatal(message string) {
	fmt.Fprintln(os.Stderr, message)
	os.Exit(1)
}

package main

import (
	"context"
	"encoding/json"
	"errors"
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
	PlanBefore         map[string]any    `json:"plan_before"`
	PlanAfter          map[string]any    `json:"plan_after"`
	Result             map[string]any    `json:"result"`
	Events             []eventSnapshot   `json:"events"`
	Statuses           map[string]string `json:"statuses"`
	FakeSendPlanBefore map[string]any    `json:"fake_send_plan_before"`
	FakeSendPlanAfter  map[string]any    `json:"fake_send_plan_after"`
	FakeSendResult     map[string]any    `json:"fake_send_result"`
	FakeSendEvents     []eventSnapshot   `json:"fake_send_events"`
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
	if err := pinRequestTimestamps(ctx, store, requestID, emailID); err != nil {
		fatal(err.Error())
	}
	planBefore, err := campaign.GetPlan(ctx, repo, in.CampaignID, "")
	if err != nil {
		fatal(err.Error())
	}
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
	if err := pinRequestTimestamps(ctx, store, requestID, emailID); err != nil {
		fatal(err.Error())
	}
	planAfter, err := campaign.GetPlan(ctx, repo, in.CampaignID, "")
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
	if _, err := repo.CreateCampaign(ctx, "dom002-fake-send", "initial", ""); err != nil {
		fatal(err.Error())
	}
	failureID, err := repo.CreateRemovalRequest(ctx, "fake-failure", "email", "dom002-fake-send", "DE", "", "")
	if err != nil {
		fatal(err.Error())
	}
	successID, err := repo.CreateRemovalRequest(ctx, "fake-success", "email", "dom002-fake-send", "DE", "", "")
	if err != nil {
		fatal(err.Error())
	}
	for _, item := range []struct {
		id       int64
		brokerID string
		endpoint string
	}{
		{failureID, "fake-failure", "failure@example.invalid"},
		{successID, "fake-success", "success@example.invalid"},
	} {
		if _, _, err := store.AppendAndProject(ctx, item.id, eventstore.EvtPlanned,
			map[string]any{"broker_name": item.brokerID, "endpoint": item.endpoint}, eventstore.SrcSystem, fixedTime()); err != nil {
			fatal(err.Error())
		}
	}
	if err := pinRequestTimestamps(ctx, store, failureID, successID); err != nil {
		fatal(err.Error())
	}
	fakePlanBefore, err := campaign.GetPlan(ctx, repo, "dom002-fake-send", "")
	if err != nil {
		fatal(err.Error())
	}
	fakeResult, err := campaign.ExecuteCampaign(ctx, store, "dom002-fake-send", campaign.ExecuteOpts{
		ProfilePath: profilePath,
		Email: func(_ context.Context, to, _, _ string) (map[string]string, error) {
			if to == "failure@example.invalid" {
				return nil, errors.New("synthetic send failure")
			}
			return map[string]string{"message_id": "fake-42"}, nil
		},
	}, 5)
	if err != nil {
		fatal(err.Error())
	}
	if err := pinRequestTimestamps(ctx, store, failureID, successID); err != nil {
		fatal(err.Error())
	}
	fakePlanAfter, err := campaign.GetPlan(ctx, repo, "dom002-fake-send", "")
	if err != nil {
		fatal(err.Error())
	}
	fakeEvents := make([]eventSnapshot, 0, 4)
	for _, id := range []int64{failureID, successID} {
		events, err := store.GetEvents(ctx, id, 0)
		if err != nil {
			fatal(err.Error())
		}
		for _, event := range events {
			fakeEvents = append(fakeEvents, eventSnapshot{
				Type: string(event.EventType), RequestID: event.RequestID, Payload: event.Payload,
			})
		}
	}
	resultDoc := output{
		PlanBefore: planBefore, PlanAfter: planAfter, Result: result, Events: snapshots, Statuses: statuses,
		FakeSendPlanBefore: fakePlanBefore, FakeSendPlanAfter: fakePlanAfter,
		FakeSendResult: fakeResult, FakeSendEvents: fakeEvents,
	}
	if err := json.NewEncoder(os.Stdout).Encode(resultDoc); err != nil {
		fatal(err.Error())
	}
}

func fixedTime() time.Time {
	return time.Date(2026, 1, 1, 0, 0, 0, 0, time.UTC)
}

func stringPtr(value string) *string { return &value }

func pinRequestTimestamps(ctx context.Context, store *eventstore.Store, ids ...int64) error {
	const created = "2026-02-03 04:05:06"
	const lastEvent = "2026-02-04 05:06:07"
	const sent = "2026-02-05 06:07:08"
	const acknowledged = "2026-02-06 07:08:09"
	const resolved = "2026-02-07 08:09:10"
	const deadline = "2026-02-08 09:10:11"
	const nextAction = "2026-02-09 10:11:12"
	for _, id := range ids {
		if _, err := store.DB().ExecContext(ctx, "UPDATE removal_requests SET created_at = ? WHERE id = ?", created, id); err != nil {
			return err
		}
		if _, err := store.DB().ExecContext(ctx, `
			UPDATE request_state SET last_event_at = ?, sent_at = ?, acknowledged_at = ?,
				resolved_at = ?, deadline_at = ?, next_action_at = ? WHERE request_id = ?`,
			lastEvent, sent, acknowledged, resolved, deadline, nextAction, id); err != nil {
			return err
		}
	}
	return nil
}

func fatal(message string) {
	fmt.Fprintln(os.Stderr, message)
	os.Exit(1)
}

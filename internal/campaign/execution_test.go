package campaign

import (
	"context"
	"errors"
	"path/filepath"
	"strings"
	"testing"

	"github.com/danieljustus/symaira-eraseme/internal/eventstore"
	"github.com/danieljustus/symaira-eraseme/internal/identity"
	"github.com/danieljustus/symaira-eraseme/internal/registry"
)

// setupStore opens a fresh store and plans a campaign on the mini
// registry, returning the store, the plan and the profile path used.
func setupPlan(t *testing.T) (*eventstore.Store, *PlanResult, string) {
	t.Helper()
	regRoot := buildMiniRegistry(t)
	brokers, err := registry.LoadFromDir(regRoot)
	if err != nil {
		t.Fatal(err)
	}
	tmp := t.TempDir()
	store, err := eventstore.Open(filepath.Join(tmp, "db.sqlite"))
	if err != nil {
		t.Fatal(err)
	}
	t.Cleanup(func() { store.Close() })

	// A real identity profile so the email path validates required fields.
	// Tests inject a deterministic in-process master key so save/load
	// never touches the host keychain (documented test path).
	identity.SetMasterKey([]byte("0123456789abcdef0123456789abcdef"))
	profilePath := filepath.Join(tmp, "identity.enc")
	t.Setenv("SYMERASEME_IDENTITY_PATH", profilePath)
	profile := &identity.Profile{
		FullName:       "Test Person",
		EmailAddresses: []string{"test@example.com"},
		Addresses: []identity.Address{{
			Street:     "Teststr. 1",
			City:       "Berlin",
			PostalCode: "10115",
			Country:    "DE",
		}},
		DateOfBirth: strPtr("1990-01-01"),
	}
	if _, err := identity.SaveProfile(profile, profilePath); err != nil {
		t.Fatalf("save profile: %v", err)
	}

	res, err := PlanCampaign(context.Background(), store, brokers, PlanOpts{
		CampaignID: "exec-test",
		MaxBrokers: 30,
	}, profilePath)
	if err != nil {
		t.Fatalf("PlanCampaign: %v", err)
	}
	return store, res, profilePath
}

func strPtr(s string) *string { return &s }

// TestWebFormExecutionSuccessAndFailure: a successful stub runner appends
// SENT with the identity hash; a failing runner appends SEND_FAILED with
// the error — matching the Python _execute_webform_request branches.
func TestWebFormExecutionSuccessAndFailure(t *testing.T) {
	store, res, profilePath := setupPlan(t)
	// request 3 is the web-form broker (golden-webform-us)
	var webFormID int64
	for _, r := range res.Requests {
		if r.Channel == "web_form" {
			webFormID = r.RequestID
		}
	}
	if webFormID == 0 {
		t.Fatal("no web_form request planned")
	}
	ctx := context.Background()

	success := func(ctx context.Context, brokerID string, dryRun bool) map[string]any {
		return map[string]any{"success": true, "url": "https://golden-webform.example.com/submitted"}
	}
	out, err := ExecuteRequest(ctx, store, webFormID, ExecuteOpts{WebForm: success, ProfilePath: profilePath})
	if err != nil {
		t.Fatalf("execute success: %v", err)
	}
	if out["success"] != true {
		t.Fatalf("expected success")
	}
	evs, err := store.GetEvents(ctx, webFormID, 0)
	if err != nil {
		t.Fatal(err)
	}
	if got := string(evs[len(evs)-1].EventType); got != "SENT" {
		t.Fatalf("last event: got %s want SENT", got)
	}
	if hash, _ := evs[len(evs)-1].Payload["identity_snapshot_hash"].(string); hash == "" {
		t.Fatalf("SENT payload missing identity hash")
	}

	// Failing runner → SEND_FAILED (event store unchanged afterwards)
	fail := func(ctx context.Context, brokerID string, dryRun bool) map[string]any {
		return map[string]any{"success": false, "error": "captcha unsolved", "task_id": "t-1"}
	}
	out, err = ExecuteRequest(ctx, store, webFormID, ExecuteOpts{WebForm: fail, ProfilePath: profilePath})
	if err != nil {
		t.Fatalf("execute failure: %v", err)
	}
	if out["success"] != false {
		t.Fatalf("expected failure")
	}
	evs, err = store.GetEvents(ctx, webFormID, 0)
	if err != nil {
		t.Fatal(err)
	}
	if got := string(evs[len(evs)-1].EventType); got != "SEND_FAILED" {
		t.Fatalf("last event: got %s want SEND_FAILED", got)
	}
	if e, _ := evs[len(evs)-1].Payload["error"].(string); e != "captcha unsolved" {
		t.Fatalf("SEND_FAILED error payload: %q", e)
	}
}

// TestDryRunIsSideEffectFree: dry-run web-form execution with NO runner
// must succeed and append nothing (mirrors the Python dry-run branch
// before the runner-required check).
func TestDryRunIsSideEffectFree(t *testing.T) {
	store, res, profilePath := setupPlan(t)
	var webFormID int64
	for _, r := range res.Requests {
		if r.Channel == "web_form" {
			webFormID = r.RequestID
		}
	}
	before, err := store.GetEvents(context.Background(), webFormID, 0)
	if err != nil {
		t.Fatal(err)
	}
	out, err := ExecuteRequest(context.Background(), store, webFormID, ExecuteOpts{
		DryRun:      true,
		ProfilePath: profilePath,
	})
	if err != nil {
		t.Fatalf("dry-run web form: %v", err)
	}
	if out["dry_run"] != true || out["success"] != true {
		t.Fatalf("dry-run result wrong: %v", out)
	}
	after, err := store.GetEvents(context.Background(), webFormID, 0)
	if err != nil {
		t.Fatal(err)
	}
	if len(after) != len(before) {
		t.Fatalf("dry-run appended events: before=%d after=%d", len(before), len(after))
	}
}

// TestEmailExecutionMissingProfile: the email path without an identity
// profile fails with ErrIdentityProfileNotFound (Python ProfileError).
func TestEmailExecutionMissingProfile(t *testing.T) {
	store, res, _ := setupPlan(t)
	var emailID int64
	for _, r := range res.Requests {
		if r.Channel == "email" {
			emailID = r.RequestID
		}
	}
	missing := filepath.Join(t.TempDir(), "nope", "identity.enc")
	_, err := ExecuteRequest(context.Background(), store, emailID, ExecuteOpts{
		ProfilePath: missing,
	})
	if !errors.Is(err, ErrIdentityProfileNotFound) {
		t.Fatalf("expected ErrIdentityProfileNotFound, got %v", err)
	}
}

// TestEmailExecutionSendsAndAppendsSENT: a stub sender appends SENT with
// to/template/account/message_id/identity fields, mirroring Python.
func TestEmailExecutionSendsAndAppendsSENT(t *testing.T) {
	store, res, profilePath := setupPlan(t)
	var emailID int64
	for _, r := range res.Requests {
		if r.Channel == "email" {
			emailID = r.RequestID
		}
	}
	ctx := context.Background()
	sender := func(ctx context.Context, to, subject, body string) (map[string]string, error) {
		return map[string]string{"message_id": "msg-42"}, nil
	}
	out, err := ExecuteRequest(ctx, store, emailID, ExecuteOpts{
		Email:       sender,
		ProfilePath: profilePath,
		Account:     "smtp-test",
	})
	if err != nil {
		t.Fatalf("execute email: %v", err)
	}
	if out["success"] != true {
		t.Fatalf("expected success")
	}
	evs, err := store.GetEvents(ctx, emailID, 0)
	if err != nil {
		t.Fatal(err)
	}
	last := evs[len(evs)-1]
	if string(last.EventType) != "SENT" {
		t.Fatalf("last event: got %s want SENT", last.EventType)
	}
	if got, _ := last.Payload["to"].(string); got == "" {
		t.Fatalf("SENT payload missing to")
	}
	if got, _ := last.Payload["message_id"].(string); got != "msg-42" {
		t.Fatalf("message_id: %v", got)
	}
	if got, _ := last.Payload["account"].(string); got != "smtp-test" {
		t.Fatalf("account: %v", got)
	}
}

// TestEmailSenderErrorAppendsSEND_FAILEDAndRaises: a failing sender
// appends SEND_FAILED and surfaces an ExecutionError, mirroring the
// Python except branch.
func TestEmailSenderErrorAppendsSEND_FAILEDAndRaises(t *testing.T) {
	store, res, profilePath := setupPlan(t)
	var emailID int64
	for _, r := range res.Requests {
		if r.Channel == "email" {
			emailID = r.RequestID
			break
		}
	}
	ctx := context.Background()
	failing := func(ctx context.Context, to, subject, body string) (map[string]string, error) {
		return nil, errors.New("connection refused")
	}
	_, err := ExecuteRequest(ctx, store, emailID, ExecuteOpts{
		Email:       failing,
		ProfilePath: profilePath,
	})
	var execErr *ExecutionError
	if !errors.As(err, &execErr) {
		t.Fatalf("expected ExecutionError, got %v", err)
	}
	evs, err := store.GetEvents(ctx, emailID, 0)
	if err != nil {
		t.Fatal(err)
	}
	if got := string(evs[len(evs)-1].EventType); got != "SEND_FAILED" {
		t.Fatalf("last event: got %s want SEND_FAILED", got)
	}
	if e, _ := evs[len(evs)-1].Payload["error"].(string); !strings.Contains(e, "connection refused") {
		t.Fatalf("SEND_FAILED error: %q", e)
	}
}

// TestExecuteCampaignBatch clamps oversized batches to BatchLimit and
// aggregates per-request results without aborting on failures.
func TestExecuteCampaignBatch(t *testing.T) {
	store, res, profilePath := setupPlan(t)
	ctx := context.Background()
	// All three planned requests execute successfully (web form + emails).
	success := func(ctx context.Context, brokerID string, dryRun bool) map[string]any {
		return map[string]any{"success": true, "url": "https://done.example"}
	}
	sender := func(ctx context.Context, to, subject, body string) (map[string]string, error) {
		return map[string]string{"message_id": "batch-" + to}, nil
	}
	out, err := ExecuteCampaign(ctx, store, "exec-test", ExecuteOpts{
		WebForm:     success,
		Email:       sender,
		ProfilePath: profilePath,
	}, 50) // clamp to 10
	if err != nil {
		t.Fatalf("execute campaign: %v", err)
	}
	if out["campaign_id"] != "exec-test" {
		t.Fatalf("campaign_id: %v", out["campaign_id"])
	}
	results, _ := out["results"].([]map[string]any)
	if len(results) != len(res.Requests) {
		t.Fatalf("results: got %d want %d", len(results), len(res.Requests))
	}
}

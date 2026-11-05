package mcp

import (
	"context"
	"os"
	"path/filepath"
	"testing"

	"github.com/danieljustus/symaira-eraseme/internal/campaign"
	"github.com/danieljustus/symaira-eraseme/internal/identity"
	"github.com/danieljustus/symaira-eraseme/internal/manualtasks"
)

func TestContractHandlerManualTaskListIsImplemented(t *testing.T) {
	t.Setenv("SYMERASEME_DATA_DIR", t.TempDir())

	result, err := ContractHandler()(context.Background(), "manual_tasks_list", map[string]any{})
	if err != nil {
		t.Fatalf("manual_tasks_list returned error: %v", err)
	}
	payload, ok := result.(manualtasks.Result)
	if !ok {
		t.Fatalf("manual_tasks_list result type = %T, want manualtasks.Result", result)
	}
	if !payload.Success {
		t.Fatalf("manual_tasks_list success = false: %#v", payload)
	}
}

func TestContractHandlerAcceptsNativeIntegerArguments(t *testing.T) {
	t.Setenv("SYMERASEME_DATA_DIR", t.TempDir())

	result, err := ContractHandler()(context.Background(), "plan_create", map[string]any{
		"campaign_id": "integer-arguments",
		"max_brokers": 1,
	})
	if err != nil {
		t.Fatalf("plan_create returned error: %v", err)
	}
	plan, ok := result.(*campaign.PlanResult)
	if !ok {
		t.Fatalf("plan_create result type = %T, want *campaign.PlanResult", result)
	}
	if plan.Planned > 1 {
		t.Fatalf("plan_create planned %d brokers, want at most 1", plan.Planned)
	}
}

func TestContractHandlerListRequestsPaginates(t *testing.T) {
	t.Setenv("SYMERASEME_DATA_DIR", t.TempDir())

	result, err := ContractHandler()(context.Background(), "list_requests", map[string]any{
		"page":      2,
		"page_size": 3,
	})
	if err != nil {
		t.Fatalf("list_requests returned error: %v", err)
	}
	payload, ok := result.(map[string]any)
	if !ok {
		t.Fatalf("list_requests result type = %T, want map[string]any", result)
	}
	if payload["page"] != 2 || payload["page_size"] != 3 {
		t.Fatalf("pagination payload = %#v", payload)
	}
	if payload["total"] != 0 {
		t.Fatalf("empty request total = %#v, want 0", payload["total"])
	}
}

func TestContractHandlerSchedulerDryRunDoesNotWrite(t *testing.T) {
	t.Setenv("SYMERASEME_DATA_DIR", t.TempDir())
	outputDir := filepath.Join(t.TempDir(), "schedules")

	result, err := ContractHandler()(context.Background(), "generate_scheduler", map[string]any{
		"output_dir": outputDir,
		"platform":   "cron",
		"tick_hour":  6,
		"poll_hours": "6,12",
		"dry_run":    true,
	})
	if err != nil {
		t.Fatalf("generate_scheduler dry-run returned error: %v", err)
	}
	payload, ok := result.(map[string]any)
	if !ok || payload["success"] != true || payload["dry_run"] != true {
		t.Fatalf("unexpected scheduler dry-run payload: %#v", result)
	}
	if _, err := os.Stat(outputDir); !os.IsNotExist(err) {
		t.Fatalf("scheduler dry-run created output directory: %v", err)
	}
}

func TestContractHandlerGrantRevokeAll(t *testing.T) {
	t.Setenv("SYMERASEME_DATA_DIR", t.TempDir())
	ctx := context.Background()

	for _, command := range []string{"execute", "review"} {
		if _, err := ContractHandler()(ctx, "grant", map[string]any{"command": command, "ttl": 3600}); err != nil {
			t.Fatalf("grant %s returned error: %v", command, err)
		}
	}
	before, err := identity.ListTokens()
	if err != nil {
		t.Fatalf("list tokens before revoke-all: %v", err)
	}
	if len(before) != 2 {
		t.Fatalf("tokens before revoke-all = %d, want 2", len(before))
	}

	if _, err := ContractHandler()(ctx, "grant", map[string]any{"revoke_all": true}); err != nil {
		t.Fatalf("grant revoke-all returned error: %v", err)
	}
	after, err := identity.ListTokens()
	if err != nil {
		t.Fatalf("list tokens after revoke-all: %v", err)
	}
	if len(after) != 0 {
		t.Fatalf("tokens after revoke-all = %d, want 0", len(after))
	}
}

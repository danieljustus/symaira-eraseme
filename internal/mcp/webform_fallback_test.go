package mcp

import (
	"context"
	"os"
	"testing"

	"github.com/danieljustus/symaira-eraseme/internal/config"
	"github.com/danieljustus/symaira-eraseme/internal/eventstore"
	"github.com/danieljustus/symaira-eraseme/internal/manualtasks"
	"github.com/danieljustus/symaira-eraseme/internal/registry"
)

func TestRunWebFormProductionFallbackPersistsAndDryRunsWithoutWrite(t *testing.T) {
	dataDir := t.TempDir()
	t.Setenv("SYMERASEME_DATA_DIR", dataDir)
	t.Setenv("PATH", t.TempDir())
	brokers, err := registry.LoadEmbedded()
	if err != nil {
		t.Fatal(err)
	}
	brokerID := ""
	for _, broker := range brokers {
		for _, channel := range broker.OptOut {
			if channel.Type == "web_form" && channel.FormSpec != nil && (channel.Disabled == nil || !*channel.Disabled) {
				brokerID = broker.ID
				break
			}
		}
		if brokerID != "" {
			break
		}
	}
	if brokerID == "" {
		t.Fatal("embedded registry has no active web-form broker")
	}
	ctx := context.Background()
	dry, err := ContractHandler()(ctx, "run_web_form", map[string]any{"broker_id": brokerID, "dry_run": true})
	if err != nil {
		t.Fatal(err)
	}
	dryResult, ok := dry.(map[string]any)
	if !ok || dryResult["success"] != true || dryResult["dry_run"] != true {
		t.Fatalf("dry-run result = %#v", dry)
	}
	storage, err := config.ResolveStorage()
	if err != nil {
		t.Fatal(err)
	}
	if _, err := os.Stat(storage.DBPath); !os.IsNotExist(err) {
		t.Fatalf("dry-run created database %q: %v", storage.DBPath, err)
	}

	run, err := ContractHandler()(ctx, "run_web_form", map[string]any{"broker_id": brokerID, "dry_run": false})
	if err != nil {
		t.Fatal(err)
	}
	result, ok := run.(map[string]any)
	if !ok || result["success"] != false || result["status"] != "manual_action_required" || result["task_id"] == nil {
		t.Fatalf("production fallback result = %#v", run)
	}
	store, err := eventstore.Open(storage.DBPath)
	if err != nil {
		t.Fatal(err)
	}
	defer store.Close()
	tasks, err := manualtasks.List(ctx, store, manualtasks.ListOpts{})
	if err != nil || len(tasks) != 1 || tasks[0].Status != "pending" || tasks[0].Reason != "dynamic_form" {
		t.Fatalf("manual tasks = %#v, err=%v", tasks, err)
	}
}

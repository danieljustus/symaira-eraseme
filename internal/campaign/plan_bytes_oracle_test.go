package campaign

import (
	"context"
	"crypto/sha256"
	"encoding/hex"
	"encoding/json"
	"os"
	"path/filepath"
	"testing"

	"github.com/danieljustus/symaira-eraseme/internal/eventstore"
	"github.com/danieljustus/symaira-eraseme/internal/identity"
	"github.com/danieljustus/symaira-eraseme/internal/registry"
)

const planBytesOraclePath = "tests/fixtures/event-store/campaign-plan-bytes-oracle.json"

type planBytesOracle struct {
	Schema           string             `json:"schema"`
	GeneratorSHA256  string             `json:"generator_sha256"`
	Sources          []sourceDigest     `json:"sources"`
	PlanCampaignJSON string             `json:"plan_campaign_json"`
	GetPlanJSON      string             `json:"get_plan_json"`
	CampaignRowJSON  string             `json:"campaign_row_json"`
	RequestRowsJSON  []string           `json:"request_rows_json"`
	Events           []eventBytesOracle `json:"events"`
}

type sourceDigest struct {
	Path   string `json:"path"`
	SHA256 string `json:"sha256"`
}

type eventBytesOracle struct {
	RequestID   int64  `json:"request_id"`
	OccurredAt  string `json:"occurred_at"`
	RecordedAt  string `json:"recorded_at"`
	EventType   string `json:"event_type"`
	Source      string `json:"source"`
	PayloadJSON string `json:"payload_json"`
}

func TestCampaignPlanBytesOracle(t *testing.T) {
	root := repoRoot(t)
	registryRoot := buildMiniRegistry(t)
	brokers, err := registry.LoadFromDir(registryRoot)
	if err != nil {
		t.Fatal(err)
	}
	if len(brokers) != 3 {
		t.Fatalf("mini registry: got %d brokers, want 3", len(brokers))
	}
	tree := t.TempDir()
	store, err := eventstore.Open(filepath.Join(tree, "store.sqlite"))
	if err != nil {
		t.Fatal(err)
	}
	defer store.Close()
	if err := identity.SetMasterKey([]byte("0123456789abcdef0123456789abcdef")); err != nil {
		t.Fatal(err)
	}
	t.Cleanup(func() { _ = identity.SetMasterKey(nil) })
	profilePath := filepath.Join(tree, "identity.enc")
	profile := &identity.Profile{
		FullName:       "Oracle Person",
		EmailAddresses: []string{"oracle@example.invalid"},
		Addresses: []identity.Address{{
			Street: "1 Test Street", City: "Berlin", PostalCode: "10115", Country: "DE",
		}},
		DateOfBirth: strPtr("1990-01-01"),
	}
	if _, err := identity.SaveProfile(profile, profilePath); err != nil {
		t.Fatal(err)
	}
	ctx := context.Background()
	plan, err := PlanCampaign(ctx, store, brokers, PlanOpts{
		CampaignID: "campaign-plan-byte-oracle",
		MaxBrokers: 30,
		Notes:      "raw byte oracle",
	}, profilePath)
	if err != nil {
		t.Fatal(err)
	}
	// PlanCampaign owns its production clock; freeze its persisted clock columns
	// after the real call so the read-model bytes stay stable across runs.
	const pinnedAt = "2026-08-06 12:00:00"
	if _, err := store.DB().ExecContext(ctx, "UPDATE campaigns SET created_at = ? WHERE id = ?", pinnedAt, plan.CampaignID); err != nil {
		t.Fatal(err)
	}
	if _, err := store.DB().ExecContext(ctx, "UPDATE removal_requests SET created_at = ?", pinnedAt); err != nil {
		t.Fatal(err)
	}
	if _, err := store.DB().ExecContext(ctx, "UPDATE request_events SET occurred_at = ?, recorded_at = ?", pinnedAt, pinnedAt); err != nil {
		t.Fatal(err)
	}
	if _, err := store.DB().ExecContext(ctx, "UPDATE request_state SET last_event_at = ?", pinnedAt); err != nil {
		t.Fatal(err)
	}
	getPlan, err := GetPlan(ctx, eventstore.NewRepository(store), plan.CampaignID, "")
	if err != nil {
		t.Fatal(err)
	}
	planBytes, err := json.Marshal(plan)
	if err != nil {
		t.Fatal(err)
	}
	getPlanBytes, err := json.Marshal(getPlan)
	if err != nil {
		t.Fatal(err)
	}
	oracle := planBytesOracle{
		Schema:           "symeraseme.campaign.plan-bytes-oracle.v1",
		PlanCampaignJSON: string(planBytes),
		GetPlanJSON:      string(getPlanBytes),
	}
	var row struct {
		ID        string `json:"id"`
		CreatedAt string `json:"created_at"`
		Kind      string `json:"kind"`
		Notes     string `json:"notes"`
	}
	if err := store.DB().QueryRowContext(ctx,
		"SELECT id, CAST(created_at AS TEXT), kind, notes FROM campaigns WHERE id = ?", plan.CampaignID,
	).Scan(&row.ID, &row.CreatedAt, &row.Kind, &row.Notes); err != nil {
		t.Fatal(err)
	}
	encoded, err := json.Marshal(row)
	if err != nil {
		t.Fatal(err)
	}
	oracle.CampaignRowJSON = string(encoded)
	requestRows, err := store.DB().QueryContext(ctx, `
		SELECT id, broker_id, channel, campaign_id, CAST(created_at AS TEXT), jurisdiction,
			template_id, identity_snapshot_hash FROM removal_requests ORDER BY id`)
	if err != nil {
		t.Fatal(err)
	}
	for requestRows.Next() {
		var request struct {
			ID                   int64  `json:"id"`
			BrokerID             string `json:"broker_id"`
			Channel              string `json:"channel"`
			CampaignID           string `json:"campaign_id"`
			CreatedAt            string `json:"created_at"`
			Jurisdiction         string `json:"jurisdiction"`
			TemplateID           string `json:"template_id"`
			IdentitySnapshotHash string `json:"identity_snapshot_hash"`
		}
		if err := requestRows.Scan(&request.ID, &request.BrokerID, &request.Channel, &request.CampaignID,
			&request.CreatedAt, &request.Jurisdiction, &request.TemplateID, &request.IdentitySnapshotHash); err != nil {
			t.Fatal(err)
		}
		encoded, err := json.Marshal(request)
		if err != nil {
			t.Fatal(err)
		}
		oracle.RequestRowsJSON = append(oracle.RequestRowsJSON, string(encoded))
	}
	if err := requestRows.Close(); err != nil {
		t.Fatal(err)
	}
	events, err := store.DB().QueryContext(ctx, `
		SELECT request_id, CAST(occurred_at AS TEXT), CAST(recorded_at AS TEXT), event_type, source, payload_json
		FROM request_events ORDER BY id`)
	if err != nil {
		t.Fatal(err)
	}
	for events.Next() {
		var event eventBytesOracle
		if err := events.Scan(&event.RequestID, &event.OccurredAt, &event.RecordedAt,
			&event.EventType, &event.Source, &event.PayloadJSON); err != nil {
			t.Fatal(err)
		}
		oracle.Events = append(oracle.Events, event)
	}
	if err := events.Close(); err != nil {
		t.Fatal(err)
	}
	for _, path := range []string{
		"internal/campaign/campaign.go",
		"internal/campaign/planning.go",
		"internal/eventstore/repo.go",
		"internal/eventstore/store.go",
		"internal/eventstore/projection.go",
		"internal/identity/profile.go",
		"internal/identity/secrets.go",
		"internal/registry/loader.go",
		"tests/fixtures/registry-contract/golden-email-eu.yaml",
		"tests/fixtures/registry-contract/golden-multi-uk.yaml",
		"tests/fixtures/registry-contract/golden-webform-us.yaml",
	} {
		source, err := os.ReadFile(filepath.Join(root, path))
		if err != nil {
			t.Fatal(err)
		}
		digest := sha256.Sum256(source)
		oracle.Sources = append(oracle.Sources, sourceDigest{Path: path, SHA256: hex.EncodeToString(digest[:])})
	}
	generator, err := os.ReadFile(filepath.Join(root, "internal/campaign/plan_bytes_oracle_test.go"))
	if err != nil {
		t.Fatal(err)
	}
	generatorDigest := sha256.Sum256(generator)
	oracle.GeneratorSHA256 = hex.EncodeToString(generatorDigest[:])
	fixturePath := filepath.Join(root, planBytesOraclePath)
	actual, err := json.MarshalIndent(oracle, "", "  ")
	if err != nil {
		t.Fatal(err)
	}
	actual = append(actual, '\n')
	if os.Getenv("UPDATE_GO_CAMPAIGN_PLAN_ORACLE") == "1" {
		if err := os.WriteFile(fixturePath, actual, 0o644); err != nil {
			t.Fatal(err)
		}
		return
	}
	want, err := os.ReadFile(fixturePath)
	if err != nil {
		t.Fatal(err)
	}
	if string(actual) != string(want) {
		t.Fatalf("Go campaign raw-byte oracle changed; regenerate %s after review", planBytesOraclePath)
	}
}

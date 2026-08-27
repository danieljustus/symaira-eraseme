package campaign

import (
	"context"
	"encoding/json"
	"os"
	"path/filepath"
	"testing"

	"github.com/danieljustus/symaira-eraseme/internal/eventstore"
	"github.com/danieljustus/symaira-eraseme/internal/registry"
)

// repoRoot walks up from the package dir to the module root.
func repoRoot(t *testing.T) string {
	t.Helper()
	dir, err := os.Getwd()
	if err != nil {
		t.Fatal(err)
	}
	for {
		if _, err := os.Stat(filepath.Join(dir, "go.mod")); err == nil {
			return dir
		}
		parent := filepath.Dir(dir)
		if parent == dir {
			t.Fatal("go.mod not found")
		}
		dir = parent
	}
}

// buildMiniRegistry constructs the fixed broker subset the golden Python
// fixture ran against (same YAML files, same layout as
// /tmp/golden-plan-registry used by scripts/gen_golden_plan_fixture.py).
func buildMiniRegistry(t *testing.T) string {
	t.Helper()
	root := repoRoot(t)
	src := filepath.Join(root, "tests", "fixtures", "registry-contract")
	dst := t.TempDir()
	for sub, file := range map[string]string{
		"DE": "golden-email-eu.yaml",
		"UK": "golden-multi-uk.yaml",
		"US": "golden-webform-us.yaml",
	} {
		dir := filepath.Join(dst, "brokers", sub)
		if err := os.MkdirAll(dir, 0o755); err != nil {
			t.Fatal(err)
		}
		b, err := os.ReadFile(filepath.Join(src, file))
		if err != nil {
			t.Fatal(err)
		}
		if err := os.WriteFile(filepath.Join(dir, file), b, 0o644); err != nil {
			t.Fatal(err)
		}
	}
	return dst
}

// goldenPlanFixture loads the Python-generated golden plan fixture.
type goldenPlan struct {
	CampaignID   string          `json:"campaign_id"`
	TotalBrokers int             `json:"total_brokers"`
	Matched      int             `json:"matched"`
	Planned      int             `json:"planned"`
	PlanRequests []planReqGolden `json:"plan_requests"`
	DBRequests   []dbReqGolden   `json:"db_requests"`
	Events       []eventGolden   `json:"events"`
}

type planReqGolden struct {
	RequestID  string `json:"request_id"`
	BrokerID   string `json:"broker_id"`
	BrokerName string `json:"broker_name"`
	Channel    string `json:"channel"`
	Template   string `json:"template"`
}

type dbReqGolden struct {
	ID           int64  `json:"id"`
	BrokerID     string `json:"broker_id"`
	Channel      string `json:"channel"`
	CampaignID   string `json:"campaign_id"`
	Jurisdiction string `json:"jurisdiction"`
	TemplateID   string `json:"template_id"`
	IdentityHash string `json:"identity_snapshot_hash"`
}

type eventGolden struct {
	RequestID int64          `json:"request_id"`
	EventType string         `json:"event_type"`
	Source    string         `json:"source"`
	Payload   map[string]any `json:"payload"`
}

func loadGoldenPlan(t *testing.T) *goldenPlan {
	t.Helper()
	b, err := os.ReadFile(filepath.Join(repoRoot(t), "tests", "fixtures", "event-store", "golden-plan.json"))
	if err != nil {
		t.Fatalf("read golden plan: %v", err)
	}
	var gp goldenPlan
	if err := json.Unmarshal(b, &gp); err != nil {
		t.Fatalf("parse golden plan: %v", err)
	}
	return &gp
}

// TestGoldenPlanConformance replays the Python-generated golden plan: the
// Go planner must produce the same removal_requests and the same PLANNED
// event payloads from the identical registry subset.  Byte-identical
// fields (broker/channel/template resolution, jurisdiction, payload)
// prove the port, not just plausible parity.
func TestGoldenPlanConformance(t *testing.T) {
	regRoot := buildMiniRegistry(t)
	brokers, err := registry.LoadFromDir(regRoot)
	if err != nil {
		t.Fatalf("load mini registry: %v", err)
	}
	if len(brokers) != 3 {
		t.Fatalf("mini registry: got %d brokers, want 3", len(brokers))
	}

	tmp := t.TempDir()
	store, err := eventstore.Open(filepath.Join(tmp, "db.sqlite"))
	if err != nil {
		t.Fatalf("open store: %v", err)
	}
	defer store.Close()

	// A deliberately missing identity profile: planning tolerates it and
	// records an empty snapshot hash, exactly like the Python fixture run.
	missingProfile := filepath.Join(tmp, "nope", "identity.enc")
	res, err := PlanCampaign(context.Background(), store, brokers, PlanOpts{
		CampaignID: "golden-plan",
		MaxBrokers: 30,
	}, missingProfile)
	if err != nil {
		t.Fatalf("PlanCampaign: %v", err)
	}

	golden := loadGoldenPlan(t)

	if res.CampaignID != golden.CampaignID {
		t.Errorf("campaign_id: got %q want %q", res.CampaignID, golden.CampaignID)
	}
	if res.TotalBrokers != golden.TotalBrokers {
		t.Errorf("total_brokers: got %d want %d", res.TotalBrokers, golden.TotalBrokers)
	}
	if res.Matched != golden.Matched {
		t.Errorf("matched: got %d want %d", res.Matched, golden.Matched)
	}
	if res.Planned != golden.Planned {
		t.Errorf("planned: got %d want %d", res.Planned, golden.Planned)
	}
	if len(res.Requests) != len(golden.PlanRequests) {
		t.Fatalf("requests: got %d want %d", len(res.Requests), len(golden.PlanRequests))
	}

	for i, want := range golden.PlanRequests {
		got := res.Requests[i]
		if got.BrokerID != want.BrokerID {
			t.Errorf("req[%d].broker_id: got %q want %q", i, got.BrokerID, want.BrokerID)
		}
		if got.BrokerName != want.BrokerName {
			t.Errorf("req[%d].broker_name: got %q want %q", i, got.BrokerName, want.BrokerName)
		}
		if got.Channel != want.Channel {
			t.Errorf("req[%d].channel: got %q want %q", i, got.Channel, want.Channel)
		}
		if got.Template != want.Template {
			t.Errorf("req[%d].template: got %q want %q", i, got.Template, want.Template)
		}
	}

	ctx := context.Background()
	repo := eventstore.NewRepository(store)
	for i, want := range golden.DBRequests {
		req, err := repo.GetRemovalRequest(ctx, want.ID)
		if err != nil {
			t.Fatalf("get request %d: %v", want.ID, err)
		}
		if req == nil {
			t.Fatalf("request %d missing", want.ID)
		}
		if got := req["broker_id"]; got != want.BrokerID {
			t.Errorf("db[%d].broker_id: got %v want %q", i, got, want.BrokerID)
		}
		if got := req["channel"]; got != want.Channel {
			t.Errorf("db[%d].channel: got %v want %q", i, got, want.Channel)
		}
		if got := req["campaign_id"]; got != want.CampaignID {
			t.Errorf("db[%d].campaign_id: got %v want %q", i, got, want.CampaignID)
		}
		if got := req["jurisdiction"]; got != want.Jurisdiction {
			t.Errorf("db[%d].jurisdiction: got %v want %q", i, got, want.Jurisdiction)
		}
		if got := req["template_id"]; got != want.TemplateID {
			t.Errorf("db[%d].template_id: got %v want %q", i, got, want.TemplateID)
		}
		if got := req["identity_snapshot_hash"]; got != want.IdentityHash {
			t.Errorf("db[%d].identity_snapshot_hash: got %v want %q", i, got, want.IdentityHash)
		}
	}

	// PLANNED event payloads must be byte-identical to Python's — one
	// event per request, in request order.
	var gotEvents []eventstore.Event
	for _, w := range golden.Events {
		evs, err := store.GetEvents(ctx, w.RequestID, 0)
		if err != nil {
			t.Fatal(err)
		}
		if len(evs) != 1 {
			t.Fatalf("request %d: got %d events, want 1 PLANNED", w.RequestID, len(evs))
		}
		gotEvents = append(gotEvents, evs[0])
	}
	if len(gotEvents) != len(golden.Events) {
		t.Fatalf("events: got %d want %d", len(gotEvents), len(golden.Events))
	}
	for i, ev := range gotEvents {
		want := golden.Events[i]
		if string(ev.EventType) != want.EventType {
			t.Errorf("event[%d].type: got %q want %q", i, ev.EventType, want.EventType)
		}
		if string(ev.Source) != want.Source {
			t.Errorf("event[%d].source: got %q want %q", i, ev.Source, want.Source)
		}
		gotJSON, _ := json.Marshal(ev.Payload)
		wantJSON, _ := json.Marshal(want.Payload)
		if string(gotJSON) != string(wantJSON) {
			t.Errorf("event[%d] payload mismatch\n got: %s\nwant: %s", i, gotJSON, wantJSON)
		}
	}
}

// TestFilterBrokersSemantics pins the load_all_brokers filter behaviour
// (status default active, include_inactive opt-out, disabled handling).
func TestFilterBrokersSemantics(t *testing.T) {
	boolPtr := func(b bool) *bool { return &b }
	brokers := []registry.Broker{
		{ID: "active-one", Status: "active", Category: "marketing"},
		{ID: "deprecated-one", Status: "deprecated", Category: "marketing"},
		{ID: "disabled-one", Status: "active", Category: "credit", Disabled: boolPtr(true)},
	}

	got := registry.FilterBrokers(brokers, registry.BrokerFilter{})
	if len(got) != 1 || got[0].ID != "active-one" {
		t.Fatalf("default filter: got %d brokers (%v), want active-only", len(got), got)
	}

	got = registry.FilterBrokers(brokers, registry.BrokerFilter{IncludeInactive: true})
	if len(got) != 2 {
		t.Fatalf("include_inactive: got %d brokers, want 2 (disabled still excluded)", len(got))
	}

	got = registry.FilterBrokers(brokers, registry.BrokerFilter{IncludeDisabled: true, IncludeInactive: true})
	if len(got) != 3 {
		t.Fatalf("include_disabled+inactive: got %d brokers, want 3", len(got))
	}

	got = registry.FilterBrokers(brokers, registry.BrokerFilter{Category: "credit"})
	if len(got) != 0 {
		t.Fatalf("category filter: got %d, want 0 (only disabled credit)", len(got))
	}
}

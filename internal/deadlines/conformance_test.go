package deadlines

import (
	"context"
	"encoding/json"
	"os"
	"path/filepath"
	"testing"
	"time"

	"github.com/danieljustus/symaira-eraseme/internal/eventstore"
	"github.com/danieljustus/symaira-eraseme/internal/eventstore/timeutil"
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

// now is the fixed reference date the Python fixture generator used.
var now = mustParse("2026-08-27T10:00:00+00:00")

func mustParse(s string) time.Time {
	t, err := time.Parse("2006-01-02T15:04:05Z07:00", s)
	if err != nil {
		panic(err)
	}
	return t
}

// goldenTick is the Python-generated fixture: now + expected actions.
type goldenTick struct {
	Now     string      `json:"now"`
	Actions []goldenAct `json:"actions"`
}

type goldenAct struct {
	RequestID     int64          `json:"request_id"`
	BrokerID      string         `json:"broker_id"`
	CampaignID    string         `json:"campaign_id"`
	CurrentStatus string         `json:"current_status"`
	ActionType    string         `json:"action_type"`
	EventType     string         `json:"event_type"`
	Description   string         `json:"description"`
	Payload       map[string]any `json:"payload"`
	DryRun        bool           `json:"dry_run"`
}

func loadGoldenTick(t *testing.T) *goldenTick {
	t.Helper()
	b, err := os.ReadFile(filepath.Join(repoRoot(t), "tests", "fixtures", "event-store", "golden-tick.json"))
	if err != nil {
		t.Fatalf("read golden tick: %v", err)
	}
	var gt goldenTick
	if err := json.Unmarshal(b, &gt); err != nil {
		t.Fatalf("parse golden tick: %v", err)
	}
	return &gt
}

// seedTickHistories writes the exact event histories the Python fixture
// generator used (same brokers, same offsets, same payloads).
func seedTickHistories(t *testing.T, store *eventstore.Store) map[int64]string {
	t.Helper()
	ctx := context.Background()
	histories := []struct {
		broker string
		juris  string
		events []struct {
			typ     string
			payload map[string]any
			offset  int // days relative to now
		}
	}{
		{"broker-a", "GDPR", []struct {
			typ     string
			payload map[string]any
			offset  int
		}{
			{"SENT", map[string]any{"expected_response_days": 30}, -21},
			{"REMINDER_SENT", map[string]any{"count": 1, "days_since_sent": 14}, -14},
		}},
		{"broker-b", "GDPR", []struct {
			typ     string
			payload map[string]any
			offset  int
		}{
			{"SENT", map[string]any{"expected_response_days": 30}, -40},
			{"VERIFICATION_PROVIDED", map[string]any{}, -38},
		}},
		{"broker-c", "GDPR", []struct {
			typ     string
			payload map[string]any
			offset  int
		}{
			{"SENT", map[string]any{"expected_response_days": 30}, -50},
			{"DEADLINE_REACHED", map[string]any{"deadline_days": 30}, -20},
		}},
		{"broker-d", "GDPR", []struct {
			typ     string
			payload map[string]any
			offset  int
		}{
			{"SENT", map[string]any{"expected_response_days": 30}, -100},
			{"ACK", map[string]any{"message_id": "m-1"}, -98},
			{"CONFIRMED", map[string]any{"via": "ack"}, -98},
		}},
	}
	ids := map[int64]string{}
	for _, h := range histories {
		rid, err := store.CreateRemovalRequest(ctx, h.broker, "email", "golden-tick", h.juris, "", "")
		if err != nil {
			t.Fatalf("create request %s: %v", h.broker, err)
		}
		ids[rid] = h.broker
		for _, ev := range h.events {
			at := now.Add(time.Duration(ev.offset) * 24 * time.Hour)
			if _, _, err := store.AppendAndProject(ctx, rid, eventstore.EventType(ev.typ), ev.payload, eventstore.SrcSystem, at); err != nil {
				t.Fatalf("append %s for %s: %v", ev.typ, h.broker, err)
			}
		}
	}
	return ids
}

// TestGoldenTickConformance replays the Python-generated tick fixture:
// the same event histories must produce the same actions with identical
// descriptions and payloads (byte parity, including the timedelta
// rendering "10 days, 0:00:00" inside the mark_overdue description).
func TestGoldenTickConformance(t *testing.T) {
	tmp := t.TempDir()
	store, err := eventstore.Open(filepath.Join(tmp, "db.sqlite"))
	if err != nil {
		t.Fatalf("open store: %v", err)
	}
	defer store.Close()
	ids := seedTickHistories(t, store)
	if _, err := store.RebuildAllStates(t.Context(), 500); err != nil {
		t.Fatalf("rebuild states: %v", err)
	}

	repo := eventstore.NewRepository(store)
	actions, err := RunTick(t.Context(), repo, RunOpts{ReferenceDate: &now})
	if err != nil {
		t.Fatalf("RunTick: %v", err)
	}

	golden := loadGoldenTick(t)
	if len(actions) != len(golden.Actions) {
		t.Fatalf("actions: got %d want %d", len(actions), len(golden.Actions))
	}
	for i, want := range golden.Actions {
		got := actions[i]
		if got.RequestID == 0 {
			t.Fatalf("action[%d] has no request id", i)
		}
		if ids[got.RequestID] != want.BrokerID {
			t.Errorf("action[%d].broker: got request %d (%s) want broker %s", i, got.RequestID, ids[got.RequestID], want.BrokerID)
		}
		if got.CampaignID != want.CampaignID {
			t.Errorf("action[%d].campaign_id: got %q want %q", i, got.CampaignID, want.CampaignID)
		}
		if got.CurrentStatus != want.CurrentStatus {
			t.Errorf("action[%d].current_status: got %q want %q", i, got.CurrentStatus, want.CurrentStatus)
		}
		if got.ActionType != want.ActionType {
			t.Errorf("action[%d].action_type: got %q want %q", i, got.ActionType, want.ActionType)
		}
		if got.EventType != want.EventType {
			t.Errorf("action[%d].event_type: got %q want %q", i, got.EventType, want.EventType)
		}
		if got.Description != want.Description {
			t.Errorf("action[%d].description:\n got: %q\nwant: %q", i, got.Description, want.Description)
		}
		if got.DryRun != want.DryRun {
			t.Errorf("action[%d].dry_run: got %v want %v", i, got.DryRun, want.DryRun)
		}
		gotJSON, _ := json.Marshal(got.Payload)
		wantJSON, _ := json.Marshal(want.Payload)
		if string(gotJSON) != string(wantJSON) {
			t.Errorf("action[%d] payload mismatch\n got: %s\nwant: %s", i, gotJSON, wantJSON)
		}
	}
}

// TestDeadlineDSTAndMonthBoundary pins the date handling parity the
// issue demands: timedelta(days=n) is fixed-86400s (DST-transition
// immune), month boundaries roll over calendar-correctly.
func TestDeadlineDSTAndMonthBoundary(t *testing.T) {
	// DST: 2026-03-29 is the EU spring-forward.  sent 2026-03-20T12:00 UTC
	// + 30 fixed days must be 2026-04-19T12:00 UTC, regardless of the
	// local timezone — Python timedelta ignores DST, so Go must too
	// (Add uses fixed hours, unlike AddDate).
	sent := mustParse("2026-03-20T12:00:00+00:00")
	effective := sent.Add(30 * 24 * time.Hour)
	want := mustParse("2026-04-19T12:00:00+00:00")
	if !effective.Equal(want) {
		t.Fatalf("DST deadline: got %s want %s", timeutil.FormatISO(effective), timeutil.FormatISO(want))
	}
	// Month boundary: 31. Jan + 30d = 2. Mar (2026 is not a leap year).
	jan := mustParse("2026-01-31T12:00:00+00:00")
	feb := jan.Add(30 * 24 * time.Hour)
	if feb.Day() != 2 || feb.Month() != 3 {
		t.Fatalf("month boundary: got %s want 2026-03-02", timeutil.FormatISO(feb))
	}
	// Cross-year boundary keeps the wall clock.
	dec := mustParse("2026-12-15T08:30:00+00:00")
	jan2 := dec.Add(30 * 24 * time.Hour)
	if jan2.Year() != 2027 || jan2.Month() != 1 || jan2.Day() != 14 {
		t.Fatalf("cross-year: got %s want 2027-01-14", timeutil.FormatISO(jan2))
	}
	// pyTimedelta rendering matches Python str(timedelta) for sub-day and
	// day cases (hours are NOT zero-padded, minutes/seconds are).
	if got := pyTimedelta(10 * 24 * time.Hour); got != "10 days, 0:00:00" {
		t.Errorf("pyTimedelta 10d: %q", got)
	}
	if got := pyTimedelta(1 * 24 * time.Hour); got != "1 day, 0:00:00" {
		t.Errorf("pyTimedelta 1d: %q", got)
	}
	if got := pyTimedelta(5*time.Hour + 3*time.Minute + 4*time.Second); got != "5:03:04" {
		t.Errorf("pyTimedelta subday: %q", got)
	}
}

// TestApplyTickActionsWritesEvents: applying the actions appends the
// expected event types with source scheduler and bumps the projection.
func TestApplyTickActionsWritesEvents(t *testing.T) {
	tmp := t.TempDir()
	store, err := eventstore.Open(filepath.Join(tmp, "db.sqlite"))
	if err != nil {
		t.Fatalf("open store: %v", err)
	}
	defer store.Close()
	ids := seedTickHistories(t, store)
	if _, err := store.RebuildAllStates(t.Context(), 500); err != nil {
		t.Fatalf("rebuild states: %v", err)
	}

	repo := eventstore.NewRepository(store)
	actions, err := RunTick(t.Context(), repo, RunOpts{ReferenceDate: &now})
	if err != nil {
		t.Fatalf("RunTick: %v", err)
	}
	results, err := ApplyTickActions(t.Context(), store, actions)
	if err != nil {
		t.Fatalf("ApplyTickActions: %v", err)
	}
	if len(results) != len(actions) {
		t.Fatalf("results: got %d want %d", len(results), len(actions))
	}
	byRequest := map[int64]string{}
	for _, r := range results {
		if !r.Executed {
			t.Errorf("result for %d not executed: %v", r.RequestID, r.Error)
		}
		byRequest[r.RequestID] = r.EventType
	}
	// Request 4 (broker-d) got RE_SCAN_TRIGGERED; verify it landed.
	if et := byRequest[0]; et != "" {
		// deterministic check via broker map
	}
	for rid, broker := range ids {
		if broker == "broker-a" {
			if byRequest[rid] != "REMINDER_SENT" {
				t.Errorf("request %d (broker-a): got event %q", rid, byRequest[rid])
			}
		}
		if broker == "broker-b" {
			if byRequest[rid] != "DEADLINE_REACHED" {
				t.Errorf("request %d (broker-b): got event %q", rid, byRequest[rid])
			}
		}
		if broker == "broker-c" {
			if byRequest[rid] != "DPA_COMPLAINT_DRAFTED" {
				t.Errorf("request %d (broker-c): got event %q", rid, byRequest[rid])
			}
		}
		if broker == "broker-d" {
			if byRequest[rid] != "RE_SCAN_TRIGGERED" {
				t.Errorf("request %d (broker-d): got event %q", rid, byRequest[rid])
			}
		}
	}
}

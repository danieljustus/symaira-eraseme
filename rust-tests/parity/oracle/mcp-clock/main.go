// Command mcp-clock is the source-bound Go oracle for the MCP calendar and
// dashboard tools, evaluated against a fixed instant and synthetic stores.
package main

import (
	"context"
	"encoding/json"
	"fmt"
	"os"
	"path/filepath"
	"time"

	"github.com/danieljustus/symaira-eraseme/internal/config"
	"github.com/danieljustus/symaira-eraseme/internal/eventstore"
	"github.com/danieljustus/symaira-eraseme/internal/mcp"
)

const nowText = "2026-08-05T12:00:00Z"

type fixtureCase struct {
	Name      string          `json:"name"`
	State     string          `json:"state"`
	Tool      string          `json:"tool"`
	Arguments map[string]any  `json:"arguments"`
	Result    json.RawMessage `json:"result"`
}

func main() {
	// The helper is also run with hostile inherited config variables by its
	// parity test. Clear them before any path resolution or store access.
	os.Clearenv()
	tempRoot := os.TempDir()
	root, err := os.MkdirTemp("", "mcp-clock-oracle-")
	if err != nil {
		fail(err)
	}
	defer func() {
		_ = os.Chdir(tempRoot)
		_ = os.RemoveAll(root)
	}()
	if err := os.Chdir(root); err != nil {
		fail(err)
	}
	home := filepath.Join(root, "home")
	if err := os.MkdirAll(home, 0o700); err != nil {
		fail(err)
	}
	if err := os.Setenv("HOME", home); err != nil {
		fail(err)
	}
	if err := os.Setenv("USERPROFILE", home); err != nil {
		fail(err)
	}
	for _, name := range []string{"XDG_CONFIG_HOME", "XDG_DATA_HOME", "XDG_STATE_HOME", "XDG_CACHE_HOME"} {
		path := filepath.Join(home, name)
		if err := os.MkdirAll(path, 0o700); err != nil {
			fail(err)
		}
		if err := os.Setenv(name, path); err != nil {
			fail(err)
		}
	}
	data := filepath.Join(root, "data")
	if err := os.Setenv("SYMERASEME_DATA_DIR", data); err != nil {
		fail(err)
	}
	instant, err := time.Parse(time.RFC3339, nowText)
	if err != nil {
		fail(err)
	}
	handler := mcp.ContractHandlerWithOptions(mcp.ContractHandlerOptions{Now: func() time.Time { return instant }})
	cases := []fixtureCase{}
	call := func(name, state, tool string, arguments map[string]any) {
		result, err := handler(context.Background(), tool, arguments)
		if err != nil {
			fail(fmt.Errorf("%s: %w", name, err))
		}
		encoded, err := json.Marshal(result)
		if err != nil {
			fail(err)
		}
		cases = append(cases, fixtureCase{Name: name, State: state, Tool: tool, Arguments: arguments, Result: encoded})
	}

	call("empty_dashboard", "empty", "get_dashboard_data", map[string]any{})
	call("empty_calendar_defaults_to_four_weeks", "empty", "get_calendar", map[string]any{})
	seed(data)
	call("populated_dashboard_counts_request_statuses", "populated", "get_dashboard_data", map[string]any{})
	call("calendar_includes_exact_horizon_and_current_instant", "populated", "get_calendar", map[string]any{"weeks": 1})
	call("calendar_zero_weeks_keeps_past_and_current_markers", "populated", "get_calendar", map[string]any{"weeks": 0})
	call("calendar_filters_campaign", "populated", "get_calendar", map[string]any{"campaign_id": "alpha", "weeks": 1})

	if err := json.NewEncoder(os.Stdout).Encode(struct {
		OracleSource string        `json:"oracle_source"`
		Now          string        `json:"now"`
		Cases        []fixtureCase `json:"cases"`
	}{"live current-checkout Go ContractHandler", nowText, cases}); err != nil {
		fail(err)
	}
}

func seed(data string) {
	if err := os.MkdirAll(data, 0o700); err != nil {
		fail(err)
	}
	storage, err := config.ResolveStorage()
	if err != nil {
		fail(err)
	}
	store, err := eventstore.Open(storage.DBPath)
	if err != nil {
		fail(err)
	}
	defer func() {
		if err := store.Close(); err != nil {
			fail(err)
		}
	}()

	_, err = store.DB().Exec(`
		INSERT INTO campaigns (id, created_at, kind, notes) VALUES
			('alpha', '2026-08-01T12:00:00+00:00', 'initial', NULL),
			('beta',  '2026-08-02T12:00:00+00:00', 'initial', NULL);
		INSERT INTO removal_requests (id, broker_id, channel, campaign_id, created_at, jurisdiction, template_id) VALUES
			(1, 'broker-past', 'email', 'alpha', '2026-08-01T13:00:00+00:00', 'DE', ''),
			(2, 'broker-horizon', 'email', 'alpha', '2026-08-01T14:00:00+00:00', 'DE', ''),
			(3, 'broker-now', 'email', 'alpha', '2026-08-01T15:00:00+00:00', 'DE', ''),
			(4, 'broker-resolved', 'email', 'alpha', '2026-08-01T16:00:00+00:00', 'DE', ''),
			(5, 'broker-after', 'email', 'alpha', '2026-08-01T17:00:00+00:00', 'DE', ''),
			(6, 'broker-beta', 'email', 'beta', '2026-08-02T13:00:00+00:00', 'US', '');
		INSERT INTO request_state (request_id, current_status, last_event_at, sent_at, resolved_at, deadline_at, next_action_at, reminders_sent, escalation_level) VALUES
			(1, 'SENT', '2026-08-01T13:00:00+00:00', '2026-08-01T13:00:00+00:00', NULL, '2026-08-04T12:00:00+00:00', NULL, 0, 0),
			(2, 'AWAITING_RESPONSE', '2026-08-01T14:00:00+00:00', '2026-08-01T14:00:00+00:00', NULL, '2026-08-12T12:00:00+00:00', NULL, 1, 1),
			(3, 'AWAITING_ACK', '2026-08-01T15:00:00+00:00', '2026-08-01T15:00:00+00:00', NULL, '2026-08-20T12:00:00+00:00', '2026-08-05T12:00:00+00:00', 0, 0),
			(4, 'CONFIRMED', '2026-08-01T16:00:00+00:00', '2026-08-01T16:00:00+00:00', '2026-08-03T12:00:00+00:00', '2026-08-04T12:00:00+00:00', NULL, 0, 0),
			(5, 'REJECTED_FINAL', '2026-08-01T17:00:00+00:00', '2026-08-01T17:00:00+00:00', NULL, '2026-08-12T12:00:01+00:00', NULL, 0, 2),
			(6, 'PLANNED', '2026-08-02T13:00:00+00:00', NULL, NULL, '2026-08-06T12:00:00+00:00', NULL, 0, 0);`)
	if err != nil {
		fail(err)
	}
}

func fail(err error) {
	fmt.Fprintln(os.Stderr, err)
	os.Exit(1)
}

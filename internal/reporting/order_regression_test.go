package reporting

import (
	"encoding/json"
	"testing"
)

// Ties are what the committed golden fixture lacks: its totals are 2 and 1, so
// any ordering looks correct against it. These tests seed equal totals on
// purpose, because that is the only case where a nondeterministic or
// input-order-dependent ordering is observable.

func tieRows() []requestRow {
	var rows []requestRow
	for _, id := range []string{"delta", "alpha", "charlie", "bravo"} {
		rows = append(rows, requestRow{BrokerID: id, CurrentStatus: "CONFIRMED", Jurisdiction: "GDPR"})
	}
	return rows
}

// TestBrokerLeaderboardOrderIsStable pins the order across repeated calls. Map
// iteration is randomised per run, so the previous implementation returned a
// different order each time (#963).
func TestBrokerLeaderboardOrderIsStable(t *testing.T) {
	want := `[{"avg_response_time_days":null,"broker_id":"alpha","confirmed":1,"overdue":0,"pending":0,"rejected":0,"success_rate":100,"total":1},` +
		`{"avg_response_time_days":null,"broker_id":"bravo","confirmed":1,"overdue":0,"pending":0,"rejected":0,"success_rate":100,"total":1},` +
		`{"avg_response_time_days":null,"broker_id":"charlie","confirmed":1,"overdue":0,"pending":0,"rejected":0,"success_rate":100,"total":1},` +
		`{"avg_response_time_days":null,"broker_id":"delta","confirmed":1,"overdue":0,"pending":0,"rejected":0,"success_rate":100,"total":1}]`
	for run := 0; run < 50; run++ {
		got, err := json.Marshal(brokerLeaderboard(tieRows()))
		if err != nil {
			t.Fatal(err)
		}
		if string(got) != want {
			t.Fatalf("run %d: leaderboard order is not stable\ngot:  %s\nwant: %s", run, got, want)
		}
	}
}

// TestBrokerLeaderboardIgnoresInputOrder proves the result is a function of the
// data and not of the order the query happened to return rows in. `loadRequests`
// only orders by `created_at`, so ties there are not guaranteed to be stable.
func TestBrokerLeaderboardIgnoresInputOrder(t *testing.T) {
	rows := tieRows()
	reversed := make([]requestRow, 0, len(rows))
	for i := len(rows) - 1; i >= 0; i-- {
		reversed = append(reversed, rows[i])
	}
	forward, err := json.Marshal(brokerLeaderboard(rows))
	if err != nil {
		t.Fatal(err)
	}
	backward, err := json.Marshal(brokerLeaderboard(reversed))
	if err != nil {
		t.Fatal(err)
	}
	if string(forward) != string(backward) {
		t.Fatalf("leaderboard depends on input order\nforward:  %s\nreversed: %s", forward, backward)
	}
}

// TestJurisdictionBreakdownIgnoresInputOrder covers the same defect on the
// jurisdiction path, where equal totals previously followed first-seen order.
func TestJurisdictionBreakdownIgnoresInputOrder(t *testing.T) {
	rows := []requestRow{
		{BrokerID: "x", CurrentStatus: "CONFIRMED", Jurisdiction: "CCPA"},
		{BrokerID: "x", CurrentStatus: "CONFIRMED", Jurisdiction: "GDPR"},
	}
	reversed := []requestRow{rows[1], rows[0]}
	forward, err := json.Marshal(jurisdictionBreakdown(rows))
	if err != nil {
		t.Fatal(err)
	}
	backward, err := json.Marshal(jurisdictionBreakdown(reversed))
	if err != nil {
		t.Fatal(err)
	}
	if string(forward) != string(backward) {
		t.Fatalf("jurisdiction breakdown depends on input order\nforward:  %s\nreversed: %s", forward, backward)
	}
	if got, want := string(forward), `[{"confirmation_rate":100,"confirmed":1,"jurisdiction":"CCPA","overdue":0,"rejected":0,"total":1},`+
		`{"confirmation_rate":100,"confirmed":1,"jurisdiction":"GDPR","overdue":0,"rejected":0,"total":1}]`; got != want {
		t.Fatalf("jurisdiction order\ngot:  %s\nwant: %s", got, want)
	}
}

// TestBrokerDashboardKeepsTheLeaderboardOrder covers `GetDashboardData`, which
// reuses the leaderboard and therefore inherited the same nondeterminism.
func TestBrokerDashboardKeepsTheLeaderboardOrder(t *testing.T) {
	leaderboard, err := json.Marshal(brokerLeaderboard(tieRows()))
	if err != nil {
		t.Fatal(err)
	}
	dashboard, err := json.Marshal(brokerDashboard(tieRows()))
	if err != nil {
		t.Fatal(err)
	}
	var first []map[string]any
	var second []map[string]any
	if err := json.Unmarshal(leaderboard, &first); err != nil {
		t.Fatal(err)
	}
	if err := json.Unmarshal(dashboard, &second); err != nil {
		t.Fatal(err)
	}
	if len(first) != len(second) {
		t.Fatalf("length mismatch: %d vs %d", len(first), len(second))
	}
	for i := range first {
		if first[i]["broker_id"] != second[i]["broker_id"] {
			t.Fatalf("dashboard order diverges at %d: %v vs %v", i, first[i]["broker_id"], second[i]["broker_id"])
		}
	}
}

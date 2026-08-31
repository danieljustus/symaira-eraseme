package eventstore

import (
	"context"
	"path/filepath"
	"testing"
	"time"
)

func newTestRepository(t *testing.T) (*Repository, *Store) {
	t.Helper()
	dir := t.TempDir()
	dbPath := filepath.Join(dir, "test.db")
	store, err := Open(dbPath)
	if err != nil {
		t.Fatalf("Open failed: %v", err)
	}
	t.Cleanup(func() { _ = store.Close() })
	repo := NewRepository(store)
	if repo.Store() != store {
		t.Fatalf("Store() mismatch")
	}
	return repo, store
}

func ptrString(s string) *string {
	return &s
}

func ptrInt(i int) *int {
	return &i
}

func ptrEventType(e EventType) *EventType {
	return &e
}

func TestCampaignRepository(t *testing.T) {
	ctx := context.Background()
	repo, _ := newTestRepository(t)

	// test_create_and_list
	ok, err := repo.CreateCampaign(ctx, "repo-campaign", "initial", "test")
	if err != nil || !ok {
		t.Fatalf("CreateCampaign failed: ok=%v err=%v", ok, err)
	}

	camps, err := repo.ListCampaigns(ctx)
	if err != nil {
		t.Fatalf("ListCampaigns failed: %v", err)
	}
	found := false
	for _, c := range camps {
		if c["id"] == "repo-campaign" {
			found = true
			if c["kind"] != "initial" || c["notes"] != "test" {
				t.Fatalf("unexpected campaign data: %#v", c)
			}
		}
	}
	if !found {
		t.Fatal("campaign not found in list")
	}

	// test_create_duplicate_is_detected
	ok1, err := repo.CreateCampaign(ctx, "dup-repo", "", "")
	if err != nil || !ok1 {
		t.Fatalf("first CreateCampaign failed: ok=%v err=%v", ok1, err)
	}
	ok2, err := repo.CreateCampaign(ctx, "dup-repo", "", "")
	if err != nil {
		t.Fatalf("second CreateCampaign errored: %v", err)
	}
	if ok2 {
		t.Fatal("duplicate CreateCampaign returned true, want false")
	}
}

func TestRequestRepository(t *testing.T) {
	ctx := context.Background()
	repo, _ := newTestRepository(t)

	// test_create_and_retrieve
	_, err := repo.CreateCampaign(ctx, "camp-repo", "initial", "")
	if err != nil {
		t.Fatalf("CreateCampaign failed: %v", err)
	}

	rid, err := repo.CreateRemovalRequest(ctx, "acxiom", "email", "camp-repo", "GDPR-DE", "tmpl-1", "hash-1")
	if err != nil || rid <= 0 {
		t.Fatalf("CreateRemovalRequest failed: rid=%d err=%v", rid, err)
	}

	req, err := repo.GetRemovalRequest(ctx, rid)
	if err != nil || req == nil {
		t.Fatalf("GetRemovalRequest failed: req=%#v err=%v", req, err)
	}
	if req["broker_id"] != "acxiom" || req["campaign_id"] != "camp-repo" || req["jurisdiction"] != "GDPR-DE" {
		t.Fatalf("mismatched removal request: %#v", req)
	}

	// Non-existent request returns error or nil
	nonExistent, err := repo.GetRemovalRequest(ctx, 999999)
	if err == nil && nonExistent != nil {
		t.Fatalf("expected nil/error for non-existent request, got %#v", nonExistent)
	}
}

func TestListRemovalRequestsFiltersAndPagination(t *testing.T) {
	ctx := context.Background()
	repo, _ := newTestRepository(t)

	_, _ = repo.CreateCampaign(ctx, "camp-a", "initial", "")
	_, _ = repo.CreateCampaign(ctx, "camp-b", "initial", "")

	r1, _ := repo.CreateRemovalRequest(ctx, "b1", "email", "camp-a", "GDPR", "", "")
	r2, _ := repo.CreateRemovalRequest(ctx, "b2", "email", "camp-a", "CCPA", "", "")
	r3, _ := repo.CreateRemovalRequest(ctx, "b3", "email", "camp-b", "GDPR", "", "")

	// List by campaign
	campA, err := repo.ListRemovalRequests(ctx, ListRemovalRequestsOpts{CampaignID: ptrString("camp-a")})
	if err != nil {
		t.Fatalf("ListRemovalRequests failed: %v", err)
	}
	if len(campA) != 2 {
		t.Fatalf("len(campA) = %d, want 2", len(campA))
	}

	campB, err := repo.ListRemovalRequests(ctx, ListRemovalRequestsOpts{CampaignID: ptrString("camp-b")})
	if err != nil || len(campB) != 1 {
		t.Fatalf("len(campB) = %d, want 1 (err=%v)", len(campB), err)
	}

	// List by broker
	byBroker, err := repo.ListRemovalRequests(ctx, ListRemovalRequestsOpts{BrokerID: ptrString("b1")})
	if err != nil || len(byBroker) != 1 || byBroker[0]["broker_id"] != "b1" {
		t.Fatalf("byBroker mismatch: %#v (err=%v)", byBroker, err)
	}

	// Count requests
	countA, err := repo.CountRemovalRequests(ctx, ptrString("camp-a"), nil)
	if err != nil || countA != 2 {
		t.Fatalf("countA = %d (err=%v), want 2", countA, err)
	}
	countAll, err := repo.CountRemovalRequests(ctx, nil, nil)
	if err != nil || countAll != 3 {
		t.Fatalf("countAll = %d (err=%v), want 3", countAll, err)
	}

	_ = r1
	_ = r2
	_ = r3
}

func TestPaginationLimitsAndOffsets(t *testing.T) {
	ctx := context.Background()
	repo, _ := newTestRepository(t)

	_, _ = repo.CreateCampaign(ctx, "pag-camp", "initial", "")
	for i := 0; i < 10; i++ {
		brokerID := "b" + string(rune('0'+i))
		_, _ = repo.CreateRemovalRequest(ctx, brokerID, "email", "pag-camp", "GDPR", "", "")
	}

	// test_pagination_limit
	res5, err := repo.ListRemovalRequests(ctx, ListRemovalRequestsOpts{CampaignID: ptrString("pag-camp"), Limit: ptrInt(5)})
	if err != nil || len(res5) != 5 {
		t.Fatalf("len(res5) = %d, want 5 (err=%v)", len(res5), err)
	}
	if res5[0]["broker_id"] != "b0" || res5[4]["broker_id"] != "b4" {
		t.Fatalf("ordering mismatch in limit: first=%v fifth=%v", res5[0]["broker_id"], res5[4]["broker_id"])
	}

	// test_pagination_offset
	resOffset, err := repo.ListRemovalRequests(ctx, ListRemovalRequestsOpts{CampaignID: ptrString("pag-camp"), Limit: ptrInt(3), Offset: ptrInt(3)})
	if err != nil || len(resOffset) != 3 {
		t.Fatalf("len(resOffset) = %d, want 3 (err=%v)", len(resOffset), err)
	}
	if resOffset[0]["broker_id"] != "b3" || resOffset[2]["broker_id"] != "b5" {
		t.Fatalf("offset mismatch: first=%v third=%v", resOffset[0]["broker_id"], resOffset[2]["broker_id"])
	}

	// test_pagination_offset_without_limit_uses_negative_one
	resNoLimit, err := repo.ListRemovalRequests(ctx, ListRemovalRequestsOpts{CampaignID: ptrString("pag-camp"), Offset: ptrInt(8)})
	if err != nil || len(resNoLimit) != 2 {
		t.Fatalf("len(resNoLimit) = %d, want 2 (err=%v)", len(resNoLimit), err)
	}
	if resNoLimit[0]["broker_id"] != "b8" {
		t.Fatalf("no limit offset mismatch: first=%v", resNoLimit[0]["broker_id"])
	}

	// test_pagination_limit_zero
	resZero, err := repo.ListRemovalRequests(ctx, ListRemovalRequestsOpts{CampaignID: ptrString("pag-camp"), Limit: ptrInt(0)})
	if err != nil || len(resZero) != 0 {
		t.Fatalf("len(resZero) = %d, want 0 (err=%v)", len(resZero), err)
	}

	// test_pagination_offset_beyond_results
	resBeyond, err := repo.ListRemovalRequests(ctx, ListRemovalRequestsOpts{CampaignID: ptrString("pag-camp"), Limit: ptrInt(5), Offset: ptrInt(100)})
	if err != nil || len(resBeyond) != 0 {
		t.Fatalf("len(resBeyond) = %d, want 0 (err=%v)", len(resBeyond), err)
	}
}

func TestEventRepositoryOperations(t *testing.T) {
	ctx := context.Background()
	repo, _ := newTestRepository(t)

	_, _ = repo.CreateCampaign(ctx, "c", "initial", "")
	rid1, _ := repo.CreateRemovalRequest(ctx, "b1", "email", "c", "GDPR", "", "")
	rid2, _ := repo.CreateRemovalRequest(ctx, "b2", "email", "c", "GDPR", "", "")

	now := time.Now().UTC()
	eid1, err := repo.AppendEvent(ctx, rid1, EvtPlanned, map[string]any{"plan": "test"}, SrcSystem, now)
	if err != nil || eid1 <= 0 {
		t.Fatalf("AppendEvent failed: eid=%d err=%v", eid1, err)
	}

	events, err := repo.GetEvents(ctx, rid1, 0)
	if err != nil || len(events) != 1 {
		t.Fatalf("GetEvents failed: len=%d (err=%v)", len(events), err)
	}
	if events[0].EventType != EvtPlanned {
		t.Fatalf("events[0].EventType = %v, want PLANNED", events[0].EventType)
	}

	// get_events_after_id
	eid2, err := repo.AppendEvent(ctx, rid1, EvtSent, map[string]any{"msg": "sent"}, SrcSystem, now.Add(time.Second))
	if err != nil {
		t.Fatalf("AppendEvent 2 failed: %v", err)
	}
	later, err := repo.GetEvents(ctx, rid1, eid1)
	if err != nil || len(later) != 1 || later[0].ID != eid2 {
		t.Fatalf("GetEvents after ID mismatch: %#v (err=%v)", later, err)
	}

	// get_events_for_requests
	eid3, _ := repo.AppendEvent(ctx, rid2, EvtSent, map[string]any{}, SrcSystem, now)
	_ = eid3

	resMap, err := repo.GetEventsForRequests(ctx, []int64{rid1, rid2}, nil)
	if err != nil {
		t.Fatalf("GetEventsForRequests failed: %v", err)
	}
	if len(resMap[rid1]) != 2 || len(resMap[rid2]) != 1 {
		t.Fatalf("GetEventsForRequests mismatch: rid1=%d rid2=%d", len(resMap[rid1]), len(resMap[rid2]))
	}

	// Event type filter
	sentOnly, err := repo.GetEventsForRequests(ctx, []int64{rid1, rid2}, ptrEventType(EvtSent))
	if err != nil {
		t.Fatalf("GetEventsForRequests with filter failed: %v", err)
	}
	if len(sentOnly[rid1]) != 1 || len(sentOnly[rid2]) != 1 {
		t.Fatalf("sentOnly mismatch: rid1=%d rid2=%d", len(sentOnly[rid1]), len(sentOnly[rid2]))
	}

	// Event type no match
	noMatch, err := repo.GetEventsForRequests(ctx, []int64{rid2}, ptrEventType(EvtConfirmed))
	if err != nil {
		t.Fatalf("GetEventsForRequests noMatch failed: %v", err)
	}
	if len(noMatch[rid2]) != 0 {
		t.Fatalf("expected 0 events for noMatch, got %d", len(noMatch[rid2]))
	}

	// Empty request list
	emptyRes, err := repo.GetEventsForRequests(ctx, []int64{}, nil)
	if err != nil || len(emptyRes) != 0 {
		t.Fatalf("empty request list failed: %#v (err=%v)", emptyRes, err)
	}
}

func TestActiveMatchableRequests(t *testing.T) {
	ctx := context.Background()
	repo, store := newTestRepository(t)

	_, _ = repo.CreateCampaign(ctx, "c1", "initial", "")
	_, _ = repo.CreateCampaign(ctx, "c2", "initial", "")

	r1, _ := repo.CreateRemovalRequest(ctx, "b1", "email", "c1", "GDPR", "", "")
	r2, _ := repo.CreateRemovalRequest(ctx, "b2", "email", "c1", "GDPR", "", "")
	r3, _ := repo.CreateRemovalRequest(ctx, "b3", "email", "c1", "GDPR", "", "")
	r4, _ := repo.CreateRemovalRequest(ctx, "b4", "email", "c2", "GDPR", "", "")

	now := time.Now().UTC()
	// r1: PLANNED + SENT (active)
	_, _, _ = store.AppendAndProject(ctx, r1, EvtPlanned, map[string]any{}, SrcSystem, now)
	_, _, _ = store.AppendAndProject(ctx, r1, EvtSent, map[string]any{}, SrcSystem, now)

	// r2: SENT + CONFIRMED (terminal)
	_, _, _ = store.AppendAndProject(ctx, r2, EvtSent, map[string]any{}, SrcSystem, now)
	_, _, _ = store.AppendAndProject(ctx, r2, EvtConfirmed, map[string]any{}, SrcSystem, now)

	// r3: SENT + REJECTED_FINAL (terminal)
	_, _, _ = store.AppendAndProject(ctx, r3, EvtSent, map[string]any{}, SrcSystem, now)
	_, _, _ = store.AppendAndProject(ctx, r3, EvtRejectedFinal, map[string]any{}, SrcSystem, now)

	// r4: in campaign c2 (active)
	_, _, _ = store.AppendAndProject(ctx, r4, EvtPlanned, map[string]any{}, SrcSystem, now)

	activeC1, err := repo.GetActiveMatchableRequests(ctx, ptrString("c1"))
	if err != nil {
		t.Fatalf("GetActiveMatchableRequests failed: %v", err)
	}
	if len(activeC1) != 1 || activeC1[0]["id"].(int64) != r1 {
		t.Fatalf("activeC1 mismatch: got %#v, want [r1=%d]", activeC1, r1)
	}

	activeAll, err := repo.GetActiveMatchableRequests(ctx, nil)
	if err != nil {
		t.Fatalf("GetActiveMatchableRequests all failed: %v", err)
	}
	if len(activeAll) != 2 {
		t.Fatalf("activeAll length = %d, want 2 (r1 and r4)", len(activeAll))
	}
}

func TestTickCandidates(t *testing.T) {
	ctx := context.Background()
	repo, store := newTestRepository(t)

	_, _ = repo.CreateCampaign(ctx, "c", "initial", "")
	rid1, _ := repo.CreateRemovalRequest(ctx, "b1", "email", "c", "GDPR", "", "")
	rid2, _ := repo.CreateRemovalRequest(ctx, "b2", "email", "c", "GDPR", "", "")

	now := time.Now().UTC()
	_, _, _ = store.AppendAndProject(ctx, rid1, EvtPlanned, map[string]any{}, SrcSystem, now)
	_, _, _ = store.AppendAndProject(ctx, rid2, EvtSent, map[string]any{}, SrcSystem, now)

	nowISO := now.Format(time.RFC3339)
	candidates, err := repo.FetchTickCandidates(ctx, nowISO, 10)
	if err != nil {
		t.Fatalf("FetchTickCandidates failed: %v", err)
	}
	if len(candidates) != 2 {
		t.Fatalf("candidates length = %d, want 2", len(candidates))
	}
}

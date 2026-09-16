package eventstore

import (
	"context"
	"errors"
	"reflect"
	"testing"
	"time"
)

func TestProjectionStatusTransitionsAndCounters(t *testing.T) {
	when := time.Date(2026, 8, 31, 12, 0, 0, 0, time.UTC)
	state := newBlankState(7)
	cases := []struct {
		event EventType
		want  string
	}{
		{EvtPlanned, "PLANNED"},
		{EvtSent, "AWAITING_ACK"},
		{EvtAck, "ACK"},
		{EvtVerificationRequested, "AWAITING_USER_ACTION"},
		{EvtVerificationProvided, "AWAITING_RESPONSE"},
		{EvtHumanActionRequired, "AWAITING_USER_ACTION"},
		{EvtConfirmed, "CONFIRMED"},
		{EvtRejectedFinal, "REJECTED_FINAL"},
		{EvtConfirmationLinkClicked, "CONFIRMED"},
		{EvtRebuttalSent, "AWAITING_RESPONSE"},
		{EvtReminderSent, "AWAITING_ACK"},
		{EvtDeadlineReached, "OVERDUE"},
		{EvtDPAComplaintDrafted, "ESCALATED"},
		{EvtDPAComplaintFiled, "DPA_FILED"},
		{EvtRescanTriggered, "RE_SCAN_DUE"},
		{EvtSendFailed, "SEND_FAILED"},
		{EvtBounce, "BOUNCE"},
		{EvtAutoresponder, "AWAITING_ACK"},
	}
	for id, tc := range cases {
		t.Run(string(tc.event), func(t *testing.T) {
			local := state
			payload := map[string]any{}
			if tc.event == EvtSent {
				payload["expected_response_days"] = float64(2)
			}
			if tc.event == EvtReminderSent {
				payload["count"] = float64(3)
			}
			if err := applyEvent(&local, Event{ID: int64(id + 1), EventType: tc.event, OccurredAt: when, Payload: payload}); err != nil {
				t.Fatal(err)
			}
			if local.CurrentStatus != tc.want {
				t.Fatalf("status = %q, want %q", local.CurrentStatus, tc.want)
			}
		})
	}
	if state.RemindersSent != 0 {
		t.Fatal("test mutated source state")
	}
	if err := applyEvent(&state, Event{ID: 100, EventType: EvtReminderSent, OccurredAt: when, Payload: nil}); err != nil || state.RemindersSent != 1 {
		t.Fatalf("default reminder count = %d, err=%v", state.RemindersSent, err)
	}
	if err := applyEvent(&state, Event{ID: 101, EventType: EvtDeadlineReached, OccurredAt: when, Payload: nil}); err != nil || state.EscalationLevel != 1 {
		t.Fatalf("deadline escalation = %d, err=%v", state.EscalationLevel, err)
	}
	if err := applyEvent(&state, Event{ID: 102, EventType: EvtDPAComplaintDrafted, OccurredAt: when, Payload: nil}); err != nil || state.EscalationLevel != 2 {
		t.Fatalf("DPA escalation = %d, err=%v", state.EscalationLevel, err)
	}
}

func TestProjectionHelpersRejectInvalidEventsAndAcceptNumericTypes(t *testing.T) {
	state := newBlankState(1)
	for _, event := range []Event{
		{EventType: "", OccurredAt: time.Now()},
		{EventType: EvtSent},
	} {
		if err := applyEvent(&state, event); err == nil {
			t.Fatal("invalid event accepted")
		}
	}
	for _, value := range []any{int(3), int32(4), int64(5), float64(6), float32(7)} {
		got, ok := toInt(value)
		if !ok || got < 3 {
			t.Fatalf("toInt(%T) = %d, %v", value, got, ok)
		}
	}
	if _, ok := toInt("8"); ok {
		t.Fatal("string was accepted as an integer")
	}
	if pointer := isoPtr(time.Time{}); pointer == nil || *pointer != "" {
		t.Fatalf("zero iso pointer = %#v", pointer)
	}
}

func TestStoreProjectionRebuildAllAndRequestIDs(t *testing.T) {
	store, err := Open(t.TempDir() + "/state.db")
	if err != nil {
		t.Fatal(err)
	}
	defer store.Close()
	ctx := context.Background()
	id1, err := store.CreateRemovalRequest(ctx, "one", "", "c", "DE", "", "")
	if err != nil {
		t.Fatal(err)
	}
	id2, err := store.CreateRemovalRequest(ctx, "two", "", "c", "DE", "", "")
	if err != nil {
		t.Fatal(err)
	}
	if _, _, err := store.AppendAndProject(ctx, id1, EvtSent, nil, SrcSystem, time.Now().UTC()); err != nil {
		t.Fatal(err)
	}
	if _, err := store.Append(ctx, id2, EvtAck, nil, SrcSystem, time.Now().UTC()); err != nil {
		t.Fatal(err)
	}
	ids, err := store.RequestIDs(ctx)
	if err != nil || len(ids) != 2 || ids[0] != id1 || ids[1] != id2 {
		t.Fatalf("request IDs = %#v, err=%v", ids, err)
	}
	if count, err := store.RebuildAllStates(ctx, 1); err != nil || count != 1 {
		t.Fatalf("rebuild all = %d, err=%v", count, err)
	}
	projections, err := store.AllProjections(ctx)
	if err != nil || len(projections) != 2 {
		t.Fatalf("projections = %#v, err=%v", projections, err)
	}
	if _, err := store.ApplyPendingProjects(ctx); err != nil {
		t.Fatal(err)
	}
	if _, err := store.Append(ctx, id1, EventType("INVALID"), nil, SrcSystem, time.Now().UTC()); !errors.Is(err, ErrUnknownEventType) {
		t.Fatalf("invalid event error = %v", err)
	}
	if _, err := store.Append(ctx, id1, EvtNoteAdded, nil, Source("invalid"), time.Now().UTC()); !errors.Is(err, ErrUnknownSource) {
		t.Fatalf("invalid source error = %v", err)
	}
}

func TestParseableUnknownReplayAdvancesBookkeepingWithoutStatusSideEffects(t *testing.T) {
	store, err := Open(t.TempDir() + "/unknown-replay.db")
	if err != nil {
		t.Fatal(err)
	}
	defer store.Close()

	ctx := context.Background()
	requestID, err := store.CreateRemovalRequest(ctx, "unknown-replay-broker", "email", "campaign", "DE", "", "")
	if err != nil {
		t.Fatal(err)
	}
	knownAt := time.Date(2026, 8, 1, 8, 0, 0, 0, time.UTC)
	unknownAt := knownAt.Add(24 * time.Hour)
	knownID, err := store.Append(ctx, requestID, EvtSent, map[string]any{"expected_response_days": 5}, SrcSystem, knownAt)
	if err != nil {
		t.Fatal(err)
	}

	result, err := store.DB().ExecContext(ctx, `
		INSERT INTO request_events
		 (request_id, occurred_at, recorded_at, event_type, payload_json, source)
		 VALUES (?, ?, ?, ?, ?, ?)`,
		requestID,
		unknownAt.Format(time.RFC3339),
		unknownAt.Format(time.RFC3339),
		"FUTURE_EVENT",
		"{}",
		string(SrcSystem),
	)
	if err != nil {
		t.Fatal(err)
	}
	unknownID, err := result.LastInsertId()
	if err != nil {
		t.Fatal(err)
	}

	events, err := store.GetEvents(ctx, requestID, 0)
	if err != nil {
		t.Fatal(err)
	}
	if len(events) != 2 || events[1].EventType != EventType("FUTURE_EVENT") {
		t.Fatalf("replay events = %#v, want preserved unknown event type", events)
	}

	state, err := store.RebuildState(ctx, requestID)
	if err != nil {
		t.Fatal(err)
	}
	want := StateJSON{
		CurrentStatus:   "AWAITING_ACK",
		DeadlineAt:      isoPtr(knownAt.Add(5 * 24 * time.Hour)),
		LastEventAt:     isoPtr(unknownAt),
		LastEventID:     unknownID,
		RemindersSent:   0,
		RequestID:       requestID,
		SentAt:          isoPtr(knownAt),
		EscalationLevel: 0,
	}
	if !reflect.DeepEqual(state, want) {
		t.Fatalf("unknown replay state = %#v, want %#v (known event id=%d)", state, want, knownID)
	}

	persisted, err := store.UpsertState(ctx, requestID)
	if err != nil {
		t.Fatal(err)
	}
	if !reflect.DeepEqual(persisted, want) {
		t.Fatalf("persisted unknown replay state = %#v, want %#v", persisted, want)
	}
}

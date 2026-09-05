package replies

import (
	"context"
	"encoding/json"
	"strings"
	"testing"

	"github.com/danieljustus/symaira-eraseme/internal/confirmation"
	"github.com/danieljustus/symaira-eraseme/internal/eventstore"
)

func TestSuccessfulAutoConfirmSummarizesSensitiveResult(t *testing.T) {
	store, requestID := newRepliesTestStore(t)
	ctx := context.Background()
	if _, err := NewRepository(store).InsertReply(ctx, &requestID, "secure-confirm", "thread", "broker@acxiom.com", "Confirm", "https://acxiom.com/confirm?token=secret", ""); err != nil {
		t.Fatal(err)
	}
	result, err := NewService(store).AutoConfirm(ctx, AutoConfirmRequest{RequestID: requestID, Click: func(_ context.Context, link string, _ confirmation.ClickOptions) (confirmation.Result, error) {
		return confirmation.Result{Success: true, Step: "clicked", ClickedURL: link, ScreenshotBefore: "before-secret", ScreenshotAfter: "after-secret"}, nil
	}})
	if err != nil {
		t.Fatal(err)
	}
	if result.ClickedURL != "" || result.ScreenshotBefore != "" || result.ScreenshotAfter != "" || result.ClickedURLSHA256 == "" || result.ScreenshotBeforeSHA256 == "" || result.ScreenshotAfterSHA256 == "" {
		t.Fatalf("unsanitized result = %#v", result)
	}
	events, err := store.GetEvents(ctx, requestID, 0)
	if err != nil {
		t.Fatal(err)
	}
	var clicked *eventstore.Event
	for i := range events {
		if events[i].EventType == eventstore.EvtConfirmationLinkClicked {
			clicked = &events[i]
		}
	}
	if clicked == nil {
		t.Fatalf("confirmation event missing: %#v", events)
	}
	encoded, err := json.Marshal(clicked.Payload)
	if err != nil {
		t.Fatal(err)
	}
	text := string(encoded)
	for _, forbidden := range []string{"token=secret", "before-secret", "after-secret", `"url"`, `"screenshot_before"`, `"screenshot_after"`} {
		if strings.Contains(text, forbidden) {
			t.Fatalf("sensitive confirmation data leaked: %s", text)
		}
	}
	if !strings.Contains(text, "url_sha256") || !strings.Contains(text, "screenshot_before_sha256") {
		t.Fatalf("confirmation summary missing: %s", text)
	}
}

func TestFailedAutoConfirmWithoutGoErrorSummarizesSensitiveResult(t *testing.T) {
	store, requestID := newRepliesTestStore(t)
	ctx := context.Background()
	if _, err := NewRepository(store).InsertReply(ctx, &requestID, "failed-confirm", "thread", "broker@acxiom.com", "Confirm", "https://acxiom.com/confirm?token=secret", ""); err != nil {
		t.Fatal(err)
	}
	result, err := NewService(store).AutoConfirm(ctx, AutoConfirmRequest{RequestID: requestID, Click: func(_ context.Context, link string, _ confirmation.ClickOptions) (confirmation.Result, error) {
		return confirmation.Result{Success: false, Step: "click_failed", Error: "failed for jane@example.test at " + link, ClickedURL: link, ScreenshotBefore: "before-secret"}, nil
	}})
	if err != nil {
		t.Fatal(err)
	}
	if result.ClickedURL != "" || result.ScreenshotBefore != "" || result.ClickedURLSHA256 == "" || result.ScreenshotBeforeSHA256 == "" || strings.Contains(result.Error, "jane@example.test") {
		t.Fatalf("unsanitized failed result = %#v", result)
	}
	events, err := store.GetEvents(ctx, requestID, 0)
	if err != nil {
		t.Fatal(err)
	}
	encoded, err := json.Marshal(events[len(events)-1].Payload)
	if err != nil {
		t.Fatal(err)
	}
	text := string(encoded)
	for _, forbidden := range []string{"token=secret", "before-secret", "jane@example.test", `"url"`, `"screenshot_before"`} {
		if strings.Contains(text, forbidden) {
			t.Fatalf("failed confirmation leaked sensitive data: %s", text)
		}
	}
	if !strings.Contains(text, "url_sha256") {
		t.Fatalf("failed confirmation summary missing: %s", text)
	}
}

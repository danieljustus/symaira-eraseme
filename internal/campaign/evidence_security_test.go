package campaign

import (
	"context"
	"encoding/json"
	"path/filepath"
	"strings"
	"testing"
	"time"

	"github.com/danieljustus/symaira-eraseme/internal/eventstore"
)

const sentinelUnknownEvidence = "unknown-executor-field-with-sensitive-data"

func TestExecuteWebFormSummarizesSensitiveEvidence(t *testing.T) {
	store, err := eventstore.Open(filepath.Join(t.TempDir(), "db.sqlite"))
	if err != nil {
		t.Fatal(err)
	}
	defer store.Close()
	t.Setenv("SYMERASEME_DATA_DIR", t.TempDir())
	ctx := context.Background()
	requestID, err := store.CreateRemovalRequest(ctx, "broker", "web_form", "campaign", "DE", "", "")
	if err != nil {
		t.Fatal(err)
	}
	if _, _, err := store.AppendAndProject(ctx, requestID, eventstore.EvtPlanned, map[string]any{"form_url": "https://broker.test/form"}, eventstore.SrcSystem, time.Now().UTC()); err != nil {
		t.Fatal(err)
	}
	const email = "jane@example.test"
	const page = "page text containing jane@example.test"
	const screenshot = "screenshot containing jane@example.test"
	result, err := ExecuteRequest(ctx, store, requestID, ExecuteOpts{WebForm: func(context.Context, string, bool) map[string]any {
		return map[string]any{
			"success": false, "code": string(CodeFieldNotFound), "reason": "unknown_field",
			"url": "https://broker.test/form?email=" + email, "error": "failed for " + email,
			"page_text": page, "html_snapshot": page, "form_fields": map[string]string{"email": email},
			"evidence":    map[string]any{"page_text": page, "post_submit_screenshot": []byte(screenshot)},
			"future_blob": sentinelUnknownEvidence,
		}
	}})
	if err != nil {
		t.Fatal(err)
	}
	events, err := store.GetEvents(ctx, requestID, 0)
	if err != nil {
		t.Fatal(err)
	}
	encoded, err := json.Marshal(map[string]any{"result": result, "events": events})
	if err != nil {
		t.Fatal(err)
	}
	text := string(encoded)
	for _, forbidden := range []string{email, page, screenshot, sentinelUnknownEvidence} {
		if strings.Contains(text, forbidden) {
			t.Fatalf("sensitive evidence leaked: %s", text)
		}
	}
	if !strings.Contains(text, "page_text_sha256") || !strings.Contains(text, "post_submit_screenshot_sha256") {
		t.Fatalf("evidence summary missing: %s", text)
	}
}

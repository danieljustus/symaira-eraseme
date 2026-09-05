package main

import (
	"context"
	"encoding/json"
	"os"
	"path/filepath"
	"strings"
	"testing"
	"time"

	"github.com/danieljustus/symaira-eraseme/internal/email"
	"github.com/danieljustus/symaira-eraseme/internal/eventstore"
	"github.com/danieljustus/symaira-eraseme/internal/mcp"
)

func TestCLIReadOnlyAndDryRunSurfaces(t *testing.T) {
	t.Setenv("SYMERASEME_DATA_DIR", t.TempDir())
	cases := [][]string{
		{"--output", "json", "status"},
		{"--output", "json", "plan", "show"},
		{"--output", "json", "plan", "status"},
		{"--output", "json", "tick", "--dry-run"},
		{"--output", "json", "calendar"},
		{"--output", "json", "dashboard"},
		{"--output", "json", "requests", "list"},
		{"--output", "json", "events", "show", "999"},
		{"--output", "json", "manual-tasks", "list"},
		{"--output", "json", "manual-tasks", "show", "999"},
		{"--output", "json", "manual-tasks", "complete", "999"},
		{"--output", "json", "manual-tasks", "cleanup", "--dry-run"},
		{"--output", "json", "generate-scheduler", "--platform", "cron", "--dry-run"},
		{"--output", "json", "schedule", "install", "--platform", "cron", "--dry-run"},
		{"--output", "json", "schedule", "status", "--platform", "cron"},
	}
	for _, args := range cases {
		name := strings.Join(args, " ")
		t.Run(name, func(t *testing.T) {
			out, err := execute(t, args...)
			if err != nil {
				t.Fatalf("command failed: %v\n%s", err, out)
			}
			if !json.Valid([]byte(out)) {
				t.Fatalf("command did not return valid JSON: %s", out)
			}
		})
	}
}

func TestCLIGenerateReportText(t *testing.T) {
	t.Setenv("SYMERASEME_DATA_DIR", t.TempDir())
	out, err := execute(t, "generate-report", "--all-campaigns")
	if err != nil || !strings.Contains(out, "success") {
		t.Fatalf("generate-report failed: %v\\n%s", err, out)
	}
}

func TestCLIProfileTemplateAndAdapterPaths(t *testing.T) {
	dataDir := t.TempDir()
	t.Setenv("SYMERASEME_DATA_DIR", dataDir)
	t.Setenv("SYMERASEME_IDENTITY_MASTER_KEY", strings.Repeat("a", 64))

	profileOut, err := execute(t, "--output", "json", "init-profile", "--full-name", "Test Person", "--email", "test@example.test")
	if err != nil || !strings.Contains(profileOut, "profile_path") {
		t.Fatalf("init-profile failed: %v\\n%s", err, profileOut)
	}
	showOut, err := execute(t, "--output", "json", "show-profile")
	if err != nil || !strings.Contains(showOut, "Test Person") || strings.Contains(showOut, "master_key") {
		t.Fatalf("show-profile failed or leaked data: %v\\n%s", err, showOut)
	}
	templateOut, err := execute(t, "render-template", "gdpr-art17.en.md.j2", "--broker-name", "Example")
	if err != nil || templateOut == "" {
		t.Fatalf("render-template failed: %v\\n%s", err, templateOut)
	}
	grantOut, err := execute(t, "--output", "json", "grant", "execute", "--dry-run")
	if err != nil || !strings.Contains(grantOut, "dry_run") {
		t.Fatalf("grant dry-run failed: %v\\n%s", err, grantOut)
	}
	webOut, err := execute(t, "--output", "json", "run-web-form", "missing", "--dry-run")
	if err == nil || !strings.Contains(webOut, `"success":false`) {
		t.Fatalf("missing web-form dry-run did not fail honestly: %v\\n%s", err, webOut)
	}
	autoOut, err := execute(t, "--output", "json", "auto-confirm", "999", "--dry-run")
	if err == nil || !strings.Contains(autoOut, "no inbox reply") {
		t.Fatalf("missing-reply auto-confirm did not fail honestly: %v\\n%s", err, autoOut)
	}
	reportPath := filepath.Join(t.TempDir(), "dashboard.html")
	reportOut, err := execute(t, "generate-dashboard", "--output", reportPath)
	if err != nil || !strings.Contains(reportOut, "success") {
		t.Fatalf("generate-dashboard failed: %v\\n%s", err, reportOut)
	}
	if _, err := os.Stat(reportPath); err != nil {
		t.Fatalf("dashboard output missing: %v", err)
	}
	if _, err := execute(t, "poll-inbox", "--host", "imap.example.test"); err == nil {
		t.Fatal("poll-inbox unexpectedly succeeded without an IMAP dialer")
	}
}

func TestCLIEmbeddedRegistrySurfaces(t *testing.T) {
	for _, args := range [][]string{
		{"--output", "json", "registry", "list"},
		{"--output", "json", "registry", "validate"},
		{"--output", "json", "brokers", "list", "--jurisdiction", "DE"},
	} {
		out, err := execute(t, args...)
		if err != nil {
			t.Fatalf("%v failed: %v\n%s", args, err, out)
		}
		if !strings.Contains(out, "schema_version") {
			t.Fatalf("%v returned unexpected output: %s", args, out)
		}
	}
}

func TestCLICompletionSupportsAllShellsAndRejectsUnknown(t *testing.T) {
	for _, shell := range []string{"bash", "zsh", "fish", "powershell"} {
		out, err := execute(t, "completion", shell)
		if err != nil || !strings.Contains(out, "symeraseme") {
			t.Fatalf("completion %s failed: %v\n%s", shell, err, out)
		}
	}
	if _, err := execute(t, "completion", "unknown"); err == nil {
		t.Fatal("unknown completion shell accepted")
	}
}

func TestCLIArgumentAndOutputValidation(t *testing.T) {
	if value, err := intArgument(nil, 4, "request ID"); err != nil || value != 4 {
		t.Fatalf("fallback int argument = %d, err=%v", value, err)
	}
	if _, err := intArgument([]string{"bad"}, 4, "request ID"); err == nil {
		t.Fatal("invalid positional integer accepted")
	}
	if _, err := execute(t, "--output", "yaml", "status"); err == nil {
		t.Fatal("invalid output format accepted")
	}
	if _, err := execute(t, "review"); err == nil {
		t.Fatal("review without a file path accepted")
	}
}

type fakeCLIPollDialer struct {
	session email.IMAPSession
	lastCfg email.IMAPConfig
}

func (d *fakeCLIPollDialer) Dial(_ context.Context, cfg email.IMAPConfig) (email.IMAPSession, error) {
	d.lastCfg = cfg
	return d.session, nil
}

type fakeCLIPollSession struct {
	uidValidity uint32
	uids        []uint32
	messages    []email.FetchedMessage
	selected    []string
}

func (s *fakeCLIPollSession) Select(_ context.Context, folder string) (uint32, error) {
	s.selected = append(s.selected, folder)
	return s.uidValidity, nil
}

func (s *fakeCLIPollSession) SearchUID(_ context.Context, _ string, _ ...time.Time) ([]uint32, error) {
	return append([]uint32(nil), s.uids...), nil
}

func (s *fakeCLIPollSession) Fetch(_ context.Context, _ []uint32) ([]email.FetchedMessage, error) {
	return append([]email.FetchedMessage(nil), s.messages...), nil
}

func (s *fakeCLIPollSession) Close() error {
	return nil
}

func TestCLIPollInboxWithFakeDialer(t *testing.T) {
	dataDir := t.TempDir()
	t.Setenv("SYMERASEME_DATA_DIR", dataDir)
	dbPath := filepath.Join(dataDir, "symeraseme.db")
	store, err := eventstore.Open(dbPath)
	if err != nil {
		t.Fatalf("open store: %v", err)
	}
	reqID, err := store.CreateRemovalRequest(context.Background(), "test-broker", "email", "campaign-1", "US", "tmpl", "hash")
	if err != nil {
		t.Fatalf("create request: %v", err)
	}
	_, _, err = store.AppendAndProject(context.Background(), reqID, eventstore.EvtSent, map[string]any{
		"message_id": "<sent-abc@symeraseme>", "broker_id": "test-broker",
	}, eventstore.SrcSystem, time.Now().UTC())
	if err != nil {
		t.Fatalf("append sent: %v", err)
	}
	if err := store.Close(); err != nil {
		t.Fatalf("close store: %v", err)
	}

	session := &fakeCLIPollSession{
		uidValidity: 100,
		uids:        []uint32{10},
		messages: []email.FetchedMessage{{
			UID:          10,
			Header:       []byte("Subject: Re: Removal\r\nFrom: broker@example.com\r\nTo: user@example.com\r\nIn-Reply-To: <sent-abc@symeraseme>\r\nMessage-ID: <reply-xyz@broker>\r\n\r\n"),
			Body:         []byte("Done."),
			InternalDate: time.Now().UTC(),
		}},
	}
	dialer := &fakeCLIPollDialer{session: session}
	handler := mcp.ContractHandlerWithOptions(mcp.ContractHandlerOptions{IMAPDialer: dialer})
	result, err := handler(context.Background(), "poll_inbox", map[string]any{
		"host": "imap.fake.test", "username": "testuser", "ssl": true,
	})
	if err != nil {
		t.Fatalf("poll-inbox handler failed: %v", err)
	}
	res, ok := result.(map[string]any)
	if !ok || res["total_fetched"] != 1 || res["total_matched"] != 1 {
		t.Fatalf("unexpected poll result: %#v", result)
	}
	if dialer.lastCfg.Host != "imap.fake.test" || dialer.lastCfg.Username != "testuser" {
		t.Fatalf("unexpected dial config: %#v", dialer.lastCfg)
	}
	store, err = eventstore.Open(dbPath)
	if err != nil {
		t.Fatalf("reopen store: %v", err)
	}
	defer store.Close()
	v, u, err := store.GetIMAPHWM(context.Background(), "imap.fake.test", "INBOX")
	if err != nil || v == nil || *v != 100 || u == nil || *u != 10 {
		t.Fatalf("unexpected HWM: val=%v, uid=%v, err=%v", v, u, err)
	}
}

func TestCLIPollInboxOAuth2FlagsReachDialer(t *testing.T) {
	t.Setenv("SYMERASEME_DATA_DIR", t.TempDir())
	t.Setenv("IMAP_PASSWORD", "password-fallback")
	t.Setenv("IMAP_OAUTH2_ACCESS_TOKEN", "")

	dialer := &fakeCLIPollDialer{session: &fakeCLIPollSession{uidValidity: 1}}
	originalHandler := pollInboxHandler
	pollInboxHandler = func() mcp.Handler {
		return mcp.ContractHandlerWithOptions(mcp.ContractHandlerOptions{IMAPDialer: dialer})
	}
	t.Cleanup(func() { pollInboxHandler = originalHandler })

	if _, err := execute(t, "poll-inbox", "--host", "imap.cli.test", "--username", "cli-user@example.test", "--oauth2-username", "oauth-cli@example.test", "--oauth2-access-token", "cli-access-token"); err != nil {
		t.Fatalf("poll-inbox CLI failed: %v", err)
	}
	if dialer.lastCfg.OAuth2 == nil || dialer.lastCfg.OAuth2.Username != "oauth-cli@example.test" || dialer.lastCfg.OAuth2.AccessToken != "cli-access-token" {
		t.Fatalf("CLI OAuth2 config = %#v", dialer.lastCfg.OAuth2)
	}
	if dialer.lastCfg.Password != "password-fallback" {
		t.Fatalf("password fallback = %q, want preserved fallback", dialer.lastCfg.Password)
	}
}

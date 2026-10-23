// Command mcp-auto-confirm is a process oracle for the stored-reply fallback.
package main

import (
	"bytes"
	"context"
	"crypto/sha256"
	"encoding/hex"
	"encoding/json"
	"fmt"
	"os"
	"path/filepath"
	"runtime"
	"strings"

	"github.com/danieljustus/symaira-eraseme/internal/config"
	"github.com/danieljustus/symaira-eraseme/internal/eventstore"
	"github.com/danieljustus/symaira-eraseme/internal/mcp"
	"github.com/danieljustus/symaira-eraseme/internal/replies"
)

const (
	sourceRevision = "e8a8c969cb1a3b5a7f77dfe28f807e3707d0a8d8"
	sourcePath     = "internal/confirmation/confirmation.go, internal/replies/service.go"
)

type snapshot struct {
	Tasks        int    `json:"tasks"`
	HumanEvents  int    `json:"human_action_required_events"`
	ClickEvents  int    `json:"confirmation_link_clicked_events"`
	FailureNotes int    `json:"failure_notes"`
	FormURL      string `json:"form_url"`
	Instructions string `json:"instructions"`
	TaskStatus   string `json:"task_status"`
}

type oracleResult struct {
	GoVersion          string            `json:"go_version"`
	SourcesSHA256      map[string]string `json:"sources_sha256"`
	SourceRevision     string            `json:"source_revision"`
	SourcePath         string            `json:"source_path"`
	DryRunResponse     string            `json:"dry_run_response"`
	DryRunState        snapshot          `json:"dry_run_state"`
	ManualResponse     string            `json:"manual_response"`
	FinalState         snapshot          `json:"final_state"`
	NoLinksDryResponse string            `json:"no_links_dry_response"`
	NoLinksResponse    string            `json:"no_links_response"`
	NoLinksNotes       int               `json:"no_links_notes"`
}

func main() {
	sourceRoot, err := os.Getwd()
	if err != nil {
		fail(err)
	}
	sources := map[string]string{}
	for _, path := range []string{"go.mod", "go.sum", "internal/mcp/contract_handler.go", "internal/mcp/server.go", "internal/confirmation/confirmation.go", "internal/replies/service.go", "rust-tests/parity/oracle/mcp-auto-confirm/main.go"} {
		data, err := os.ReadFile(path)
		if err != nil {
			fail(err)
		}
		digest := sha256.Sum256(data)
		sources[path] = hex.EncodeToString(digest[:])
	}
	root, err := os.MkdirTemp("", "mcp-auto-confirm")
	if err != nil {
		fail(err)
	}
	defer func() {
		_ = os.Chdir(sourceRoot)
		_ = os.RemoveAll(root)
	}()
	systemRoot := os.Getenv("SystemRoot")
	os.Clearenv()
	if systemRoot != "" {
		if err := os.Setenv("SystemRoot", systemRoot); err != nil {
			fail(err)
		}
	}
	for _, key := range []string{"HOME", "USERPROFILE", "XDG_CONFIG_HOME", "XDG_DATA_HOME", "XDG_STATE_HOME", "XDG_CACHE_HOME", "TMPDIR", "TMP", "TEMP"} {
		if err := os.Setenv(key, root); err != nil {
			fail(err)
		}
	}
	if err := os.Chdir(root); err != nil {
		fail(err)
	}
	dataDir := filepath.Join(root, "data")
	if err := os.MkdirAll(dataDir, 0o700); err != nil {
		fail(err)
	}
	if err := os.Setenv("SYMERASEME_DATA_DIR", dataDir); err != nil {
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
	ctx := context.Background()
	requestID, err := store.CreateRemovalRequest(ctx, "broker-a", "email", "mcp-auto-confirm", "GDPR", "", "")
	if err != nil {
		fail(err)
	}
	if _, err := replies.NewRepository(store).InsertReply(ctx, &requestID, "msg-1", "thread-1", "broker@acxiom.com", "Confirm", "Confirm here: https://acxiom.com/confirm", ""); err != nil {
		fail(err)
	}
	noLinksID, err := store.CreateRemovalRequest(ctx, "broker-b", "email", "mcp-auto-confirm", "GDPR", "", "")
	if err != nil {
		fail(err)
	}
	if _, err := replies.NewRepository(store).InsertReply(ctx, &noLinksID, "msg-2", "thread-2", "broker@acxiom.com", "No link", "", ""); err != nil {
		fail(err)
	}
	if _, err := store.DB().Exec(`UPDATE inbox_replies SET snippet = NULL WHERE request_id = ?`, noLinksID); err != nil {
		fail(err)
	}
	if err := store.Close(); err != nil {
		fail(err)
	}

	dryResponse := call(callRequest(requestID, true))
	dryState := inspect(storage.DBPath)
	manualResponse := call(callRequest(requestID, false))
	finalState := inspect(storage.DBPath)
	noLinksDryResponse := call(callRequest(noLinksID, true))
	noLinksResponse := call(callRequest(noLinksID, false))
	noLinksStore, err := eventstore.Open(storage.DBPath)
	if err != nil {
		fail(err)
	}
	var noLinksNotes int
	if err := noLinksStore.DB().QueryRow(`SELECT count(*) FROM request_events WHERE request_id = ? AND event_type = 'NOTE_ADDED'`, noLinksID).Scan(&noLinksNotes); err != nil {
		fail(err)
	}
	if err := noLinksStore.Close(); err != nil {
		fail(err)
	}

	if err := json.NewEncoder(os.Stdout).Encode(oracleResult{
		GoVersion:          runtime.Version(),
		SourcesSHA256:      sources,
		SourceRevision:     sourceRevision,
		SourcePath:         sourcePath,
		DryRunResponse:     dryResponse,
		DryRunState:        dryState,
		ManualResponse:     manualResponse,
		FinalState:         finalState,
		NoLinksDryResponse: noLinksDryResponse,
		NoLinksResponse:    noLinksResponse,
		NoLinksNotes:       noLinksNotes,
	}); err != nil {
		fail(err)
	}
}

func callRequest(requestID int64, dryRun bool) string {
	return fmt.Sprintf(`{"jsonrpc":"2.0","id":1,"method":"tools/call","params":{"name":"auto_confirm","arguments":{"request_id":%d,"dry_run":%t}}}`, requestID, dryRun)
}

func call(request string) string {
	var output bytes.Buffer
	server := mcp.NewServer(mcp.ContractHandler())
	if err := server.ServeStdio(context.Background(), strings.NewReader(request+"\n"), &output); err != nil {
		fail(err)
	}
	return output.String()
}

func inspect(path string) snapshot {
	store, err := eventstore.Open(path)
	if err != nil {
		fail(err)
	}
	defer store.Close()
	var state snapshot
	if err := store.DB().QueryRow(`SELECT count(*) FROM manual_tasks`).Scan(&state.Tasks); err != nil {
		fail(err)
	}
	if err := store.DB().QueryRow(`SELECT count(*) FROM request_events WHERE event_type = 'HUMAN_ACTION_REQUIRED'`).Scan(&state.HumanEvents); err != nil {
		fail(err)
	}
	if err := store.DB().QueryRow(`SELECT count(*) FROM request_events WHERE event_type = 'CONFIRMATION_LINK_CLICKED'`).Scan(&state.ClickEvents); err != nil {
		fail(err)
	}
	if err := store.DB().QueryRow(`SELECT count(*) FROM request_events WHERE event_type = 'NOTE_ADDED'`).Scan(&state.FailureNotes); err != nil {
		fail(err)
	}
	_ = store.DB().QueryRow(`SELECT form_url, instructions, status FROM manual_tasks ORDER BY id LIMIT 1`).Scan(&state.FormURL, &state.Instructions, &state.TaskStatus)
	return state
}

func fail(err error) {
	fmt.Fprintln(os.Stderr, err)
	os.Exit(1)
}

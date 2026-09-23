// Command mcp-tools-call is the byte oracle for MCP `tools/call` responses
// produced by the real contract handler.
//
// Unlike the earlier MCP oracles this one deliberately does NOT use a stub
// handler: it wires `mcp.ContractHandler()`, so every recorded response is the
// production answer for the given tool and arguments.
package main

import (
	"bytes"
	"context"
	"crypto/sha256"
	"database/sql"
	"encoding/base64"
	"encoding/hex"
	"encoding/json"
	"fmt"
	"io"
	"os"
	"os/exec"
	"path/filepath"
	"strings"

	"github.com/danieljustus/symaira-eraseme/internal/mcp"

	_ "modernc.org/sqlite"
)

const (
	sourceRevision = "79bf23e83b31f18d98487101200eaf32749e5a46"
	sourcePath     = "internal/mcp/contract_handler.go:167-320"
	// storeFixtureDir holds the store-backed read cases and the frozen rows
	// both sides read.
	storeFixtureDir = "tests/fixtures/mcp-contract/mcp-003-store"
)

const schedulerSourceRevision = "4af87d9d2cd127722aa4d0e3942057b0365bd7d9"

var schedulerSourceFiles = []sourceFileDigest{
	{Path: "internal/mcp/contract_handler.go", SHA256: "b70d4a121aaced8a0efc548380e95f2618c5a172f456c0e944a36921338b488f"},
	{Path: "internal/scheduler/scheduler.go", SHA256: "46b18551267d75eeeeb675f1f6af00e3201c63e3db64307a174ccc5f327c3138"},
}

type sourceFileDigest struct {
	Path   string `json:"path"`
	SHA256 string `json:"sha256"`
}

type fixtureCase struct {
	Name       string  `json:"name"`
	Request    string  `json:"request,omitempty"`
	Response   *string `json:"response"`
	ParseError bool    `json:"parse_error,omitempty"`
}

func main() {
	if len(os.Args) == 2 && os.Args[1] == "--scheduler-fixture" {
		writeSchedulerFixtures()
		return
	}
	if len(os.Args) == 2 && os.Args[1] == "--fixture" {
		writeFixture()
		writeStoreFixtures()
		writeSchedulerFixtures()
		return
	}
	raw, err := io.ReadAll(os.Stdin)
	if err != nil {
		fail(err)
	}
	var output bytes.Buffer
	server := mcp.NewServer(mcp.ContractHandler())
	if err := server.ServeStdio(context.Background(), bytes.NewReader(append(raw, '\n')), &output); err != nil {
		fail(err)
	}
	if err := json.NewEncoder(os.Stdout).Encode(struct {
		BodyB64 string `json:"body_b64"`
	}{BodyB64: base64.StdEncoding.EncodeToString(output.Bytes())}); err != nil {
		fail(err)
	}
}

// writeSchedulerFixtures measures real install/status/uninstall calls while
// every scheduler command resolves to a private fake crontab on PATH.
func writeSchedulerFixtures() {
	verifySchedulerSources()
	root, err := os.MkdirTemp("", "mcp-scheduler-oracle")
	if err != nil {
		fail(err)
	}
	defer os.RemoveAll(root)

	home := filepath.Join(root, "home")
	data := filepath.Join(root, "data")
	bin := filepath.Join(root, "bin")
	tmp := filepath.Join(root, "tmp")
	state := filepath.Join(root, "crontab")
	for _, dir := range []string{home, data, bin, tmp} {
		if err := os.MkdirAll(dir, 0o700); err != nil {
			fail(err)
		}
	}
	if err := os.WriteFile(state, []byte("# user schedule\n"), 0o600); err != nil {
		fail(err)
	}
	script := `#!/bin/sh
case "$1" in
  -l)
    [ -f "$SCHEDULER_CRONTAB_STATE" ] || exit 1
    while IFS= read -r line; do printf '%s\n' "$line"; done < "$SCHEDULER_CRONTAB_STATE"
    ;;
  *)
    while IFS= read -r line; do printf '%s\n' "$line"; done < "$1" > "$SCHEDULER_CRONTAB_STATE"
    ;;
esac
`
	if err := os.WriteFile(filepath.Join(bin, "crontab"), []byte(script), 0o700); err != nil {
		fail(err)
	}
	origin, err := os.Getwd()
	if err != nil {
		fail(err)
	}
	oldHome, hadHome := os.LookupEnv("HOME")
	oldPath, hadPath := os.LookupEnv("PATH")
	oldData, hadData := os.LookupEnv("SYMERASEME_DATA_DIR")
	oldState, hadState := os.LookupEnv("SCHEDULER_CRONTAB_STATE")
	oldTemp, hadTemp := os.LookupEnv("TMPDIR")
	oldXDGConfig, hadXDGConfig := os.LookupEnv("XDG_CONFIG_HOME")
	oldXDGData, hadXDGData := os.LookupEnv("XDG_DATA_HOME")
	oldXDGState, hadXDGState := os.LookupEnv("XDG_STATE_HOME")
	oldXDGCache, hadXDGCache := os.LookupEnv("XDG_CACHE_HOME")
	defer func() {
		_ = os.Chdir(origin)
		restoreEnv("HOME", oldHome, hadHome)
		restoreEnv("PATH", oldPath, hadPath)
		restoreEnv("SYMERASEME_DATA_DIR", oldData, hadData)
		restoreEnv("SCHEDULER_CRONTAB_STATE", oldState, hadState)
		restoreEnv("TMPDIR", oldTemp, hadTemp)
		restoreEnv("XDG_CONFIG_HOME", oldXDGConfig, hadXDGConfig)
		restoreEnv("XDG_DATA_HOME", oldXDGData, hadXDGData)
		restoreEnv("XDG_STATE_HOME", oldXDGState, hadXDGState)
		restoreEnv("XDG_CACHE_HOME", oldXDGCache, hadXDGCache)
	}()
	for key, value := range map[string]string{
		"HOME": home, "PATH": bin, "SYMERASEME_DATA_DIR": data, "TMPDIR": tmp,
		"XDG_CONFIG_HOME": filepath.Join(home, "config"), "XDG_DATA_HOME": filepath.Join(home, "data"),
		"XDG_STATE_HOME": filepath.Join(home, "state"), "XDG_CACHE_HOME": filepath.Join(home, "cache"),
		"SCHEDULER_CRONTAB_STATE": state,
	} {
		if err := os.Setenv(key, value); err != nil {
			fail(err)
		}
	}
	if err := os.Chdir(root); err != nil {
		fail(err)
	}
	canonicalRoot, err := filepath.EvalSymlinks(root)
	if err != nil {
		fail(err)
	}
	cases := []fixtureCase{
		{Name: "schedule_install_writes_only_through_private_crontab", Request: callRequest(1, "schedule_install", `{"platform":"cron","tick_hour":9,"tick_minute":30}`)},
		{Name: "schedule_status_reports_the_installed_cron_block", Request: callRequest(2, "schedule_status", `{"platform":"cron"}`)},
		{Name: "schedule_uninstall_removes_only_the_managed_cron_block", Request: callRequest(3, "schedule_uninstall", `{"platform":"cron"}`)},
		{Name: "schedule_status_reports_the_uninstalled_cron_block", Request: callRequest(4, "schedule_status", `{"platform":"cron"}`)},
	}
	nativeCases := []fixtureCase{
		schedulerInstallCase("launchd", 5),
		schedulerInstallCase("systemd", 6),
	}
	crontabAfterInstall := false
	filesAfterInstall := []string(nil)
	for index := range cases {
		var output bytes.Buffer
		server := mcp.NewServer(mcp.ContractHandler())
		if err := server.ServeStdio(context.Background(), bytes.NewReader(append([]byte(cases[index].Request), '\n')), &output); err != nil {
			fail(err)
		}
		if output.Len() != 0 {
			body := strings.ReplaceAll(output.String(), canonicalRoot, "<SCHEDULE_ROOT>")
			body = strings.ReplaceAll(body, root, "<SCHEDULE_ROOT>")
			cases[index].Response = &body
		}
		if index == 0 {
			installed, err := os.ReadFile(state)
			if err != nil {
				fail(err)
			}
			crontabAfterInstall = strings.Contains(string(installed), "# Symaira EraseMe scheduled tasks")
			files, err := os.ReadDir(filepath.Join(root, "schedules"))
			if err != nil {
				fail(err)
			}
			filesAfterInstall = make([]string, 0, len(files))
			for _, file := range files {
				filesAfterInstall = append(filesAfterInstall, file.Name())
			}
		}
	}

	cronAfterUninstall, err := os.ReadFile(state)
	if err != nil {
		fail(err)
	}
	fixture := struct {
		SourceRevision string             `json:"source_revision"`
		SourceFiles    []sourceFileDigest `json:"source_files"`
		SourcePath     string             `json:"source_path"`
		Cases          []fixtureCase      `json:"cases"`
		NativeCases    []fixtureCase      `json:"native_install_cases"`
		SideEffects    struct {
			FilesAfterInstall   []string `json:"files_after_install"`
			CrontabAfterInstall bool     `json:"crontab_after_install"`
			CrontabAfterRemove  string   `json:"crontab_after_uninstall"`
		} `json:"side_effects"`
	}{
		SourceRevision: schedulerSourceRevision,
		SourceFiles:    schedulerSourceFiles,
		SourcePath:     "internal/mcp/contract_handler.go; internal/scheduler/scheduler.go",
		Cases:          cases,
		NativeCases:    nativeCases,
	}
	fixture.SideEffects.FilesAfterInstall = filesAfterInstall
	fixture.SideEffects.CrontabAfterInstall = crontabAfterInstall
	fixture.SideEffects.CrontabAfterRemove = string(cronAfterUninstall)
	encoded, err := json.MarshalIndent(fixture, "", "  ")
	if err != nil {
		fail(err)
	}
	destination := filepath.Join(origin, "tests", "fixtures", "mcp-contract", "mcp-003-scheduler", "cases.json")
	if err := os.MkdirAll(filepath.Dir(destination), 0o755); err != nil {
		fail(err)
	}
	if err := os.WriteFile(destination, append(encoded, '\n'), 0o644); err != nil {
		fail(err)
	}
}

func verifySchedulerSources() {
	for _, source := range schedulerSourceFiles {
		pinned, err := exec.Command("git", "show", schedulerSourceRevision+":"+source.Path).Output()
		if err != nil {
			fail(fmt.Errorf("read pinned Go oracle source %s: %w", source.Path, err))
		}
		working, err := os.ReadFile(source.Path)
		if err != nil {
			fail(fmt.Errorf("read working Go oracle source %s: %w", source.Path, err))
		}
		pinnedHash := sha256.Sum256(pinned)
		workingHash := sha256.Sum256(working)
		if !bytes.Equal(pinned, working) || hex.EncodeToString(workingHash[:]) != source.SHA256 || hex.EncodeToString(pinnedHash[:]) != source.SHA256 {
			fail(fmt.Errorf("Go oracle source %s does not match pinned revision %s", source.Path, schedulerSourceRevision))
		}
	}
}

func schedulerInstallCase(platform string, id int) fixtureCase {
	root, err := os.MkdirTemp("", "mcp-scheduler-"+platform)
	if err != nil {
		fail(err)
	}
	defer os.RemoveAll(root)

	home, data, tmp, bin := filepath.Join(root, "home"), filepath.Join(root, "data"), filepath.Join(root, "tmp"), filepath.Join(root, "bin")
	for _, dir := range []string{home, data, tmp, bin} {
		if err := os.MkdirAll(dir, 0o700); err != nil {
			fail(err)
		}
	}
	command := "launchctl"
	if platform == "systemd" {
		command = "systemctl"
	}
	if err := os.WriteFile(filepath.Join(bin, command), []byte("#!/bin/sh\nexit 0\n"), 0o700); err != nil {
		fail(err)
	}
	origin, err := os.Getwd()
	if err != nil {
		fail(err)
	}
	keys := []string{"HOME", "PATH", "SYMERASEME_DATA_DIR", "TMPDIR", "XDG_CONFIG_HOME", "XDG_DATA_HOME", "XDG_STATE_HOME", "XDG_CACHE_HOME"}
	old := make(map[string]string, len(keys))
	existed := make(map[string]bool, len(keys))
	for _, key := range keys {
		old[key], existed[key] = os.LookupEnv(key)
	}
	defer func() {
		_ = os.Chdir(origin)
		for _, key := range keys {
			restoreEnv(key, old[key], existed[key])
		}
	}()
	for key, value := range map[string]string{
		"HOME": home, "PATH": bin, "SYMERASEME_DATA_DIR": data, "TMPDIR": tmp,
		"XDG_CONFIG_HOME": filepath.Join(home, "config"), "XDG_DATA_HOME": filepath.Join(home, "data"),
		"XDG_STATE_HOME": filepath.Join(home, "state"), "XDG_CACHE_HOME": filepath.Join(home, "cache"),
	} {
		if err := os.Setenv(key, value); err != nil {
			fail(err)
		}
	}
	if err := os.Chdir(root); err != nil {
		fail(err)
	}
	canonicalRoot, err := filepath.EvalSymlinks(root)
	if err != nil {
		fail(err)
	}
	request := callRequest(id, "schedule_install", fmt.Sprintf(`{"platform":%q,"tick_hour":9,"tick_minute":30}`, platform))
	var output bytes.Buffer
	if err := mcp.NewServer(mcp.ContractHandler()).ServeStdio(context.Background(), bytes.NewReader(append([]byte(request), '\n')), &output); err != nil {
		fail(err)
	}
	response := strings.ReplaceAll(output.String(), canonicalRoot, "<SCHEDULE_ROOT>")
	response = strings.ReplaceAll(response, root, "<SCHEDULE_ROOT>")
	return fixtureCase{Name: "schedule_install_" + platform + "_returns_empty_legacy_array", Request: request, Response: &response}
}

func restoreEnv(key, value string, existed bool) {
	if existed {
		_ = os.Setenv(key, value)
	} else {
		_ = os.Unsetenv(key)
	}
}

func callRequest(id int, name string, arguments string) string {
	return fmt.Sprintf(`{"jsonrpc":"2.0","id":%d,"method":"tools/call","params":{"name":"%s","arguments":%s}}`, id, name, arguments)
}

func writeFixture() {
	workspace, err := os.MkdirTemp("", "mcp-tools-call")
	if err != nil {
		fail(err)
	}
	defer os.RemoveAll(workspace)

	// The MCP handler reads with the process working directory as its
	// workspace root, so the fixture uses relative names and runs from the
	// workspace. That keeps every recorded request reproducible.
	if err := os.WriteFile(filepath.Join(workspace, "pii.txt"), []byte("Contact jane.doe@example.com or 555-123-4567.\n"), 0o600); err != nil {
		fail(err)
	}
	if err := os.WriteFile(filepath.Join(workspace, "clean.txt"), []byte("No personal data here.\n"), 0o600); err != nil {
		fail(err)
	}
	// A small registry for the `validate` cases: the three golden brokers with
	// a manifest and schema stub, referenced by a relative path so the request
	// stays reproducible.
	registryDir := filepath.Join(workspace, "registry")
	for _, sub := range []string{"schemas", "brokers/eu", "brokers/uk", "brokers/us"} {
		if err := os.MkdirAll(filepath.Join(registryDir, sub), 0o755); err != nil {
			fail(err)
		}
	}
	if err := os.WriteFile(filepath.Join(registryDir, "manifest.json"), []byte(`{"schema_version":1,"schemas":{"broker":"schemas/broker.schema.json"}}`), 0o644); err != nil {
		fail(err)
	}
	if err := os.WriteFile(filepath.Join(registryDir, "schemas", "broker.schema.json"), []byte(`{"schema_version":1}`), 0o644); err != nil {
		fail(err)
	}
	for sub, file := range map[string]string{"eu": "golden-email-eu.yaml", "uk": "golden-multi-uk.yaml", "us": "golden-webform-us.yaml"} {
		source := filepath.Join("tests", "fixtures", "registry-contract", file)
		content, err := os.ReadFile(source)
		if err != nil {
			fail(err)
		}
		target := filepath.Join(registryDir, "brokers", sub, file)
		if err := os.WriteFile(target, content, 0o644); err != nil {
			fail(err)
		}
	}

	origin, err := os.Getwd()
	if err != nil {
		fail(err)
	}
	if err := os.Chdir(workspace); err != nil {
		fail(err)
	}
	defer func() { _ = os.Chdir(origin) }()

	cases := []fixtureCase{
		{
			Name:    "redact_file_returns_the_redacted_text",
			Request: callRequest(1, "redact_file", `{"path":"pii.txt"}`),
		},
		{
			Name:    "redact_file_leaves_clean_text_unchanged",
			Request: callRequest(2, "redact_file", `{"path":"clean.txt"}`),
		},
		{
			Name:    "redact_file_reports_a_missing_file_as_a_tool_error",
			Request: callRequest(3, "redact_file", `{"path":"missing.txt"}`),
		},
		{
			// `status` passes validation as the legacy alias but has no case in
			// the contract handler, so Go answers with its default.
			Name:    "legacy_status_alias_hits_the_handler_default",
			Request: callRequest(4, "status", `{}`),
		},
		{
			Name:    "validate_reports_a_clean_registry",
			Request: callRequest(5, "validate", `{"registry_dir":"registry"}`),
		},
		{
			// `grant` with a dry run echoes its arguments and touches no store,
			// so it is fully reproducible.
			Name:    "grant_dry_run_echoes_defaults",
			Request: callRequest(6, "grant", `{"dry_run":true}`),
		},
		{
			Name:    "grant_dry_run_echoes_explicit_arguments",
			Request: callRequest(7, "grant", `{"dry_run":true,"command":"execute","revoke":"token-1","revoke_all":true}`),
		},
		// Not recorded: `plan_create` answers with the removal-request row ids it
		// just created, and this oracle's store isolation did not hold — the ids
		// came from the developer's real store (12351+) and grew between runs.
		// Pinning them would bake a moving value into the contract, so the tool is
		// covered by shape assertions instead.
		// Not recorded: `list_brokers` answers with the registry itself. With
		// `include_inactive` the status filter is ignored and the answer is 1274
		// brokers (985 KB); without it, 1273 active ones. Either way the fixture
		// would duplicate the embedded registry, whose model and filter semantics
		// already have byte-exact coverage in the registry goldens, so the tool is
		// covered by shape assertions instead.
		// Not recorded: `schedule_install` accepts only platform/tick fields, so the
		// paths its templates embed come from Go's defaults. Those resolve to the
		// running binary, and under `go run` that is a fresh temp directory every
		// time — the answer changed between two runs. The file *names* are stable,
		// so the tool is covered by shape assertions instead.
		{
			// The dry run returns the generated file *contents*, so pinning the
			// paths in the request makes it reproducible.
			Name:    "generate_scheduler_dry_run_returns_file_contents",
			Request: callRequest(8, "generate_scheduler", `{"dry_run":true,"platform":"cron","project_dir":"/srv/project","symeraseme_bin":"/usr/local/bin/symeraseme","output_dir":"./schedules","tick_hour":9,"tick_minute":30,"poll_hours":"8,20"}`),
		},
	}

	for index := range cases {
		var output bytes.Buffer
		server := mcp.NewServer(mcp.ContractHandler())
		err := server.ServeStdio(context.Background(), bytes.NewReader(append([]byte(cases[index].Request), '\n')), &output)
		if err != nil {
			cases[index].ParseError = true
		} else if body := output.String(); body != "" {
			cases[index].Response = &body
		}
	}

	encoder := json.NewEncoder(os.Stdout)
	encoder.SetIndent("", "  ")
	if err := encoder.Encode(struct {
		SourceRevision string        `json:"source_revision"`
		SourcePath     string        `json:"source_path"`
		Cases          []fixtureCase `json:"cases"`
	}{SourceRevision: sourceRevision, SourcePath: sourcePath, Cases: cases}); err != nil {
		fail(err)
	}
}

func fail(err error) {
	fmt.Fprintln(os.Stderr, err)
	os.Exit(1)
}

// storeCase is one recorded request against a store-backed tool.
type storeCase struct {
	name    string
	request string
}

// writeStoreFixtures records `plan_show`, `list_requests` and `get_events`
// against a store the handler created itself.
//
// The rows come from `mcp-003-store/seed.sql` rather than from the product's
// write path: Go stamps `created_at`/`recorded_at` with `time.Now()` and offers
// no injection point, so a store the product wrote would carry a wall clock and
// no response over it could be pinned. The frozen rows are fixture input, like
// the registry YAML the `validate` cases read; the contract these three tools
// carry is the read path. Both this oracle and the Rust test execute the same
// file, so the bytes are comparable.
//
// The empty-store fixture stays separate from the seeded one: one store cannot
// be both, and keeping them apart removes any ordering coupling.
func writeStoreFixtures() {
	seed, err := os.ReadFile(filepath.Join(storeFixtureDir, "seed.sql"))
	if err != nil {
		fail(err)
	}

	empty := []storeCase{
		{
			name:    "plan_show_labels_an_unfiltered_empty_store_as_all",
			request: callRequest(1, "plan_show", `{}`),
		},
		{
			name:    "plan_show_keeps_an_explicit_campaign_label_without_rows",
			request: callRequest(2, "plan_show", `{"campaign_id":"campaign-x","status":"SENT"}`),
		},
		{
			name:    "list_requests_reports_an_empty_first_page",
			request: callRequest(3, "list_requests", `{}`),
		},
		{
			// Go rejects a non-positive page before it opens a query.
			name:    "list_requests_rejects_page_zero",
			request: callRequest(4, "list_requests", `{"page":0}`),
		},
		{
			name:    "list_requests_rejects_page_size_zero",
			request: callRequest(5, "list_requests", `{"page_size":0}`),
		},
		{
			name:    "get_events_reports_no_events_for_an_unknown_request",
			request: callRequest(6, "get_events", `{"request_id":7}`),
		},
		{
			name:    "get_events_treats_request_zero_as_unknown",
			request: callRequest(7, "get_events", `{"request_id":0,"after_event_id":0}`),
		},
	}

	seeded := []storeCase{
		{
			name:    "plan_show_lists_every_request_under_the_all_label",
			request: callRequest(1, "plan_show", `{}`),
		},
		{
			name:    "plan_show_filters_by_campaign",
			request: callRequest(2, "plan_show", `{"campaign_id":"campaign-1"}`),
		},
		{
			name:    "plan_show_filters_by_campaign_and_status",
			request: callRequest(3, "plan_show", `{"campaign_id":"campaign-1","status":"HUMAN_ACTION_REQUIRED"}`),
		},
		{
			name:    "list_requests_returns_the_default_page",
			request: callRequest(4, "list_requests", `{}`),
		},
		{
			name:    "list_requests_paginates_with_offset_and_limit",
			request: callRequest(5, "list_requests", `{"page":2,"page_size":2}`),
		},
		{
			// The broker filter narrows the rows but not the count: Go passes
			// only campaign and status to CountRemovalRequests.
			name:    "list_requests_filters_by_broker_without_narrowing_the_total",
			request: callRequest(6, "list_requests", `{"broker_id":"broker-a"}`),
		},
		{
			name:    "list_requests_filters_by_status",
			request: callRequest(7, "list_requests", `{"status":"SENT"}`),
		},
		{
			name:    "list_requests_answers_an_unmatched_filter_with_null_rows",
			request: callRequest(8, "list_requests", `{"status":"CONFIRMED"}`),
		},
		{
			name:    "get_events_orders_a_request_by_occurred_at_then_id",
			request: callRequest(9, "get_events", `{"request_id":1}`),
		},
		{
			name:    "get_events_resumes_after_an_event_id",
			request: callRequest(10, "get_events", `{"request_id":1,"after_event_id":2}`),
		},
		{
			name:    "get_events_returns_a_payload_verbatim",
			request: callRequest(11, "get_events", `{"request_id":2}`),
		},
		{
			name:    "get_events_passes_an_unknown_event_type_through",
			request: callRequest(12, "get_events", `{"request_id":3}`),
		},
		{
			name:    "get_events_reports_no_events_for_an_unknown_request",
			request: callRequest(13, "get_events", `{"request_id":99}`),
		},
	}

	recordStoreFixture("empty-cases.json", nil, empty)
	recordStoreFixture("seeded-cases.json", seed, seeded)
}

// recordStoreFixture runs one fixture family inside its own isolated workspace:
// a fresh data directory (so the developer's store can never leak in, which is
// what invalidated the earlier `plan_create` attempt), a private
// `XDG_CONFIG_HOME`, a schema the handler creates, and then the frozen rows.
func recordStoreFixture(file string, seed []byte, cases []storeCase) {
	workspace, err := os.MkdirTemp("", "mcp-store-reads")
	if err != nil {
		fail(err)
	}
	defer os.RemoveAll(workspace)

	dataDir := filepath.Join(workspace, "data")
	for _, dir := range []string{dataDir, filepath.Join(workspace, "xdg")} {
		if err := os.MkdirAll(dir, 0o700); err != nil {
			fail(err)
		}
	}
	if err := os.Setenv("SYMERASEME_DATA_DIR", dataDir); err != nil {
		fail(err)
	}
	if err := os.Setenv("SYMERASEME_DB_DIR", dataDir); err != nil {
		fail(err)
	}
	if err := os.Setenv("XDG_CONFIG_HOME", filepath.Join(workspace, "xdg")); err != nil {
		fail(err)
	}

	origin, err := os.Getwd()
	if err != nil {
		fail(err)
	}
	if err := os.Chdir(workspace); err != nil {
		fail(err)
	}
	defer func() { _ = os.Chdir(origin) }()

	// One request through the handler creates the store with its schema; the
	// answer itself is discarded.
	warmUp := mcp.NewServer(mcp.ContractHandler())
	if err := warmUp.ServeStdio(
		context.Background(),
		bytes.NewReader([]byte(callRequest(0, "list_requests", `{}`)+"\n")),
		&bytes.Buffer{},
	); err != nil {
		fail(err)
	}
	if seed != nil {
		applySeed(filepath.Join(dataDir, "symeraseme.db"), seed)
	}

	recorded := make([]fixtureCase, 0, len(cases))
	for index := range cases {
		recorded = append(recorded, fixtureCase{
			Name:    cases[index].name,
			Request: cases[index].request,
		})
	}
	for index := range recorded {
		var output bytes.Buffer
		server := mcp.NewServer(mcp.ContractHandler())
		err := server.ServeStdio(
			context.Background(),
			bytes.NewReader(append([]byte(recorded[index].Request), '\n')),
			&output,
		)
		if err != nil {
			recorded[index].ParseError = true
		} else if body := output.String(); body != "" {
			recorded[index].Response = &body
		}
	}

	target, err := os.Create(filepath.Join(origin, storeFixtureDir, file))
	if err != nil {
		fail(err)
	}
	defer target.Close()
	encoder := json.NewEncoder(target)
	encoder.SetIndent("", "  ")
	if err := encoder.Encode(struct {
		SourceRevision string        `json:"source_revision"`
		SourcePath     string        `json:"source_path"`
		Seed           string        `json:"seed,omitempty"`
		Cases          []fixtureCase `json:"cases"`
	}{SourceRevision: sourceRevision, SourcePath: sourcePath, Seed: seedFile(seed), Cases: recorded}); err != nil {
		fail(err)
	}
}

func seedFile(seed []byte) string {
	if seed == nil {
		return ""
	}
	return filepath.Join(storeFixtureDir, "seed.sql")
}

// applySeed runs the frozen rows. The file holds one statement per line (no
// semicolons inside values), which keeps the Go and Rust readers identical.
func applySeed(dbPath string, seed []byte) {
	db, err := sql.Open("sqlite", dbPath)
	if err != nil {
		fail(err)
	}
	defer db.Close()
	for _, line := range strings.Split(string(seed), "\n") {
		statement := strings.TrimSpace(line)
		if statement == "" || strings.HasPrefix(statement, "--") {
			continue
		}
		if _, err := db.Exec(statement); err != nil {
			fail(fmt.Errorf("seed %q: %w", statement, err))
		}
	}
}

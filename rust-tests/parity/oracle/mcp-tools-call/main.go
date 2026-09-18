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
	"encoding/base64"
	"encoding/json"
	"fmt"
	"io"
	"os"
	"path/filepath"

	"github.com/danieljustus/symaira-eraseme/internal/mcp"
)

const (
	sourceRevision = "79bf23e83b31f18d98487101200eaf32749e5a46"
	sourcePath     = "internal/mcp/contract_handler.go:167-320"
)

type fixtureCase struct {
	Name       string  `json:"name"`
	Request    string  `json:"request,omitempty"`
	Response   *string `json:"response"`
	ParseError bool    `json:"parse_error,omitempty"`
}

func main() {
	if len(os.Args) == 2 && os.Args[1] == "--fixture" {
		writeFixture()
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

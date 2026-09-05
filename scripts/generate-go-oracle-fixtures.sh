#!/usr/bin/env bash
# Generate deterministic black-box Go oracle fixtures from the corrected commit.
# The temporary Cobra test is injected only into a clean git-archive export.
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "${SCRIPT_DIR}/.." && pwd)"
PINNED_COMMIT="bf53346eec234929bedf0314b99e3da85dbb991b"
OUT_DIR="${REPO_ROOT}/rust-tests/parity"
RUNTIME_ROOT="/tmp/symeraseme-go-oracle"

usage() {
    printf '%s\n' "Usage: $0" "" "Generate CLI, MCP, HTTP, and filesystem fixtures from ${PINNED_COMMIT}."
}

if [[ "${1:-}" == "-h" || "${1:-}" == "--help" ]]; then
    usage
    exit 0
fi
if [[ $# -ne 0 ]]; then
    printf 'error: unknown argument %q\n' "$1" >&2
    usage >&2
    exit 2
fi

cd "${REPO_ROOT}"
if ! git cat-file -e "${PINNED_COMMIT}^{commit}"; then
    printf 'error: pinned Go oracle commit is unavailable: %s\n' "${PINNED_COMMIT}" >&2
    exit 1
fi

SCRATCH="$(mktemp -d "${TMPDIR:-/tmp}/symeraseme-oracle.XXXXXX")"
cleanup() {
    rm -rf "${SCRATCH}" "${RUNTIME_ROOT}" 2>/dev/null || true
}
trap cleanup EXIT

SRC="${SCRATCH}/src"
BIN="${SCRATCH}/symeraseme"
SURFACE="${SCRATCH}/command-surface.json"
mkdir -p "${SRC}"
git archive "${PINNED_COMMIT}" | tar -x -C "${SRC}"

# This test is deliberately untracked and exists only in the exported oracle.
# It calls the real unexported newRootCommand instead of inferring the surface
# from help text or source parsing.
cat > "${SRC}/cmd/symeraseme/oracle_surface_test.go" <<'EOF'
package main

import (
    "encoding/json"
    "os"
    "reflect"
    "runtime"
    "strings"
    "testing"

    "github.com/spf13/cobra"
    "github.com/spf13/pflag"
)

type oracleFlag struct {
    Order       int               `json:"order"`
    Name        string            `json:"name"`
    Shorthand   string            `json:"shorthand,omitempty"`
    Type        string            `json:"type"`
    Default     string            `json:"default"`
    NoOptValue  string            `json:"no_opt_value,omitempty"`
    Usage       string            `json:"usage"`
    Hidden      bool              `json:"hidden"`
    Deprecated  string            `json:"deprecated,omitempty"`
    Annotations map[string][]string `json:"annotations,omitempty"`
}

type oracleCommand struct {
    Order             int             `json:"order"`
    Path              []string        `json:"path"`
    Use               string          `json:"use"`
    Name              string          `json:"name"`
    Short             string          `json:"short,omitempty"`
    Long              string          `json:"long,omitempty"`
    Hidden            bool            `json:"hidden"`
    Deprecated        string          `json:"deprecated,omitempty"`
    Aliases           []string        `json:"aliases,omitempty"`
    SuggestFor        []string        `json:"suggest_for,omitempty"`
    ArgsValidator     string          `json:"args_validator,omitempty"`
    UsagePositionals  []string        `json:"usage_positionals,omitempty"`
    PersistentFlags   []oracleFlag    `json:"persistent_flags,omitempty"`
    LocalFlags        []oracleFlag    `json:"local_flags,omitempty"`
    InheritedFlags    []oracleFlag    `json:"inherited_flags,omitempty"`
    EffectiveFlags    []oracleFlag    `json:"effective_flags,omitempty"`
    Children          []oracleCommand `json:"children,omitempty"`
}

func oracleFlags(fs *pflag.FlagSet) []oracleFlag {
    if fs == nil {
        return nil
    }
    out := make([]oracleFlag, 0)
    fs.VisitAll(func(f *pflag.Flag) {
        item := oracleFlag{
            Name: f.Name, Shorthand: f.Shorthand, Type: f.Value.Type(),
            Default: f.DefValue, NoOptValue: f.NoOptDefVal, Usage: f.Usage,
            Hidden: f.Hidden, Deprecated: f.Deprecated, Annotations: f.Annotations,
        }
        item.Order = len(out)
        out = append(out, item)
    })
    return out
}

func validatorName(fn cobra.PositionalArgs) string {
    if fn == nil {
        return ""
    }
    value := reflect.ValueOf(fn)
    if !value.IsValid() || value.Kind() != reflect.Func {
        return "unknown"
    }
    if f := runtime.FuncForPC(value.Pointer()); f != nil {
        return f.Name()
    }
    return "unknown"
}

func positionals(use string) []string {
    words := strings.Fields(use)
    if len(words) < 2 {
        return nil
    }
    return words[1:]
}

func commandMetadata(cmd *cobra.Command, path []string, order int) oracleCommand {
    children := cmd.Commands()
    childMetadata := make([]oracleCommand, 0, len(children))
    for i, child := range children {
        childPath := append(append([]string(nil), path...), child.Name())
        childMetadata = append(childMetadata, commandMetadata(child, childPath, i))
    }
    return oracleCommand{
        Order: order, Path: path, Use: cmd.Use, Name: cmd.Name(), Short: cmd.Short,
        Long: cmd.Long, Hidden: cmd.Hidden, Deprecated: cmd.Deprecated,
        Aliases: append([]string(nil), cmd.Aliases...), SuggestFor: append([]string(nil), cmd.SuggestFor...),
        ArgsValidator: validatorName(cmd.Args), UsagePositionals: positionals(cmd.Use),
        PersistentFlags: oracleFlags(cmd.PersistentFlags()), LocalFlags: oracleFlags(cmd.LocalNonPersistentFlags()),
        InheritedFlags: oracleFlags(cmd.InheritedFlags()), EffectiveFlags: oracleFlags(cmd.Flags()),
        Children: childMetadata,
    }
}

func TestOracleSurface(t *testing.T) {
    output := os.Getenv("ORACLE_SURFACE_OUTPUT")
    if output == "" {
        t.Fatal("ORACLE_SURFACE_OUTPUT is required")
    }
    root := newRootCommand()
    document := struct {
        Commit string          `json:"commit"`
        Root   oracleCommand   `json:"root"`
    }{Commit: os.Getenv("ORACLE_COMMIT"), Root: commandMetadata(root, nil, 0)}
    data, err := json.MarshalIndent(document, "", "  ")
    if err != nil {
        t.Fatal(err)
    }
    data = append(data, '\n')
    if err := os.WriteFile(output, data, 0o600); err != nil {
        t.Fatal(err)
    }
}
EOF

(
    cd "${SRC}"
    GOWORK=off CGO_ENABLED=0 go build -trimpath -buildvcs=false -o "${BIN}" ./cmd/symeraseme
    ORACLE_SURFACE_OUTPUT="${SURFACE}" ORACLE_COMMIT="${PINNED_COMMIT}" \
        GOWORK=off CGO_ENABLED=0 go test -count=1 ./cmd/symeraseme -run '^TestOracleSurface$'
)

mkdir -p "${OUT_DIR}/cases/cli" "${OUT_DIR}/cases/mcp" "${OUT_DIR}/cases/http" \
    "${OUT_DIR}/cases/filesystem" "${OUT_DIR}/fixtures"

# The Python part is a black-box runner: it launches only the pinned binary,
# captures bytes before decoding, and writes no timestamp or host identity.
python3 - "${SURFACE}" "${BIN}" "${OUT_DIR}" "${RUNTIME_ROOT}" "${PINNED_COMMIT}" <<'PYEOF'
import base64
import hashlib
import http.client
import json
import os
import pathlib
import re
import shutil
import signal
import socket
import subprocess
import sys
import time

surface_path, binary, out_dir, runtime_root, commit = sys.argv[1:]
out = pathlib.Path(out_dir)
root = pathlib.Path(runtime_root)
source_binary = pathlib.Path(binary)
shutil.rmtree(root, ignore_errors=True)
root.mkdir(parents=True)
(root / "bin").mkdir(parents=True)
fixed_binary = root / "bin" / "symeraseme"
shutil.copy2(source_binary, fixed_binary)
fixed_binary.chmod(0o700)
binary = str(fixed_binary)

with open(surface_path, encoding="utf-8") as fh:
    surface = json.load(fh)

# Do not inherit application credentials, profile paths, or executable search
# paths. The pinned binary is launched by absolute path; an empty private PATH
# prevents optional runtime adapters from discovering user-installed tools.
base_env = {
    "PATH": str(root / "empty-path"),
    "HOME": str(root / "home"),
    "XDG_CONFIG_HOME": str(root / "xdg-config"),
    "XDG_DATA_HOME": str(root / "xdg-data"),
    "XDG_CACHE_HOME": str(root / "xdg-cache"),
    "TMPDIR": str(root / "tmp"),
    "TZ": "UTC",
    "LANG": "C",
    "LC_ALL": "C",
    "PYTHONHASHSEED": "0",
    "SOURCE_DATE_EPOCH": "0",
    "SYMERASEME_DATA_DIR": str(root / "data"),
    "SYMERASEME_DB_DIR": str(root / "db"),
    "SYMERASEME_CONFIG_DIR": str(root / "config"),
    "SYMERASEME_ENCRYPT_DB": "0",
    "SYMERASEME_RESOURCES": "",
    "ANTHROPIC_API_KEY": "",
    "CAPSOLVER_API_KEY": "",
    "SYMERASEME_MASTER_KEY": "",
    "SYMERASEME_CONSENT": "",
    "SYMERASEME_CONSENT_FILE": "",
}
for key in ("HOME", "XDG_CONFIG_HOME", "XDG_DATA_HOME", "XDG_CACHE_HOME", "TMPDIR", "SYMERASEME_DATA_DIR", "SYMERASEME_DB_DIR", "SYMERASEME_CONFIG_DIR"):
    pathlib.Path(base_env[key]).mkdir(parents=True, exist_ok=True)
(root / "empty-path").mkdir(parents=True, exist_ok=True)
(root / "cwd").mkdir(parents=True, exist_ok=True)


def b64(data):
    return base64.b64encode(data).decode("ascii")


def run_process(argv, stdin=b"", case_name="case", timeout=20, env_extra=None, cwd=None):
    env = dict(base_env)
    if env_extra:
        env.update(env_extra)
    proc = subprocess.Popen(
        [binary, *argv], stdin=subprocess.PIPE, stdout=subprocess.PIPE,
        stderr=subprocess.PIPE, env=env, cwd=cwd or str(root / "cwd"),
        start_new_session=True,
    )
    try:
        stdout, stderr = proc.communicate(stdin, timeout=timeout)
    except subprocess.TimeoutExpired:
        os.killpg(proc.pid, signal.SIGKILL)
        stdout, stderr = proc.communicate()
        raise RuntimeError(f"timeout in {case_name}: argv={argv!r}")
    return {
        "argv": argv,
        "stdin_base64": b64(stdin),
        "exit_code": proc.returncode,
        "stdout_base64": b64(stdout),
        "stderr_base64": b64(stderr),
        "stdout_bytes": len(stdout),
        "stderr_bytes": len(stderr),
        "_stdout_raw": stdout,
        "_stderr_raw": stderr,
    }


def case_capture(case_id, category, argv, stdin=b"", expected=None, env_extra=None, normalizers=()):
    result = run_process(argv, stdin, case_id, env_extra=env_extra)
    # Normalizers run only after raw bytes have been captured. Each one is a
    # field-specific contract exception, never a general trim/sort operation.
    stdout = result.pop("_stdout_raw")
    result.pop("_stderr_raw")
    nondeterministic = []
    for pattern, replacement, path, reason, marker in normalizers:
        stdout, count = re.subn(pattern, replacement, stdout, count=1)
        if count != 1:
            raise RuntimeError(f"expected one nondeterministic field in {case_id}: {path}")
        nondeterministic.append({"path": path, "reason": reason, "replacement": marker})
    result["stdout_base64"] = b64(stdout)
    result["stdout_bytes"] = len(stdout)
    result.update({
        "id": case_id,
        "category": category,
        "expected_outcome": expected or "observed",
        "nondeterministic_fields": nondeterministic,
    })
    return result


def command_paths(node):
    yield node
    for child in node.get("children", []):
        yield from command_paths(child)


def shell_path(node):
    return node.get("path") or []

# CLI: command metadata is produced by the temporary Go test above. Help is
# captured for every command, including hidden commands; other cases exercise
# each parser surface without rewriting public bytes.
commands = list(command_paths(surface["root"]))
help_cases = []
for node in commands:
    path = shell_path(node)
    case_id = "help-" + ("root" if not path else "-".join(path))
    help_cases.append(case_capture(case_id, "help", path + ["--help"], expected="success"))
    if path:
        unknown_id = "unknown-flag-" + "-".join(path)
        help_cases.append(case_capture(unknown_id, "unknown_flag", path + ["--definitely-unknown"], expected="parse_failure"))
    if node.get("usage_positionals"):
        missing_id = "missing-argument-" + ("-".join(path) or "root")
        help_cases.append(case_capture(missing_id, "missing_argument", path, expected="parse_failure"))

success_argvs = [
    ("version", ["version"]),
    ("version-json", ["version", "--json"]),
    ("config-show", ["config", "show"]),
    ("config-show-json", ["config", "show", "--output", "json"]),
    ("registry-validate", ["registry", "validate"]),
    ("registry-list-json", ["registry", "list", "--output", "json"]),
    ("status-json", ["status", "--output", "json"]),
    ("dashboard-json", ["dashboard", "--output", "json"]),
    ("calendar-json", ["calendar", "--output", "json"]),
    ("requests-list-json", ["requests", "list", "--output", "json"]),
    ("manual-tasks-list-json", ["manual-tasks", "list", "--output", "json"]),
    ("grant-dry-run", ["grant", "--dry-run"]),
    ("completion-bash", ["completion", "bash"]),
    ("completion-zsh", ["completion", "zsh"]),
    ("completion-fish", ["completion", "fish"]),
    ("completion-powershell", ["completion", "powershell"]),
]
success_cases = []
for case_id, argv in success_argvs:
    normalizers = ()
    if case_id == "status-json":
        normalizers = ((rb'("as_of":")[0-9]{4}-[0-9]{2}-[0-9]{2}T[^"]+"', b'"as_of":"<TIMESTAMP>"', "stdout.as_of", "status uses current UTC time", "<TIMESTAMP>"),)
    elif case_id == "dashboard-json":
        normalizers = ((rb'("generated_at":")[0-9]{4}-[0-9]{2}-[0-9]{2}T[^"]+"', b'"generated_at":"<TIMESTAMP>"', "stdout.generated_at", "dashboard uses current UTC time", "<TIMESTAMP>"),)
    elif case_id == "calendar-json":
        normalizers = (
            (rb'("as_of":")[0-9]{4}-[0-9]{2}-[0-9]{2}T[^"]+"', b'"as_of":"<TIMESTAMP>"', "stdout.as_of", "calendar uses current UTC time", "<TIMESTAMP>"),
            (rb'("horizon_until":")[0-9]{4}-[0-9]{2}-[0-9]{2}T[^"]+"', b'"horizon_until":"<TIMESTAMP>"', "stdout.horizon_until", "calendar derives horizon from current UTC time", "<TIMESTAMP>"),
        )
    result = case_capture(case_id, "success", argv, expected="success", normalizers=normalizers)
    if result["exit_code"] != 0:
        raise RuntimeError(f"designated CLI success case failed: {case_id}: {result}")
    success_cases.append(result)

cli_document = {
    "schema": "symeraseme.go-oracle.cli.v1",
    "commit": commit,
    "environment": {"timezone": "UTC", "locale": "C", "credentials": "cleared", "home": "isolated"},
    "surface_file": "surface.json",
    "cases": help_cases + success_cases,
}
write_json = lambda path, value: path.write_text(json.dumps(value, indent=2, ensure_ascii=False) + "\n", encoding="utf-8")
write_json(out / "cases" / "cli" / "surface.json", surface)
write_json(out / "cases" / "cli" / "behavior.json", cli_document)

# MCP stdio: each transcript is a fresh process with a fresh isolated tree.
mcp_inputs = [
    ("initialize", b'{"jsonrpc":"2.0","id":1,"method":"initialize","params":{}}\n'),
    ("tools-list", b'{"jsonrpc":"2.0","id":2,"method":"tools/list","params":{}}\n'),
    ("tools-call-status", b'{"jsonrpc":"2.0","id":3,"method":"tools/call","params":{"name":"status","arguments":{}}}\n'),
    ("batch-and-notification", b'[{"jsonrpc":"2.0","id":4,"method":"tools/list","params":{}},{"jsonrpc":"2.0","method":"tools/list","params":{}}]\n'),
    ("notification", b'{"jsonrpc":"2.0","method":"tools/list","params":{}}\n'),
    ("legacy-list-tools", b'{"jsonrpc":"2.0","id":"legacy","method":"list_tools","params":{}}\n'),
    ("unknown-method", b'{"jsonrpc":"2.0","id":5,"method":"does-not-exist","params":{}}\n'),
    ("invalid-params", b'{"jsonrpc":"2.0","id":6,"method":"tools/list","params":[]}\n'),
    ("unknown-tool", b'{"jsonrpc":"2.0","id":7,"method":"tools/call","params":{"name":"does_not_exist","arguments":{}}}\n'),
    ("missing-tool-name", b'{"jsonrpc":"2.0","id":8,"method":"tools/call","params":{"arguments":{}}}\n'),
    ("empty-batch", b'[]\n'),
    ("malformed-json", b'{\n'),
]
mcp_cases = []
for case_id, payload in mcp_inputs:
    case_root = root / "mcp" / case_id
    if case_root.exists():
        shutil.rmtree(case_root)
    case_root.mkdir(parents=True)
    env_extra = {
        "HOME": str(case_root / "home"),
        "XDG_CONFIG_HOME": str(case_root / "xdg-config"),
        "XDG_DATA_HOME": str(case_root / "xdg-data"),
        "XDG_CACHE_HOME": str(case_root / "xdg-cache"),
        "TMPDIR": str(case_root / "tmp"),
        "SYMERASEME_DATA_DIR": str(case_root / "data"),
        "SYMERASEME_DB_DIR": str(case_root / "db"),
        "SYMERASEME_CONFIG_DIR": str(case_root / "config"),
    }
    for key in ("HOME", "XDG_CONFIG_HOME", "XDG_DATA_HOME", "XDG_CACHE_HOME", "TMPDIR", "SYMERASEME_DATA_DIR", "SYMERASEME_DB_DIR", "SYMERASEME_CONFIG_DIR"):
        pathlib.Path(env_extra[key]).mkdir(parents=True, exist_ok=True)
    result = run_process(["mcp", "--stdio"], payload, case_id, env_extra=env_extra, cwd=str(case_root))
    result.pop("_stdout_raw")
    result.pop("_stderr_raw")
    result.update({
        "schema": "symeraseme.go-oracle.mcp.v1",
        "commit": commit,
        "id": case_id,
        "input_kind": "jsonl",
        "expected_outcome": "observed",
        "nondeterministic_fields": [],
    })
    mcp_cases.append(result)
# The matrix freezes a valid tools/call request for every one of the 26
# catalogue entries. Backend failures are still valuable oracle responses, but
# each request satisfies the published input schema and runs in isolation.
tools_frame = json.loads(base64.b64decode(next(c for c in mcp_cases if c["id"] == "tools-list")["stdout_base64"]))

def valid_tool_arguments(tool, case_root):
    schema = tool.get("inputSchema", {})
    properties = schema.get("properties", {})
    required = set(schema.get("required", []))
    args = {}
    for name, spec in properties.items():
        if name not in required:
            continue
        kind = spec.get("type")
        if name == "path":
            path = case_root / "redact-input.txt"
            path.write_text("Alice Example <alice@example.invalid>\n", encoding="utf-8")
            args[name] = str(path)
        elif name == "campaign_id":
            args[name] = "oracle-campaign"
        elif name == "broker_id":
            args[name] = "oracle-broker"
        elif name == "host":
            args[name] = "127.0.0.1"
        elif name == "port":
            args[name] = 1
        elif name == "username":
            args[name] = "oracle@example.invalid"
        elif name == "since_days":
            args[name] = 1
        elif name == "request_id" or name == "task_id":
            args[name] = 1
        elif kind == "string":
            args[name] = "oracle"
        elif kind == "integer":
            args[name] = 1
        elif kind == "boolean":
            args[name] = False
        elif kind == "array":
            args[name] = []
        else:
            raise RuntimeError(f"no deterministic fixture value for {tool['name']}.{name}")
    # Destructive operations have an explicit dry-run escape hatch.
    if "dry_run" in properties:
        args["dry_run"] = True
    return args

def normalize_mcp_timestamps(raw, tool_name):
    fields = []
    if tool_name == "get_dashboard_data":
        fields = [("generated_at", "result.content[0].text.generated_at")]
    elif tool_name == "get_calendar":
        fields = [("as_of", "result.content[0].text.as_of"), ("horizon_until", "result.content[0].text.horizon_until")]
    nondeterministic = []
    for field, path in fields:
        prefix = field.encode("ascii") + b'\\":\\"'
        suffix = b'\\"'
        pattern = re.escape(prefix) + rb'[0-9]{4}-[0-9]{2}-[0-9]{2}T[^"]+' + re.escape(suffix)
        replacement = prefix + b"<TIMESTAMP>" + suffix
        raw, count = re.subn(pattern, replacement, raw, count=1)
        if count != 1:
            raise RuntimeError(f"expected one MCP timestamp in {tool_name}: {path}")
        nondeterministic.append({"path": path, "reason": "tool result uses current UTC time", "replacement": "<TIMESTAMP>"})
    if tool_name in ("generate_dashboard", "generate_report"):
        pattern = rb'20[0-9]{2}-[0-9]{2}-[0-9]{2} [0-9]{2}:[0-9]{2} UTC'
        expected = 2 if tool_name == "generate_report" else 1
        raw, count = re.subn(pattern, b"<TIMESTAMP>", raw, count=expected)
        if count != expected:
            raise RuntimeError(f"expected {expected} rendered timestamp(s) in {tool_name}, got {count}")
        nondeterministic.append({"path": "result.content[0].text.generated_html.timestamps", "reason": "rendered report uses current UTC time", "replacement": "<TIMESTAMP>"})
    return raw, nondeterministic

for tool in tools_frame["result"]["tools"]:
    case_id = "tool-call-" + tool["name"]
    case_root = root / "mcp-valid" / tool["name"]
    case_root.mkdir(parents=True, exist_ok=True)
    args = valid_tool_arguments(tool, case_root)
    payload = json.dumps({"jsonrpc": "2.0", "id": tool["name"], "method": "tools/call", "params": {"name": tool["name"], "arguments": args}}, separators=(",", ":"), ensure_ascii=False).encode() + b"\n"
    env_extra = {
        "HOME": str(case_root / "home"),
        "XDG_CONFIG_HOME": str(case_root / "xdg-config"),
        "XDG_DATA_HOME": str(case_root / "xdg-data"),
        "XDG_CACHE_HOME": str(case_root / "xdg-cache"),
        "TMPDIR": str(case_root / "tmp"),
        "SYMERASEME_DATA_DIR": str(case_root / "data"),
        "SYMERASEME_DB_DIR": str(case_root / "db"),
        "SYMERASEME_CONFIG_DIR": str(case_root / "config"),
    }
    for key in ("HOME", "XDG_CONFIG_HOME", "XDG_DATA_HOME", "XDG_CACHE_HOME", "TMPDIR", "SYMERASEME_DATA_DIR", "SYMERASEME_DB_DIR", "SYMERASEME_CONFIG_DIR"):
        pathlib.Path(env_extra[key]).mkdir(parents=True, exist_ok=True)
    result = run_process(["mcp", "--stdio"], payload, case_id, env_extra=env_extra, cwd=str(case_root))
    raw_stdout = result.pop("_stdout_raw")
    result.pop("_stderr_raw")
    normalized_stdout, nondeterministic = normalize_mcp_timestamps(raw_stdout, tool["name"])
    result["stdout_base64"] = b64(normalized_stdout)
    result["stdout_bytes"] = len(normalized_stdout)
    result.update({
        "schema": "symeraseme.go-oracle.mcp.v1",
        "commit": commit,
        "id": case_id,
        "input_kind": "valid_tools_call",
        "tool": tool["name"],
        "arguments": args,
        "expected_outcome": "observed",
        "nondeterministic_fields": nondeterministic,
    })
    mcp_cases.append(result)

(out / "cases" / "mcp" / "transcript.jsonl").write_text("".join(json.dumps(c, ensure_ascii=False, sort_keys=False) + "\n" for c in mcp_cases), encoding="utf-8")

# HTTP: run the real MCP HTTP server and use an ephemeral loopback port. The
# token and port are never serialized; their exact locations are declared.
def free_port():
    sock = socket.socket(socket.AF_INET, socket.SOCK_STREAM)
    sock.bind(("127.0.0.1", 0))
    port = sock.getsockname()[1]
    sock.close()
    return port

http_root = root / "http"
http_root.mkdir(parents=True)
http_env = dict(base_env)
for key in ("HOME", "XDG_CONFIG_HOME", "XDG_DATA_HOME", "XDG_CACHE_HOME", "TMPDIR", "SYMERASEME_DATA_DIR", "SYMERASEME_DB_DIR", "SYMERASEME_CONFIG_DIR"):
    http_env[key] = str(http_root / pathlib.Path(base_env[key]).name)
    pathlib.Path(http_env[key]).mkdir(parents=True, exist_ok=True)
port = free_port()
server = subprocess.Popen(
    [binary, "mcp", "--host", "127.0.0.1", "--port", str(port)],
    stdin=subprocess.DEVNULL, stdout=subprocess.PIPE, stderr=subprocess.PIPE,
    env=http_env, cwd=str(http_root), start_new_session=True,
)
try:
    deadline = time.time() + 10
    while time.time() < deadline:
        try:
            with socket.create_connection(("127.0.0.1", port), timeout=0.2):
                break
        except OSError:
            if server.poll() is not None:
                raise RuntimeError(f"MCP HTTP server exited early: {server.returncode}")
            time.sleep(0.05)
    else:
        raise RuntimeError("timed out waiting for MCP HTTP server")
    token_path = pathlib.Path(http_env["SYMERASEME_DATA_DIR"]) / "mcp_token"
    token = token_path.read_text(encoding="utf-8")
    if not token:
        raise RuntimeError("MCP HTTP server wrote an empty token")

    request_body = b'{"jsonrpc":"2.0","id":1,"method":"initialize","params":{}}'
    cases = [
        ("get-method", "GET", [], b"", "method_error"),
        ("missing-auth", "POST", [("Content-Type", "application/json")], request_body, "auth_error"),
        ("wrong-auth", "POST", [("Content-Type", "application/json"), ("Authorization", "Bearer wrong")], request_body, "auth_error"),
        ("malformed-scheme", "POST", [("Content-Type", "application/json"), ("Authorization", "Basic x")], request_body, "auth_error"),
        ("duplicate-auth", "POST", [("Content-Type", "application/json"), ("Authorization", "Bearer " + token), ("Authorization", "Bearer " + token)], request_body, "auth_error"),
        ("bad-origin", "POST", [("Content-Type", "application/json"), ("Authorization", "Bearer " + token), ("Origin", "https://evil.example")], request_body, "origin_error"),
        ("valid-initialize", "POST", [("Content-Type", "application/json"), ("Authorization", "Bearer " + token), ("Origin", "http://localhost:3000")], request_body, "success"),
        ("valid-no-origin", "POST", [("Content-Type", "application/json"), ("Authorization", "Bearer " + token)], request_body, "success"),
        ("malformed-json", "POST", [("Content-Type", "application/json"), ("Authorization", "Bearer " + token)], b"{", "parse_error"),
        ("notification", "POST", [("Content-Type", "application/json"), ("Authorization", "Bearer " + token)], b'{"jsonrpc":"2.0","method":"tools/list","params":{}}', "notification"),
        ("batch-and-notification", "POST", [("Content-Type", "application/json"), ("Authorization", "Bearer " + token)], b'[{"jsonrpc":"2.0","id":2,"method":"tools/list","params":{}},{"jsonrpc":"2.0","method":"tools/list","params":{}}]', "success"),
        ("oversized-body", "POST", [("Content-Type", "application/json"), ("Authorization", "Bearer " + token)], b"x" * (5 * 1024 * 1024 + 1), "body_too_large"),
    ]
    http_cases = []
    for case_id, method, headers, body, outcome in cases:
        conn = http.client.HTTPConnection("127.0.0.1", port, timeout=10)
        conn.putrequest(method, "/", skip_host=True, skip_accept_encoding=True)
        for key, value in headers:
            conn.putheader(key, value)
        conn.putheader("Host", "127.0.0.1")
        conn.putheader("Content-Length", str(len(body)))
        if len(body) > 5 * 1024 * 1024:
            # ContentLength is rejected before the handler reads the body. Send
            # only the headers so the client does not race the server's 413.
            conn.endheaders()
        else:
            conn.endheaders(body)
        response = conn.getresponse()
        response_body = response.read()
        response_headers = response.getheaders()
        conn.close()
        public_headers = []
        public_request_headers = []
        for key, value in headers:
            if key.lower() == "authorization" and value == "Bearer " + token:
                value = "Bearer <MCP_TOKEN>"
            public_request_headers.append([key, value])
        for key, value in response_headers:
            if key.lower() == "date":
                value = "<HTTP_DATE>"
            public_headers.append([key, value])
        if len(body) > 1024 * 1024:
            request_payload = {
                "encoding": "literal_byte",
                "byte": "x",
                "size_bytes": len(body),
                "sha256": hashlib.sha256(body).hexdigest(),
            }
        else:
            request_payload = {"encoding": "base64", "data": b64(body), "size_bytes": len(body)}
        http_cases.append({
            "schema": "symeraseme.go-oracle.http.v1",
            "id": case_id,
            "request": {"method": method, "path": "/", "headers": public_request_headers, "body": request_payload},
            "response": {"status": response.status, "reason": response.reason, "headers": public_headers, "body_base64": b64(response_body), "body_bytes": len(response_body)},
            "expected_outcome": outcome,
            "nondeterministic_fields": [
                {"path": "request.headers.Authorization", "reason": "server-issued crypto/rand token", "replacement": "Bearer <MCP_TOKEN>"},
                {"path": "response.headers.Date", "reason": "net/http server wall clock", "replacement": "<HTTP_DATE>"},
                {"path": "server.port", "reason": "ephemeral local listener", "replacement": "<PORT>"},
            ],
        })
finally:
    if server.poll() is None:
        os.killpg(server.pid, signal.SIGTERM)
    try:
        server_stdout, server_stderr = server.communicate(timeout=8)
    except subprocess.TimeoutExpired:
        os.killpg(server.pid, signal.SIGKILL)
        server_stdout, server_stderr = server.communicate()
        raise RuntimeError("MCP HTTP server did not honor graceful shutdown")
    if server.returncode != 0:
        raise RuntimeError(f"MCP HTTP server exit={server.returncode}, stderr={server_stderr!r}")

write_json(out / "cases" / "http" / "transcript.json", {
    "schema": "symeraseme.go-oracle.http.v1",
    "commit": commit,
    "server": {"command": ["mcp", "--host", "127.0.0.1", "--port", "<PORT>"], "bind": "127.0.0.1", "port": "<PORT>", "auth_token": "<MCP_TOKEN>"},
    "cases": http_cases,
})

# Filesystem manifests are intentionally narrow: type, mode, size, and stable
# names are captured. Random token content is declared, not hashed or leaked.
def manifest(path):
    entries = []
    if not path.exists():
        return entries
    for item in sorted(path.rglob("*")):
        rel = item.relative_to(path).as_posix()
        st = item.lstat()
        record = {"path": rel, "mode": oct(st.st_mode & 0o777), "size_bytes": st.st_size}
        if item.is_symlink():
            record["type"] = "symlink"
            record["target"] = os.readlink(item)
        elif item.is_dir():
            record["type"] = "directory"
        elif item.is_file():
            record["type"] = "file"
            if item.name == "mcp_token":
                record["content"] = {"nondeterministic": True, "reason": "crypto/rand token; secret intentionally not captured"}
            else:
                record["sha256"] = hashlib.sha256(item.read_bytes()).hexdigest()
        else:
            record["type"] = "other"
        entries.append(record)
    return entries

filesystem_cases = [
    {
        "schema": "symeraseme.go-oracle.filesystem.v1",
        "id": "mcp-startup-token",
        "operation": ["mcp", "--host", "127.0.0.1", "--port", "<PORT>"],
        "roots": {"home": "isolated", "xdg_config": "isolated", "xdg_data": "isolated", "tmpdir": "isolated"},
        "manifest_roots": {
            "home": str(http_env["HOME"]),
            "xdg_config": str(http_env["XDG_CONFIG_HOME"]),
            "xdg_data": str(http_env["XDG_DATA_HOME"]),
            "tmpdir": str(http_env["TMPDIR"]),
            "data_dir": str(http_env["SYMERASEME_DATA_DIR"]),
            "db_dir": str(http_env["SYMERASEME_DB_DIR"]),
        },
        "manifests": {
            "home": manifest(pathlib.Path(http_env["HOME"])),
            "xdg_config": manifest(pathlib.Path(http_env["XDG_CONFIG_HOME"])),
            "xdg_data": manifest(pathlib.Path(http_env["XDG_DATA_HOME"])),
            "tmpdir": manifest(pathlib.Path(http_env["TMPDIR"])),
            "data_dir": manifest(pathlib.Path(http_env["SYMERASEME_DATA_DIR"])),
            "db_dir": manifest(pathlib.Path(http_env["SYMERASEME_DB_DIR"])),
        },
        "nondeterministic_fields": [
            {"path": "manifests.data_dir[*].content", "reason": "mcp_token is generated from crypto/rand", "replacement": "declaration only"},
            {"path": "operation.port", "reason": "ephemeral local listener", "replacement": "<PORT>"},
        ],
    }
]
write_json(out / "cases" / "filesystem" / "manifests.json", {"schema": "symeraseme.go-oracle.filesystem.v1", "commit": commit, "cases": filesystem_cases})

fixture_readme = f'''# Go oracle fixtures

These fixtures are generated from corrected Go commit `{commit}` by
`scripts/generate-go-oracle-fixtures.sh`. The generator exports that commit with
`git archive`, injects one untracked `_test.go` beside the real `newRootCommand`,
and launches the resulting binary as a black box. No tracked Go source is
modified and no developer profile, keychain, database, or credential is read.

## Layout

- `../cases/cli/surface.json` — recursively serialized Cobra metadata. `root`
  and every child include canonical path, public ordering, hidden/deprecated
  status, aliases, positional `Use` forms, validator function, local,
  inherited, persistent, and effective flags with defaults and order.
- `../cases/cli/behavior.json` — raw CLI help for every command, plus parser
  unknown-flag/missing-argument cases and successful read-only/completion cases.
- `../cases/mcp/transcript.jsonl` — one fresh `mcp --stdio` process per raw
  JSON-RPC transcript, including 26 schema-valid `tools/call` requests (one
  per pinned tool), batch/notification/legacy/error cases. `stdout_base64`
  and `stderr_base64` preserve bytes; only explicitly listed current-time
  result fields use a marker.
- `../cases/http/transcript.json` — local loopback MCP HTTP method, strict
  bearer-auth, origin, malformed-body, notification, and 5 MiB ceiling cases.
- `../cases/filesystem/manifests.json` — isolated HOME/XDG/TMPDIR side-effect
  manifest, including file modes and an explicit declaration for the random
  MCP token without recording its secret.

All records carry the oracle commit and a schema identifier. Fixture generation
uses UTC, locale `C`, a fixed dedicated `/tmp/symeraseme-go-oracle` runtime
root, an empty private executable search path, empty credential variables, and
no timestamps or host identity. The only
nondeterministic values are marked in `nondeterministic_fields`: the ephemeral
HTTP port, server-issued MCP token, HTTP `Date` header, and current-time fields
in the status/dashboard/calendar CLI and MCP results. The Go `net/http`
`Date` response header is replaced only at `response.headers.Date` with
`<HTTP_DATE>` and is likewise declared as nondeterministic. The oversized HTTP
request is represented by its exact byte length and SHA-256, not stored
verbatim. The token is represented by
`<MCP_TOKEN>` in the request transcript and its file content is never captured.

The generator is an executable drift gate:

```bash
scripts/generate-go-oracle-fixtures.sh
cp -R rust-tests/parity/cases /tmp/oracle-cases-first
scripts/generate-go-oracle-fixtures.sh
diff -ru /tmp/oracle-cases-first rust-tests/parity/cases
```

The second generation must be byte-identical. Any intended Go contract change
must first be corrected in Go, then regenerated from the new pinned commit;
fixtures must not be hand-edited.
'''
(out / "fixtures" / "README.md").write_text(fixture_readme, encoding="utf-8")

print(f"generated oracle fixtures from {commit}")
print(f"CLI commands={len(commands)} help_cases={len(help_cases)} success_cases={len(success_cases)}")
print(f"MCP cases={len(mcp_cases)} HTTP cases={len(http_cases)} filesystem_cases={len(filesystem_cases)}")
PYEOF

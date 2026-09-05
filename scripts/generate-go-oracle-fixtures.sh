#!/usr/bin/env bash
# Generate deterministic black-box Go oracle fixtures from the corrected commit.
# The temporary Cobra test is injected only into a clean git-archive export.
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "${SCRIPT_DIR}/.." && pwd)"
PINNED_COMMIT="bf53346eec234929bedf0314b99e3da85dbb991b"
OUT_DIR="${REPO_ROOT}/rust-tests/parity"

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

GO_TOOLCHAIN="go1.26.6"
GO_ROOT="$(env -i PATH="${PATH}" HOME="${HOME}" GOPROXY=off GOENV=off GOWORK=off GOTOOLCHAIN="${GO_TOOLCHAIN}" go env GOROOT)"
GO_MODULE_CACHE="$(env -i PATH="${PATH}" HOME="${HOME}" GOPROXY=off GOENV=off GOWORK=off GOTOOLCHAIN="${GO_TOOLCHAIN}" go env GOMODCACHE)"
PINNED_GO="${GO_ROOT}/bin/go"
if [[ ! -x "${PINNED_GO}" || ! -d "${GO_MODULE_CACHE}" ]]; then
    printf 'error: cached %s toolchain or pinned module cache is unavailable\n' "${GO_TOOLCHAIN}" >&2
    exit 1
fi
GO_VERSION="$(env -i PATH="${GO_ROOT}/bin:/usr/bin:/bin" HOME="${HOME}" GOPROXY=off GOENV=off GOWORK=off GOTOOLCHAIN="${GO_TOOLCHAIN}" "${PINNED_GO}" version)"
if [[ "${GO_VERSION}" != "go version ${GO_TOOLCHAIN} "* ]]; then
    printf 'error: resolved toolchain is not %s: %s\n' "${GO_TOOLCHAIN}" "${GO_VERSION}" >&2
    exit 1
fi

SCRATCH="$(mktemp -d "${TMPDIR:-/tmp}/symeraseme-oracle.XXXXXX")"
RUNTIME_ROOT="$(cd "$(mktemp -d "${TMPDIR:-/tmp}/symeraseme-go-oracle.XXXXXX")" && pwd -P)"
STAGE_DIR="$(mktemp -d "${OUT_DIR}/.oracle-staging.XXXXXX")"
BACKUP_DIR="${OUT_DIR}/.oracle-backup.$$"
SWAP_STARTED=0
CASES_BACKED_UP=0
FIXTURES_BACKED_UP=0
CASES_INSTALLED=0
FIXTURES_INSTALLED=0
cleanup() {
    rm -rf "${SCRATCH}" "${RUNTIME_ROOT}" "${STAGE_DIR}" 2>/dev/null || true
    if [[ "${SWAP_STARTED}" -eq 1 ]]; then
        if [[ "${CASES_INSTALLED}" -eq 1 ]]; then rm -rf "${OUT_DIR}/cases" 2>/dev/null || true; fi
        if [[ "${FIXTURES_INSTALLED}" -eq 1 ]]; then rm -rf "${OUT_DIR}/fixtures" 2>/dev/null || true; fi
        if [[ "${CASES_BACKED_UP}" -eq 1 ]]; then mv "${BACKUP_DIR}/cases" "${OUT_DIR}/cases" 2>/dev/null || true; fi
        if [[ "${FIXTURES_BACKED_UP}" -eq 1 ]]; then mv "${BACKUP_DIR}/fixtures" "${OUT_DIR}/fixtures" 2>/dev/null || true; fi
    fi
    rm -rf "${BACKUP_DIR}" 2>/dev/null || true
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
    // Cobra creates implicit help/version flags during initialization. Do this
    // explicitly before serializing metadata so the oracle freezes the same
    // surface that Execute() exposes to users.
    root.InitDefaultHelpFlag()
    root.InitDefaultHelpCmd()
    root.InitDefaultVersionFlag()
    document := struct {
        Schema string          `json:"schema"`
        Commit string          `json:"commit"`
        Root   oracleCommand   `json:"root"`
    }{Schema: "symeraseme.go-oracle.cli-surface.v1", Commit: os.Getenv("ORACLE_COMMIT"), Root: commandMetadata(root, nil, 0)}
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
    GO_BUILD_ENV=(env -i
        PATH="${GO_ROOT}/bin:/usr/bin:/bin"
        HOME="${SCRATCH}/go-home"
        GOCACHE="${SCRATCH}/go-cache"
        GOPATH="${SCRATCH}/go-path"
        GOMODCACHE="${GO_MODULE_CACHE}"
        TMPDIR="${SCRATCH}/go-tmp"
        GOPROXY=off GOENV=off GOWORK=off GOTOOLCHAIN="${GO_TOOLCHAIN}"
        CGO_ENABLED=0)
    mkdir -p "${SCRATCH}/go-home" "${SCRATCH}/go-cache" "${SCRATCH}/go-path" "${SCRATCH}/go-tmp"
    "${GO_BUILD_ENV[@]}" "${PINNED_GO}" build -trimpath -buildvcs=false -o "${BIN}" ./cmd/symeraseme
    ORACLE_BUILD_ENV=("${GO_BUILD_ENV[@]}" "ORACLE_SURFACE_OUTPUT=${SURFACE}" "ORACLE_COMMIT=${PINNED_COMMIT}")
    "${ORACLE_BUILD_ENV[@]}" "${PINNED_GO}" test -count=1 ./cmd/symeraseme -run '^TestOracleSurface$'
)

# Generate only in the same-filesystem staging tree. Existing cases and
# fixtures remain untouched until every subprocess and artifact succeeds.
mkdir -p "${STAGE_DIR}/cases/cli" "${STAGE_DIR}/cases/mcp" "${STAGE_DIR}/cases/http" \
    "${STAGE_DIR}/cases/filesystem" "${STAGE_DIR}/fixtures"

# The Python part is a black-box runner: it launches only the pinned binary,
# captures bytes before decoding, and writes no timestamp or host identity.
python3 - "${SURFACE}" "${BIN}" "${STAGE_DIR}" "${RUNTIME_ROOT}" "${PINNED_COMMIT}" "${GO_VERSION}" "${GO_ROOT}" "${GO_MODULE_CACHE}" <<'PYEOF'
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
import tempfile
import time

surface_path, binary, out_dir, runtime_root, commit, go_version, go_root, go_module_cache = sys.argv[1:]
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

# The generator records the exact pinned toolchain contract without exposing
# host-specific paths. The module cache is the only dependency source.
toolchain = {
    "go_version": go_version,
    "go_root": "<GO_ROOT>",
    "module_cache": "<PINNED_MODULE_CACHE>",
    "goproxy": "off", "goenv": "off", "gowork": "off",
    "gotoolchain": "go1.26.6",
    "cache": "isolated", "home": "isolated",
}
normalization_contract = {
    "exact_runtime_root": {
        "replacement": "<ORACLE_ROOT>",
        "scope": ["argv", "stdout", "stderr", "manifest_roots", "artifact_evidence"],
        "rule": "replace only the exact unique mktemp-owned runtime root",
    },
}
surface["toolchain"] = toolchain
surface["normalization"] = normalization_contract

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
    # Deterministic test-only key source; this prevents InitProfile from
    # consulting or mutating the host OS keychain.
    "SYMERASEME_IDENTITY_MASTER_KEY": "00112233445566778899aabbccddeeff00112233445566778899aabbccddeeff",
    "SYMERASEME_CONSENT": "",
    "SYMERASEME_CONSENT_FILE": "",
}
for key in ("HOME", "XDG_CONFIG_HOME", "XDG_DATA_HOME", "XDG_CACHE_HOME", "TMPDIR", "SYMERASEME_DATA_DIR", "SYMERASEME_DB_DIR", "SYMERASEME_CONFIG_DIR"):
    pathlib.Path(base_env[key]).mkdir(parents=True, exist_ok=True)
(root / "empty-path").mkdir(parents=True, exist_ok=True)
(root / "cwd").mkdir(parents=True, exist_ok=True)


def b64(data):
    return base64.b64encode(data).decode("ascii")


MAX_STDOUT_BYTES = 8 * 1024 * 1024
MAX_STDERR_BYTES = 2 * 1024 * 1024
capture_dir = root / "captures"
capture_dir.mkdir(parents=True, exist_ok=True)
root_text = str(root)
root_real_text = os.path.realpath(root_text)
root_alias_set = {root_text, root_real_text}
if root_text.startswith("/private/"):
    root_alias_set.add(root_text[len("/private"):])
else:
    root_alias_set.add("/private" + root_text)
root_aliases = sorted(root_alias_set, key=len, reverse=True)
root_bytes = tuple(alias.encode() for alias in root_aliases)


def normalize_root_bytes(data):
    # Deliberately replace only the exact generated root (including its macOS
    # symlink-resolved spelling); no broad temp/path scrubber is allowed.
    for alias in root_bytes:
        data = data.replace(alias, b"<ORACLE_ROOT>")
    return data


def normalize_root_value(value):
    if isinstance(value, str):
        for alias in root_aliases:
            value = value.replace(alias, "<ORACLE_ROOT>")
        return value
    if isinstance(value, list):
        return [normalize_root_value(item) for item in value]
    if isinstance(value, dict):
        return {key: normalize_root_value(item) for key, item in value.items()}
    return value


def bounded_read(path, limit, stream, case_name):
    size = path.stat().st_size
    if size > limit:
        raise RuntimeError(f"{stream} exceeded {limit} bytes in {case_name}: {size}")
    return path.read_bytes()


def start_process(argv, env, cwd, stdin=subprocess.DEVNULL):
    paths = []
    try:
        for stream in ("stdout", "stderr"):
            fd, name = tempfile.mkstemp(prefix=f"{stream}-", dir=capture_dir)
            os.close(fd)
            paths.append(pathlib.Path(name))
        with paths[0].open("wb") as stdout_file, paths[1].open("wb") as stderr_file:
            proc = subprocess.Popen(
                [binary, *argv], stdin=stdin, stdout=stdout_file,
                stderr=stderr_file, env=env, cwd=cwd, start_new_session=True,
            )
        return proc, paths
    except Exception:
        for path in paths:
            path.unlink(missing_ok=True)
        raise


def stop_process(proc, paths, case_name, terminate=False, timeout=8):
    try:
        if terminate and proc.poll() is None:
            os.killpg(proc.pid, signal.SIGTERM)
        try:
            proc.wait(timeout=timeout)
        except subprocess.TimeoutExpired:
            os.killpg(proc.pid, signal.SIGKILL)
            proc.wait()
        stdout = bounded_read(paths[0], MAX_STDOUT_BYTES, "stdout", case_name)
        stderr = bounded_read(paths[1], MAX_STDERR_BYTES, "stderr", case_name)
        return proc.returncode, stdout, stderr
    finally:
        for path in paths:
            path.unlink(missing_ok=True)


def run_process(argv, stdin=b"", case_name="case", timeout=20, env_extra=None, cwd=None):
    env = dict(base_env)
    if env_extra:
        env.update(env_extra)
    proc, paths = start_process(argv, env, cwd or str(root / "cwd"), stdin=subprocess.PIPE)
    try:
        proc.communicate(stdin, timeout=timeout)
    except subprocess.TimeoutExpired:
        stop_process(proc, paths, case_name, terminate=True)
        raise RuntimeError(f"timeout in {case_name}: argv={argv!r}")
    return_code, stdout, stderr = stop_process(proc, paths, case_name)
    return {
        "argv": argv,
        "stdin_base64": b64(stdin),
        "exit_code": return_code,
        "stdout_base64": b64(stdout),
        "stderr_base64": b64(stderr),
        "stdout_bytes": len(stdout),
        "stderr_bytes": len(stderr),
        "_stdout_raw": stdout,
        "_stderr_raw": stderr,
    }


def cli_case_environment(case_id):
    """Return a fresh HOME/XDG/data/db/TMPDIR/cwd for one CLI invocation."""
    case_root = root / "cli" / case_id
    shutil.rmtree(case_root, ignore_errors=True)
    case_root.mkdir(parents=True)
    env = dict(base_env)
    paths = {
        "HOME": case_root / "home",
        "XDG_CONFIG_HOME": case_root / "xdg-config",
        "XDG_DATA_HOME": case_root / "xdg-data",
        "XDG_CACHE_HOME": case_root / "xdg-cache",
        "TMPDIR": case_root / "tmp",
        "SYMERASEME_DATA_DIR": case_root / "data",
        "SYMERASEME_DB_DIR": case_root / "db",
        "SYMERASEME_CONFIG_DIR": case_root / "config",
    }
    for key, value in paths.items():
        value.mkdir(parents=True, exist_ok=True)
        env[key] = str(value)
    cwd = case_root / "cwd"
    cwd.mkdir()
    return env, cwd


def case_capture(case_id, category, argv, stdin=b"", expected=None, env_extra=None, normalizers=(), isolate_cli=True):
    if isolate_cli:
        isolated_env, isolated_cwd = cli_case_environment(case_id)
        if env_extra:
            isolated_env.update(env_extra)
        env_extra, cwd = isolated_env, str(isolated_cwd)
    else:
        cwd = None
    result = run_process(argv, stdin, case_id, env_extra=env_extra, cwd=cwd)
    # Normalizers run only after raw bytes have been captured. Each one is a
    # field-specific contract exception, never a general trim/sort operation.
    stdout = normalize_root_bytes(result.pop("_stdout_raw"))
    stderr = normalize_root_bytes(result.pop("_stderr_raw"))
    nondeterministic = [{"path": "argv/stdout/stderr", "reason": "only the exact unique runtime root is normalized", "replacement": "<ORACLE_ROOT>"}]
    for pattern, replacement, path, reason, marker in normalizers:
        stdout, count = re.subn(pattern, replacement, stdout, count=1)
        if count != 1:
            raise RuntimeError(f"expected one nondeterministic field in {case_id}: {path}")
        nondeterministic.append({"path": path, "reason": reason, "replacement": marker})
    result["argv"] = normalize_root_value(result["argv"])
    result["stdout_base64"] = b64(stdout)
    result["stdout_bytes"] = len(stdout)
    result["stderr_base64"] = b64(stderr)
    result["stderr_bytes"] = len(stderr)
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
    # Only ExactArgs is a parser-level required positional contract. Commands
    # using MaximumNArgs intentionally accept zero positionals and validate
    # their operation-specific input later; those are exercised below as
    # deterministic backend/error cases, never mislabeled parse failures.
    validator = node.get("args_validator", "")
    if node.get("usage_positionals") and "ExactArgs" in validator:
        missing_id = "missing-argument-" + ("-".join(path) or "root")
        help_cases.append(case_capture(missing_id, "missing_argument", path, expected="parse_failure"))

# Root implicit behaviors are part of the public CLI, even though --version
# and unknown-command are not represented as child commands in Cobra metadata.
root_behavior_cases = [
    case_capture("root-version", "root_version", ["--version"], expected="success"),
    case_capture("root-unknown-flag", "unknown_flag", ["--definitely-unknown"], expected="parse_failure"),
    case_capture("root-unknown-command", "unknown_command", ["definitely-not-a-command"], expected="parse_failure"),
]
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

# Every executable leaf gets one operational invocation. A non-zero result is
# retained as an explicitly classified deterministic backend/error behavior;
# only the ExactArgs cases above are parser missing-argument failures.
operational_argvs = {
    "auto-confirm": ["auto-confirm", "1", "--dry-run", "--output", "json"],
    "brokers/list": ["brokers", "list", "--output", "json"],
    "brokers/show": ["brokers", "show", "0ptimus-analytics-us", "--output", "json"],
    "calendar": ["calendar", "--output", "json"],
    "classify-reply": ["classify-reply", "1", "--save=false", "--output", "json"],
    "completion": ["completion", "bash"],
    "config/show": ["config", "show", "--output", "json"],
    "dashboard": ["dashboard", "--output", "json"],
    "events/show": ["events", "show", "1", "--output", "json"],
    "generate-dashboard": ["generate-dashboard"],
    "generate-rebuttal": ["generate-rebuttal", "1", "--save=false", "--output", "json"],
    "generate-report": ["generate-report", "--all-campaigns", "--format", "json", "--output", ""],
    "generate-scheduler": ["generate-scheduler", "--platform", "cron", "--dry-run"],
    "grant": ["grant", "--dry-run", "--output", "json"],
    "help": ["help"],
    "init-profile": ["--output", "json", "init-profile", "--full-name", "Oracle User", "--email", "oracle@example.invalid"],
    "manual-tasks/cleanup": ["manual-tasks", "cleanup", "--dry-run", "--output", "json"],
    "manual-tasks/complete": ["manual-tasks", "complete", "1", "--output", "json"],
    "manual-tasks/list": ["manual-tasks", "list", "--output", "json"],
    "manual-tasks/show": ["manual-tasks", "show", "1", "--output", "json"],
    "mcp": ["mcp", "--stdio"],
    "migrate": ["migrate", "--source", str(root / "cli" / "migrate" / "source"), "--destination", str(root / "cli" / "migrate" / "destination"), "--dry-run", "--json"],
    "plan/create": ["--output", "json", "plan", "create", "--campaign", "oracle-campaign", "--max", "1"],
    "plan/execute": ["--output", "json", "plan", "execute", "--campaign", "oracle-campaign", "--dry-run"],
    "plan/show": ["--output", "json", "plan", "show", "--campaign", "oracle-campaign"],
    "plan/status": ["--output", "json", "plan", "status"],
    "plan/tick": ["--output", "json", "plan", "tick", "--dry-run"],
    "poll-inbox": ["--output", "json", "poll-inbox", "--host", "127.0.0.1", "--port", "1", "--username", "oracle@example.invalid", "--since-days", "1", "--ssl=false"],
    "registry/list": ["registry", "list", "--output", "json"],
    "registry/validate": ["registry", "validate", "--output", "json"],
    "render-template": ["render-template", "laws/gdpr-art17.en.md.j2", "--broker-name", "Oracle Broker", "--broker-website", "https://example.invalid"],
    "requests/list": ["requests", "list", "--output", "json"],
    "review": ["review", str(root / "cli" / "review" / "missing.txt"), "--output", "json"],
    "run-web-form": ["run-web-form", "virtual-minds-eu", "--dry-run", "--output", "json"],
    "schedule/install": ["schedule", "install", "--platform", "cron", "--dry-run", "--output", "json"],
    "schedule/status": ["schedule", "status", "--platform", "cron", "--output", "json"],
    "schedule/uninstall": ["schedule", "uninstall", "--platform", "cron", "--output", "json"],
    "serve": ["serve", "--stdio"],
    "show-profile": ["show-profile", "--output", "json"],
    "status": ["status", "--output", "json"],
    "tick": ["tick", "--output", "json", "--dry-run"],
    "version": ["version", "--json"],
}
leaf_paths = [tuple(shell_path(node)) for node in commands if not node.get("children")]
missing_operations = sorted("/".join(path) for path in leaf_paths if "/".join(path) not in operational_argvs)
if missing_operations:
    raise RuntimeError(f"no operational oracle case for leaf commands: {missing_operations}")
operational_cases = []
for key in sorted(operational_argvs):
    argv = operational_argvs[key]
    normalizers = ()
    if key in ("status", "plan/status"):
        normalizers = ((rb'("as_of":")[0-9]{4}-[0-9]{2}-[0-9]{2}T[^"]+"', b'"as_of":"<TIMESTAMP>"', "stdout.as_of", "status uses current UTC time", "<TIMESTAMP>"),)
    elif key == "dashboard":
        normalizers = ((rb'("generated_at":")[0-9]{4}-[0-9]{2}-[0-9]{2}T[^"]+"', b'"generated_at":"<TIMESTAMP>"', "stdout.generated_at", "dashboard uses current UTC time", "<TIMESTAMP>"),)
    elif key == "calendar":
        normalizers = (
            (rb'("as_of":")[0-9]{4}-[0-9]{2}-[0-9]{2}T[^"]+"', b'"as_of":"<TIMESTAMP>"', "stdout.as_of", "calendar uses current UTC time", "<TIMESTAMP>"),
            (rb'("horizon_until":")[0-9]{4}-[0-9]{2}-[0-9]{2}T[^"]+"', b'"horizon_until":"<TIMESTAMP>"', "stdout.horizon_until", "calendar derives horizon from current UTC time", "<TIMESTAMP>"),
        )
    result = case_capture("operate-" + key.replace("/", "-"), "operational", argv, expected="operational", normalizers=normalizers)
    result["classification"] = "success" if result["exit_code"] == 0 else "deterministic_backend_error"
    result["expected_outcome"] = result["classification"]
    operational_cases.append(result)

cli_document = {
    "schema": "symeraseme.go-oracle.cli.v1",
    "commit": commit,
    "toolchain": toolchain,
    "normalization": normalization_contract,
    "environment": {"timezone": "UTC", "locale": "C", "credentials": "cleared", "home": "isolated", "keychain": "not_accessed", "path": "private_empty"},
    "surface_file": "surface.json",
    "cases": root_behavior_cases + help_cases + success_cases + operational_cases,
}
write_json = lambda path, value: path.write_text(json.dumps(value, indent=2, ensure_ascii=False) + "\n", encoding="utf-8")
write_json(out / "cases" / "cli" / "surface.json", surface)
write_json(out / "cases" / "cli" / "behavior.json", cli_document)

# MCP stdio: each transcript is a fresh process with a fresh isolated tree.
mcp_inputs = [
    ("initialize", b'{"jsonrpc":"2.0","id":1,"method":"initialize","params":{}}\n'),
    ("id-string", b'{"jsonrpc":"2.0","id":"string-id","method":"tools/list","params":{}}\n'),
    ("id-null", b'{"jsonrpc":"2.0","id":null,"method":"tools/list","params":{}}\n'),
    ("id-boolean-invalid", b'{"jsonrpc":"2.0","id":true,"method":"tools/list","params":{}}\n'),
    ("id-array-invalid", b'{"jsonrpc":"2.0","id":[1],"method":"tools/list","params":{}}\n'),
    ("tools-list", b'{"jsonrpc":"2.0","id":2,"method":"tools/list","params":{}}\n'),
    ("tools-list-null-params", b'{"jsonrpc":"2.0","id":2,"method":"tools/list","params":null}\n'),
    ("tools-call-status", b'{"jsonrpc":"2.0","id":3,"method":"tools/call","params":{"name":"status","arguments":{}}}\n'),
    ("tools-call-missing-arguments", b'{"jsonrpc":"2.0","id":9,"method":"tools/call","params":{"name":"plan_create"}}\n'),
    ("tools-call-null-arguments", b'{"jsonrpc":"2.0","id":10,"method":"tools/call","params":{"name":"plan_create","arguments":null}}\n'),
    ("tools-call-string-arguments", b'{"jsonrpc":"2.0","id":11,"method":"tools/call","params":{"name":"plan_create","arguments":"bad"}}\n'),
    ("batch-and-notification", b'[{"jsonrpc":"2.0","id":4,"method":"tools/list","params":{}},{"jsonrpc":"2.0","method":"tools/list","params":{}}]\n'),
    ("mixed-batch-errors", b'[{"jsonrpc":"2.0","id":12,"method":"tools/list","params":[]},{"jsonrpc":"2.0","id":13,"method":"does-not-exist","params":{}},{"jsonrpc":"2.0","method":"tools/list","params":{}}]\n'),
    ("notification", b'{"jsonrpc":"2.0","method":"tools/list","params":{}}\n'),
    ("legacy-list-tools", b'{"jsonrpc":"2.0","id":"legacy","method":"list_tools","params":{}}\n'),
    ("invalid-jsonrpc-version", b'{"jsonrpc":"1.0","id":14,"method":"tools/list","params":{}}\n'),
    ("unknown-method", b'{"jsonrpc":"2.0","id":5,"method":"does-not-exist","params":{}}\n'),
    ("invalid-params-array", b'{"jsonrpc":"2.0","id":6,"method":"tools/list","params":[]}\n'),
    ("invalid-params-scalar", b'{"jsonrpc":"2.0","id":15,"method":"tools/list","params":"bad"}\n'),
    ("unknown-tool", b'{"jsonrpc":"2.0","id":7,"method":"tools/call","params":{"name":"does_not_exist","arguments":{}}}\n'),
    ("missing-tool-name", b'{"jsonrpc":"2.0","id":8,"method":"tools/call","params":{"arguments":{}}}\n'),
    ("empty-batch", b'[]\n'),
    ("multiple-frames", b'{"jsonrpc":"2.0","id":16,"method":"tools/list","params":{}}\n{"jsonrpc":"2.0","id":17,"method":"initialize","params":{}}\n'),
    ("truncated-frame", b'{"jsonrpc":"2.0","id":18,"method":"tools/list","params":{\n'),
    ("graceful-shutdown", b""),
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
    raw_stdout = normalize_root_bytes(result.pop("_stdout_raw"))
    raw_stderr = normalize_root_bytes(result.pop("_stderr_raw"))
    result["stdout_base64"] = b64(raw_stdout)
    result["stdout_bytes"] = len(raw_stdout)
    result["stderr_base64"] = b64(raw_stderr)
    result["stderr_bytes"] = len(raw_stderr)
    result.update({
        "schema": "symeraseme.go-oracle.mcp.v1",
        "commit": commit,
        "toolchain": toolchain,
        "normalization": normalization_contract,
        "id": case_id,
        "input_kind": "jsonl",
        "expected_outcome": {
            "graceful-shutdown": "graceful_shutdown",
            "truncated-frame": "protocol_error",
            "malformed-json": "protocol_error",
        }.get(case_id, "protocol_response"),
        "classification": "graceful_shutdown" if case_id == "graceful-shutdown" else ("protocol_error" if case_id in ("truncated-frame", "malformed-json") else "protocol_response"),
        "nondeterministic_fields": [{"path": "argv/stdout/stderr", "reason": "only the exact unique runtime root is normalized", "replacement": "<ORACLE_ROOT>"}],
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
    nondeterministic = [{"path": "stdout", "reason": "only the exact unique runtime root is normalized", "replacement": "<ORACLE_ROOT>"}]
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
    raw_stdout = normalize_root_bytes(result.pop("_stdout_raw"))
    raw_stderr = normalize_root_bytes(result.pop("_stderr_raw"))
    normalized_stdout, nondeterministic = normalize_mcp_timestamps(raw_stdout, tool["name"])
    result["stdin_base64"] = b64(normalize_root_bytes(payload))
    result["stdout_base64"] = b64(normalized_stdout)
    result["stdout_bytes"] = len(normalized_stdout)
    result["stderr_base64"] = b64(raw_stderr)
    result["stderr_bytes"] = len(raw_stderr)
    result.update({
        "schema": "symeraseme.go-oracle.mcp.v1",
        "commit": commit,
        "toolchain": toolchain,
        "normalization": normalization_contract,
        "id": case_id,
        "input_kind": "valid_tools_call",
        "tool": tool["name"],
        "arguments": normalize_root_value(args),
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
# Exercise the bind policy itself, including a permitted remote bind on a
# local-only helper process. Both probes use synthetic roots and are terminated
# explicitly so no server survives generation.
startup_cases = []
def http_startup_probe(case_id, host, allow_remote):
    case_root = root / "http-policy" / case_id
    shutil.rmtree(case_root, ignore_errors=True)
    case_root.mkdir(parents=True)
    env = dict(base_env)
    for key in ("HOME", "XDG_CONFIG_HOME", "XDG_DATA_HOME", "XDG_CACHE_HOME", "TMPDIR", "SYMERASEME_DATA_DIR", "SYMERASEME_DB_DIR", "SYMERASEME_CONFIG_DIR"):
        env[key] = str(case_root / pathlib.Path(base_env[key]).name)
        pathlib.Path(env[key]).mkdir(parents=True, exist_ok=True)
    probe_port = free_port()
    proc, capture_paths = start_process(
        ["mcp", "--host", host, "--port", str(probe_port)] + (["--allow-remote"] if allow_remote else []),
        env, str(case_root),
    )
    listening = False
    try:
        deadline = time.time() + 10
        while time.time() < deadline:
            try:
                with socket.create_connection(("127.0.0.1", probe_port), timeout=0.2):
                    listening = True
                    break
            except OSError:
                if proc.poll() is not None:
                    break
                time.sleep(0.05)
    finally:
        return_code, stdout, stderr = stop_process(
            proc, capture_paths, case_id, terminate=proc.poll() is None,
        )
    stdout = normalize_root_bytes(stdout)
    stderr = normalize_root_bytes(stderr)
    stderr, port_count = re.subn(rb":" + str(probe_port).encode() + rb":", b":<PORT>:", stderr, count=1)
    expected_port_count = 1 if allow_remote else 0
    if port_count != expected_port_count:
        raise RuntimeError(f"expected {expected_port_count} startup port occurrence(s) in {case_id} stderr, got {port_count}")
    if allow_remote and b"refusing non-loopback" in stderr:
        raise RuntimeError("allow-remote probe was rejected by policy before OS bind")
    if allow_remote and (listening or return_code == 0):
        raise RuntimeError("TEST-NET allow-remote probe unexpectedly listened")
    startup_cases.append({
        "schema": "symeraseme.go-oracle.http.v1", "id": case_id,
        "phase": "startup_policy", "request": {"host": host, "allow_remote": allow_remote, "port": "<PORT>"},
        "process": {"argv": normalize_root_value(["mcp", "--host", host, "--port", "<PORT>"] + (["--allow-remote"] if allow_remote else [])), "exit_code": return_code, "stdout_base64": b64(stdout), "stderr_base64": b64(stderr), "stdout_bytes": len(stdout), "stderr_bytes": len(stderr)},
        "expected_outcome": "remote_bind_allowed_os_bind_failed" if allow_remote else "remote_bind_rejected",
        "observed_listening": listening,
        "nondeterministic_fields": [
            {"path": "request.port", "reason": "ephemeral local listener", "replacement": "<PORT>"},
            {"path": "process.argv/stdout/stderr", "reason": "only the exact unique runtime root is normalized", "replacement": "<ORACLE_ROOT>"},
        ],
    })
http_startup_probe("remote-bind-rejected", "192.0.2.1", False)
http_startup_probe("remote-bind-allowed-os-failure", "192.0.2.1", True)

server, server_capture_paths = start_process(
    ["mcp", "--host", "127.0.0.1", "--port", str(port)],
    http_env, str(http_root),
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
        ("get-method", "GET", [], b"", "method_error", "127.0.0.1"),
        ("missing-auth", "POST", [("Content-Type", "application/json")], request_body, "auth_error", "127.0.0.1"),
        ("wrong-auth", "POST", [("Content-Type", "application/json"), ("Authorization", "Bearer wrong")], request_body, "auth_error", "127.0.0.1"),
        ("malformed-scheme", "POST", [("Content-Type", "application/json"), ("Authorization", "Basic x")], request_body, "auth_error", "127.0.0.1"),
        ("duplicate-auth", "POST", [("Content-Type", "application/json"), ("Authorization", "Bearer " + token), ("Authorization", "Bearer " + token)], request_body, "auth_error", "127.0.0.1"),
        ("bad-origin", "POST", [("Content-Type", "application/json"), ("Authorization", "Bearer " + token), ("Origin", "https://evil.example")], request_body, "origin_error", "127.0.0.1"),
        ("missing-content-type", "POST", [("Authorization", "Bearer " + token)], request_body, "success", "127.0.0.1"),
        ("wrong-content-type", "POST", [("Content-Type", "text/plain"), ("Authorization", "Bearer " + token)], request_body, "success", "127.0.0.1"),
        ("host-localhost", "POST", [("Content-Type", "application/json"), ("Authorization", "Bearer " + token)], request_body, "success", "localhost"),
        ("host-untrusted", "POST", [("Content-Type", "application/json"), ("Authorization", "Bearer " + token)], request_body, "success", "evil.example"),
        ("valid-initialize", "POST", [("Content-Type", "application/json"), ("Authorization", "Bearer " + token), ("Origin", "http://localhost:3000")], request_body, "success", "127.0.0.1"),
        ("valid-no-origin", "POST", [("Content-Type", "application/json"), ("Authorization", "Bearer " + token)], request_body, "success", "127.0.0.1"),
        ("malformed-json", "POST", [("Content-Type", "application/json"), ("Authorization", "Bearer " + token)], b"{", "parse_error", "127.0.0.1"),
        ("notification", "POST", [("Content-Type", "application/json"), ("Authorization", "Bearer " + token)], b'{"jsonrpc":"2.0","method":"tools/list","params":{}}', "notification", "127.0.0.1"),
        ("batch-and-notification", "POST", [("Content-Type", "application/json"), ("Authorization", "Bearer " + token)], b'[{"jsonrpc":"2.0","id":2,"method":"tools/list","params":{}},{"jsonrpc":"2.0","method":"tools/list","params":{}}]', "success", "127.0.0.1"),
        ("body-at-limit", "POST", [("Content-Type", "application/json"), ("Authorization", "Bearer " + token)], b"x" * (5 * 1024 * 1024), "parse_error", "127.0.0.1"),
        ("oversized-body", "POST", [("Content-Type", "application/json"), ("Authorization", "Bearer " + token)], b"x" * (5 * 1024 * 1024 + 1), "body_too_large", "127.0.0.1"),
    ]
    http_cases = []
    for case_id, method, headers, body, outcome, host_header in cases:
        conn = http.client.HTTPConnection("127.0.0.1", port, timeout=10)
        conn.putrequest(method, "/", skip_host=True, skip_accept_encoding=True)
        for key, value in headers:
            conn.putheader(key, value)
        conn.putheader("Host", host_header)
        conn.putheader("Content-Length", str(len(body)))
        if len(body) > 5 * 1024 * 1024:
            # ContentLength is rejected before the handler reads the body. Send
            # only the headers so the client does not race the server's 413.
            conn.endheaders()
        else:
            conn.endheaders(body)
        response = conn.getresponse()
        response_body = normalize_root_bytes(response.read())
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
    server_returncode, server_stdout, server_stderr = stop_process(
        server, server_capture_paths, "http-server", terminate=server.poll() is None,
    )
    if server_returncode != 0:
        raise RuntimeError(f"MCP HTTP server exit={server_returncode}, stderr={server_stderr!r}")

write_json(out / "cases" / "http" / "transcript.json", {
    "schema": "symeraseme.go-oracle.http.v1",
    "commit": commit,
    "toolchain": toolchain,
    "normalization": normalization_contract,
    "server": {"command": ["mcp", "--host", "127.0.0.1", "--port", "<PORT>"], "bind": "127.0.0.1", "port": "<PORT>", "auth_token": "<MCP_TOKEN>"},
    "cases": startup_cases + http_cases,
})

# Filesystem manifests are intentionally narrow: type, mode, size, and stable
# names are captured. Random token content is declared, not hashed or leaked.
def manifest(path, nondeterministic_prefixes=()):
    entries = []
    if not path.exists():
        return entries
    prefixes = tuple(nondeterministic_prefixes)
    for item in sorted(path.rglob("*")):
        rel = item.relative_to(path).as_posix()
        st = item.lstat()
        public_rel = "<CONSENT_TOKEN_FILE>" if item.is_file() and item.name.startswith("consent_") else rel
        record = {"path": public_rel, "mode": oct(st.st_mode & 0o777), "size_bytes": st.st_size}
        if item.is_symlink():
            record["type"] = "symlink"
            record["target"] = normalize_root_value(os.readlink(item))
        elif item.is_dir():
            record["type"] = "directory"
        elif item.is_file():
            record["type"] = "file"
            nondeterministic = item.name in ("mcp_token", "identity.encrypted") or item.name.startswith("consent_") or any(rel == prefix or rel.startswith(prefix.rstrip("/") + "/") for prefix in prefixes)
            if nondeterministic:
                if item.name == "mcp_token":
                    reason = "crypto/rand token; secret intentionally not captured"
                elif item.name.startswith("consent_"):
                    reason = "crypto/rand consent token and wall-clock expiry; secret intentionally not captured"
                elif item.name == "identity.encrypted":
                    reason = "AES-GCM random nonce; encrypted identity content intentionally not captured"
                elif item.suffix in (".html", ".json", ".csv") and "report" in rel:
                    reason = "rendered report contains current UTC time; content intentionally not captured"
                else:
                    reason = "operation-generated content contains wall-clock or private state; content intentionally not captured"
                record["content"] = {"nondeterministic": True, "reason": reason}
            else:
                data = normalize_root_bytes(item.read_bytes())
                record["size_bytes"] = len(data)
                record["sha256"] = hashlib.sha256(data).hexdigest()
        else:
            record["type"] = "other"
        entries.append(record)
    return entries

def side_effect_runtime(case_id):
    case_root = root / "filesystem" / case_id
    shutil.rmtree(case_root, ignore_errors=True)
    case_root.mkdir(parents=True)
    env = dict(base_env)
    paths = {
        "HOME": case_root / "home", "XDG_CONFIG_HOME": case_root / "xdg-config",
        "XDG_DATA_HOME": case_root / "xdg-data", "XDG_CACHE_HOME": case_root / "xdg-cache",
        "TMPDIR": case_root / "tmp", "SYMERASEME_DATA_DIR": case_root / "data",
        "SYMERASEME_DB_DIR": case_root / "db", "SYMERASEME_CONFIG_DIR": case_root / "config",
    }
    for key, value in paths.items():
        value.mkdir(parents=True, exist_ok=True)
        env[key] = str(value)
    cwd = case_root / "cwd"
    cwd.mkdir()
    return case_root, env, cwd


def side_effect_process(case_id, argv, env, cwd):
    result = run_process(argv, case_name=case_id, env_extra=env, cwd=str(cwd))
    stdout = normalize_root_bytes(result.pop("_stdout_raw"))
    stderr = normalize_root_bytes(result.pop("_stderr_raw"))
    return {
        "argv": normalize_root_value(argv), "exit_code": result["exit_code"],
        "stdout_bytes": len(stdout), "stderr_bytes": len(stderr),
        "stdout_sha256": hashlib.sha256(stdout).hexdigest(),
        "stderr_sha256": hashlib.sha256(stderr).hexdigest(),
        "output_policy": "bytes_not_embedded; secrets and private state excluded",
    }


def consent_side_effect_process(case_id, argv, env, cwd):
    result = run_process(argv, case_name=case_id, env_extra=env, cwd=str(cwd))
    stdout = normalize_root_bytes(result.pop("_stdout_raw"))
    stderr = normalize_root_bytes(result.pop("_stderr_raw"))
    token_files = sorted(pathlib.Path(env["SYMERASEME_DATA_DIR"]).glob("consent_*.json"))
    if len(token_files) != 1:
        raise RuntimeError(f"expected exactly one consent token file, found {len(token_files)}")
    record = json.loads(token_files[0].read_text(encoding="utf-8"))
    token = record.get("token", "").encode("utf-8")
    time_pattern = rb"(?:20[0-9]{2}-[0-9]{2}-[0-9]{2}|issued_at|expires_at)"
    if token and (token in stdout or token in stderr):
        raise RuntimeError("consent output leaked the issued token")
    if re.search(time_pattern, stdout) or re.search(time_pattern, stderr):
        raise RuntimeError("consent output contained a time-derived field")
    return {
        "argv": normalize_root_value(argv), "exit_code": result["exit_code"],
        "stdout_bytes": len(stdout), "stderr_bytes": len(stderr),
        "stdout_sha256": hashlib.sha256(stdout).hexdigest(),
        "stderr_sha256": hashlib.sha256(stderr).hexdigest(),
        "consent_output_assertion": {
            "token_bytes_absent": True, "time_derived_bytes_absent": True,
            "checked_output": "stdout/stderr",
        },
        "output_policy": "validated_no_token_or_time_derived_bytes; bytes_not_embedded",
    }


def fs_case(case_id, argv, env, cwd, roots, nondeterministic_prefixes=(), nondeterministic_fields=(), process=None):
    return {
        "schema": "symeraseme.go-oracle.filesystem.v1", "id": case_id,
        "operation": normalize_root_value(argv), "process": process or side_effect_process(case_id, argv, env, cwd),
        "roots": {name: "isolated" for name in roots},
        "manifest_roots": {name: normalize_root_value(str(path)) for name, path in roots.items()},
        "manifests": {name: manifest(path, nondeterministic_prefixes) for name, path in roots.items()},
        "nondeterministic_fields": [{"path": "operation/process.argv/manifest_roots", "reason": "only the exact unique runtime root is normalized", "replacement": "<ORACLE_ROOT>"}] + list(nondeterministic_fields),
    }

filesystem_cases = []

# Profile creation uses a fixed env-supplied key, so the generator never calls
# the host keychain. The AES-GCM nonce remains explicitly nondeterministic.
case_root, env, cwd = side_effect_runtime("profile-init")
profile_path = case_root / "profile" / "identity.encrypted"
profile_path.parent.mkdir()
profile_argv = ["--output", "json", "init-profile", "--full-name", "Oracle User", "--email", "oracle@example.invalid", "--profile", str(profile_path)]
filesystem_cases.append(fs_case("profile-init", profile_argv, env, cwd, {"home": pathlib.Path(env["HOME"]), "data_dir": pathlib.Path(env["SYMERASEME_DATA_DIR"]), "profile_dir": profile_path.parent}, ("identity.encrypted",), ({"path": "manifests.profile_dir[identity.encrypted].content", "reason": "AES-GCM nonce is generated from crypto/rand", "replacement": "declaration only"},), side_effect_process("profile-init", profile_argv, env, cwd)))

# Consent issuance is captured only as process metadata; the token value and
# its hashed filename are never serialized. Filename and payload are markers.
case_root, env, cwd = side_effect_runtime("consent-grant")
consent_argv = ["grant", "--command", "execute", "--ttl", "3600"]
filesystem_cases.append(fs_case("consent-grant", consent_argv, env, cwd, {"data_dir": pathlib.Path(env["SYMERASEME_DATA_DIR"])}, ("<CONSENT_TOKEN_FILE>",), ({"path": "manifests.data_dir[<CONSENT_TOKEN_FILE>]", "reason": "consent token, hashed filename, and expiry are random/time-derived", "replacement": "path and content declarations only"},), consent_side_effect_process("consent-grant", consent_argv, env, cwd)))

# Scheduler generation is a real file-writing path, using cron so no native
# scheduler or system service is contacted.
case_root, env, cwd = side_effect_runtime("schedule-generate")
schedule_dir = case_root / "scheduler"
schedule_argv = ["generate-scheduler", "--platform", "cron", "--output-dir", str(schedule_dir), "--project-dir", str(case_root / "project"), "--symeraseme-bin", str(root / "bin" / "symeraseme")]
filesystem_cases.append(fs_case("schedule-generate", schedule_argv, env, cwd, {"scheduler_dir": schedule_dir}, (), (), side_effect_process("schedule-generate", schedule_argv, env, cwd)))

# Report generation writes an HTML artifact. Its two structurally known
# rendered timestamps are normalized, then the normalized bytes remain as
# evidence (both content and hash) instead of being discarded.
case_root, env, cwd = side_effect_runtime("report-generate")
report_path = case_root / "reports" / "report.html"
report_path.parent.mkdir()
report_argv = ["generate-report", "--all-campaigns", "--format", "html", "--output", str(report_path)]
report_process = side_effect_process("report-generate", report_argv, env, cwd)
report_bytes = bounded_read(report_path, 16 * 1024 * 1024, "report artifact", "report-generate")
report_bytes = normalize_root_bytes(report_bytes)
report_bytes, report_timestamp_count = re.subn(rb"20[0-9]{2}-[0-9]{2}-[0-9]{2} [0-9]{2}:[0-9]{2} UTC", b"<TIMESTAMP>", report_bytes)
if report_timestamp_count != 2:
    raise RuntimeError(f"expected exactly two report timestamps, got {report_timestamp_count}")
report_case = fs_case("report-generate", report_argv, env, cwd, {"reports": report_path.parent}, ("report.html",), ({"path": "manifests.reports[report.html].content", "reason": "report content is represented by normalized evidence below", "replacement": "normalized content evidence"},), report_process)
report_case["artifact_evidence"] = {
    "path": "reports/report.html", "encoding": "base64", "content_base64": b64(report_bytes),
    "size_bytes": len(report_bytes), "sha256": hashlib.sha256(report_bytes).hexdigest(),
    "normalizations": [{"path": "artifact rendered timestamps", "occurrences": report_timestamp_count, "replacement": "<TIMESTAMP>"}],
}
filesystem_cases.append(report_case)

# A known registry entry with no browser executor creates the durable manual
# fallback path (and returns a classified manual-action outcome).
case_root, env, cwd = side_effect_runtime("manual-task-create")
manual_argv = ["run-web-form", "virtual-minds-eu", "--output", "json"]
filesystem_cases.append(fs_case("manual-task-create", manual_argv, env, cwd, {"data_dir": pathlib.Path(env["SYMERASEME_DATA_DIR"]), "db_dir": pathlib.Path(env["SYMERASEME_DB_DIR"]), "home": pathlib.Path(env["HOME"])}, ("manual_tasks", "symeraseme.db"), ({"path": "manifests.db_dir[symeraseme.db].content", "reason": "SQLite database contains wall-clock event/task data", "replacement": "declaration only"}, {"path": "manifests.data_dir[manual_tasks/**].content", "reason": "manual evidence can contain private and time-derived data", "replacement": "declaration only"}), side_effect_process("manual-task-create", manual_argv, env, cwd)))

# Migration is run against a synthetic legacy tree and a separate destination;
# this freezes backup/state/config side effects without touching user paths.
case_root, env, cwd = side_effect_runtime("migration")
source = case_root / "legacy-source"
destination = case_root / "go-destination"
source.mkdir(); (source / "config.toml").write_text("data_dir = 'legacy'\n", encoding="utf-8")
migration_argv = ["migrate", "--source", str(source), "--destination", str(destination), "--home", str(pathlib.Path(env["HOME"])), "--platform", "cron", "--json"]
migration_process = side_effect_process("migration", migration_argv, env, cwd)
filesystem_cases.append(fs_case("migration", migration_argv, env, cwd, {"source": source, "destination": destination, "backup": pathlib.Path(str(destination) + ".migration-backup")}, (), (), migration_process))

# Verify token rotation in one isolated data directory: two independent server
# starts replace the stable mcp_token path, while both secret values remain
# absent from the fixture.
case_root, env, cwd = side_effect_runtime("mcp-token-rotation")
def one_token_server(env, cwd):
    port = free_port()
    proc, capture_paths = start_process(
        ["mcp", "--host", "127.0.0.1", "--port", str(port)], env, str(cwd),
    )
    try:
        deadline = time.time() + 10
        while time.time() < deadline:
            try:
                with socket.create_connection(("127.0.0.1", port), timeout=0.2):
                    break
            except OSError:
                if proc.poll() is not None:
                    raise RuntimeError("token rotation server exited early")
                time.sleep(0.05)
        else:
            raise RuntimeError("token rotation server did not start")
        token_path = pathlib.Path(env["SYMERASEME_DATA_DIR"]) / "mcp_token"
        token = token_path.read_text(encoding="utf-8")
    finally:
        return_code, stdout, stderr = stop_process(
            proc, capture_paths, "token-rotation", terminate=proc.poll() is None,
        )
    if return_code != 0:
        raise RuntimeError(f"token rotation server exit={return_code}: {stderr!r}")
    return token
rotation_first = one_token_server(env, cwd)
rotation_before = manifest(pathlib.Path(env["SYMERASEME_DATA_DIR"]))
rotation_second = one_token_server(env, cwd)
if not rotation_first or rotation_first == rotation_second:
    raise RuntimeError("MCP token did not rotate")
rotation_after = manifest(pathlib.Path(env["SYMERASEME_DATA_DIR"]))
rotation_roots = {"data_dir": pathlib.Path(env["SYMERASEME_DATA_DIR"])}
filesystem_cases.append({
    "schema": "symeraseme.go-oracle.filesystem.v1", "id": "mcp-token-rotation",
    "operation": ["mcp", "--host", "127.0.0.1", "--port", "<PORT>"],
    "process": {"starts": 2, "shutdown": "graceful", "tokens_equal": False},
    "roots": {"data_dir": "isolated"}, "manifest_roots": {"data_dir": normalize_root_value(str(rotation_roots["data_dir"]))},
    "before": {"data_dir": rotation_before}, "after": {"data_dir": rotation_after},
    "manifests": {"data_dir": rotation_after},
    "nondeterministic_fields": [
        {"path": "before.data_dir[*].content", "reason": "server-issued crypto/rand MCP token; secret intentionally not captured", "replacement": "declaration only"},
        {"path": "after.data_dir[*].content", "reason": "server-issued crypto/rand MCP token; secret intentionally not captured", "replacement": "declaration only"},
        {"path": "operation.port", "reason": "ephemeral local listener", "replacement": "<PORT>"},
    ],
})
write_json(out / "cases" / "filesystem" / "manifests.json", {"schema": "symeraseme.go-oracle.filesystem.v1", "commit": commit, "toolchain": toolchain, "normalization": normalization_contract, "cases": filesystem_cases})

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
- `../cases/cli/behavior.json` — raw CLI help and unknown-flag behavior for
  every command, root version/unknown-command cases, true parser-level missing
  arguments, and one isolated operational invocation for every executable leaf.
  Each operational record classifies success versus deterministic backend error.
- `../cases/mcp/transcript.jsonl` — one fresh `mcp --stdio` process per raw
  JSON-RPC transcript, including 26 schema-valid `tools/call` requests (one
  per pinned tool), ID/params, multi-frame, truncation, shutdown,
  batch/notification/legacy/error cases. `stdout_base64` and `stderr_base64`
  preserve bytes; only explicitly listed current-time result fields use a marker.
- `../cases/http/transcript.json` — MCP HTTP bind/remote policy, method,
  content type, Host, strict bearer auth, origin, malformed body, notification,
  exact 5 MiB boundary, and oversized-body cases using local listeners only.
- `../cases/filesystem/manifests.json` — isolated HOME/XDG/TMPDIR side-effect
  manifests for profile creation, consent, scheduler/report output, durable
  manual fallback, migration, and MCP-token rotation. Random/private bytes are
  represented only by path-specific nondeterminism declarations.

All records carry the oracle commit and a schema identifier. Fixture generation
uses Go `go1.26.6` resolved with `GOTOOLCHAIN=go1.26.6` and `GOPROXY=off`,
`GOENV=off`, `GOWORK=off`, an isolated `HOME`/build cache, and only the pinned
module cache. It also uses UTC, locale `C`, an empty private executable search
path, empty credential variables, and no host identity. The runtime root is a
unique `mktemp` directory owned by this invocation. Only that exact root is
replaced with `<ORACLE_ROOT>` in argv/stdout/stderr/manifests and artifact
content; this narrow rule is declared in each corpus document's `normalization`.
The only other nondeterministic values are marked in `nondeterministic_fields`:
ephemeral ports, server-issued MCP/consent tokens, encrypted-profile nonces,
private or time-derived durable artifacts, HTTP `Date`, and current-time fields
in status/dashboard/calendar/report results. Report HTML retains normalized
content and SHA-256 evidence. Consent output is checked to contain neither the
issued token nor known time-derived fields. The oversized HTTP request is
represented by its exact byte length and SHA-256, not stored verbatim. The MCP
token is represented by `<MCP_TOKEN>` in the request transcript and its file
content is never captured. The remote-policy allow probe accepts policy for
TEST-NET `192.0.2.1` and then records the expected OS bind failure; it never
binds `0.0.0.0` or a LAN interface.

The generator builds a complete same-filesystem staging corpus and swaps
`cases` and `fixtures` only after all generation and count checks pass. Any
failure leaves the previous corpus byte-identical and rollback restores it if a
swap is interrupted.

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
print(f"MCP cases={len(mcp_cases)} HTTP cases={len(startup_cases) + len(http_cases)} filesystem_cases={len(filesystem_cases)}")
PYEOF

# Validate the complete staged corpus before touching the existing checkout.
python3 - "${STAGE_DIR}" "${RUNTIME_ROOT}" <<'PYEOF'
import json
import pathlib
import sys

stage = pathlib.Path(sys.argv[1])
runtime_root = sys.argv[2].encode()
cases = stage / "cases"
cli = json.loads((cases / "cli" / "behavior.json").read_text())
mcp = [json.loads(line) for line in (cases / "mcp" / "transcript.jsonl").read_text().splitlines() if line]
http = json.loads((cases / "http" / "transcript.json").read_text())
filesystem = json.loads((cases / "filesystem" / "manifests.json").read_text())
surface = json.loads((cases / "cli" / "surface.json").read_text())
expected = {"cli": 165, "mcp": 52, "http": 19, "filesystem": 7}
actual = {"cli": len(cli["cases"]), "mcp": len(mcp), "http": len(http["cases"]), "filesystem": len(filesystem["cases"])}
if actual != expected:
    raise SystemExit(f"coverage changed: expected {expected}, got {actual}")
if surface.get("schema") != "symeraseme.go-oracle.cli-surface.v1":
    raise SystemExit("surface schema identifier is missing")
for document in (cli, http, filesystem):
    if not document.get("toolchain", {}).get("go_version", "").startswith("go version go1.26.6 "):
        raise SystemExit("fixture toolchain record is not Go 1.26.6")
    if document.get("toolchain", {}).get("goproxy") != "off":
        raise SystemExit("fixture toolchain is not offline")
for path in stage.rglob("*"):
    if path.is_file() and runtime_root in path.read_bytes():
        raise SystemExit(f"un-normalized runtime root in {path}")
print(f"validated staged coverage: {actual}")
PYEOF

# Swap the two generated roots only after complete success. The EXIT trap
# restores the old pair if either move fails or the process is interrupted.
mkdir "${BACKUP_DIR}"
SWAP_STARTED=1
if [[ -e "${OUT_DIR}/cases" ]]; then mv "${OUT_DIR}/cases" "${BACKUP_DIR}/cases"; CASES_BACKED_UP=1; fi
if [[ -e "${OUT_DIR}/fixtures" ]]; then mv "${OUT_DIR}/fixtures" "${BACKUP_DIR}/fixtures"; FIXTURES_BACKED_UP=1; fi
mv "${STAGE_DIR}/cases" "${OUT_DIR}/cases"
CASES_INSTALLED=1
mv "${STAGE_DIR}/fixtures" "${OUT_DIR}/fixtures"
FIXTURES_INSTALLED=1
SWAP_STARTED=0
rm -rf "${BACKUP_DIR}" "${STAGE_DIR}"
printf 'generated oracle fixtures from %s\n' "${PINNED_COMMIT}"

#!/usr/bin/env bash
# Shared Make/CI gate: retain the existing 80% project / 90% critical Rust policy.
set -euo pipefail
cargo_command="${CARGO:-cargo}"
critical_pattern='/(crypto|consent|auth|mcp)(/|\.rs$)'
code_item_pattern='^[[:space:]]*(pub([[:space:]]*\([^)]*\))?[[:space:]]+)?(async[[:space:]]+|unsafe[[:space:]]+|const[[:space:]]+)?(fn|impl|trait|struct|enum|union|const|static|type|macro_rules!|extern)([^[:alnum:]_]|$)'
critical_files=()
declaration_only=()
while IFS= read -r file; do
  if grep -Eq "$code_item_pattern" "$file"; then
    critical_files+=("$file")
  else
    declaration_only+=("$file")
  fi
done < <(find crates -type f -name '*.rs' -not -path '*/commands/*' -print | grep -Ei "$critical_pattern" || true)
if [[ ${#declaration_only[@]} -gt 0 ]]; then
  printf 'Declaration-only critical modules: %s\n' "${declaration_only[*]}"
fi
if [[ ${#critical_files[@]} -eq 0 ]]; then
  printf 'No critical Rust files found; cannot establish coverage.\n' >&2
  exit 1
fi
coverage_json="${COVERAGE_FILE:-${RUNNER_TEMP:-${CARGO_TARGET_DIR:-target}}/rust-coverage.json}"
mkdir -p "$(dirname "$coverage_json")"
"$cargo_command" +1.98.0 llvm-cov --workspace --all-features --json \
  --output-path "$coverage_json" --fail-under-lines 80
"${PYTHON:-python3}" - "$coverage_json" 90 "${critical_files[@]}" <<'PY'
import json
import pathlib
import sys

report_path = pathlib.Path(sys.argv[1])
threshold = float(sys.argv[2])
expected = {(pathlib.Path.cwd() / path).resolve() for path in sys.argv[3:]}
report = json.loads(report_path.read_text())
data = report.get("data")
if not isinstance(data, list) or len(data) != 1:
    raise SystemExit("coverage report must contain exactly one data entry")
if not isinstance(data[0], dict) or not isinstance(data[0].get("files"), list):
    raise SystemExit("coverage report has a malformed files list")
files = {}
for entry in data[0]["files"]:
    if not isinstance(entry, dict):
        raise SystemExit("coverage report has a malformed file entry")
    raw_filename = entry.get("filename")
    summary = entry.get("summary")
    if not isinstance(raw_filename, str) or not raw_filename or not isinstance(summary, dict):
        raise SystemExit("coverage report has a malformed file entry")
    lines = summary.get("lines")
    if not isinstance(lines, dict):
        raise SystemExit("coverage report has a malformed line summary")
    count = lines.get("count")
    covered = lines.get("covered")
    if type(count) is not int or type(covered) is not int:
        raise SystemExit("coverage report has malformed line counts")
    if count < 0 or covered < 0 or covered > count:
        raise SystemExit("coverage report has invalid line counts")
    filename = pathlib.Path(raw_filename).resolve()
    if filename in files:
        raise SystemExit(f"coverage report has duplicate file entry: {filename}")
    files[filename] = (count, covered)
missing = sorted(str(path) for path in expected if path not in files)
if missing:
    raise SystemExit(f"critical coverage files missing from report: {missing}")
counts = [files[path] for path in expected]
if any(count == 0 for count, _ in counts):
    raise SystemExit("critical coverage file contains no measurable lines")
total = sum(count for count, _ in counts)
covered = sum(value for _, value in counts)
percent = covered * 100 / total
print(f"Critical Rust line coverage: {percent:.2f}% ({covered}/{total}), gate: {threshold:.0f}%")
if percent < threshold:
    raise SystemExit(1)
PY

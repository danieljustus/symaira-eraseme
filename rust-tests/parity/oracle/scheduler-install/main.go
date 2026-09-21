// Command scheduler-install is the committed Go provenance oracle for the
// scheduler install/status/uninstall contract.
//
// `scheduler.Install`, `Status` and `Uninstall` all read and write real
// filesystem state (the per-user unit directory, generated wrappers) and shell
// out to the platform's own tooling. This oracle therefore runs each case
// against a private HOME and a recording Runner, so the observed filesystem
// effects and the exact commands Go issues are captured without touching the
// machine or requiring launchctl/systemctl/crontab to exist.
//
// The recording Runner answers with scripted output per command, which is how
// the status cases pin Go's active/error classification.
package main

import (
	"bytes"
	"context"
	"crypto/sha256"
	"encoding/hex"
	"encoding/json"
	"errors"
	"flag"
	"fmt"
	"os"
	"path/filepath"
	"sort"
	"strings"
	"time"

	"github.com/danieljustus/symaira-eraseme/internal/scheduler"
)

const schema = "symeraseme-scheduler-install-oracle/v1"

// recordingRunner records every command it is asked to run and answers from a
// scripted table, so no platform tool has to exist.
type recordingRunner struct {
	Calls []recordedCall `json:"calls"`
	// Answers maps "name arg1 arg2..." to the scripted answer.
	Answers map[string]scriptedAnswer `json:"-"`
}

type recordedCall struct {
	Name string   `json:"name"`
	Args []string `json:"args"`
	Line string   `json:"line"`
}

type scriptedAnswer struct {
	Stdout string
	Err    string
}

func (r *recordingRunner) Run(_ context.Context, name string, args ...string) ([]byte, error) {
	line := strings.TrimSpace(name + " " + strings.Join(args, " "))
	r.Calls = append(r.Calls, recordedCall{Name: name, Args: args, Line: line})
	// Longest matching key wins, so "launchctl list x" beats "launchctl list".
	best := ""
	for key := range r.Answers {
		if line == key || strings.HasPrefix(line, key+" ") {
			if len(key) > len(best) {
				best = key
			}
		}
	}
	if best != "" {
		answer := r.Answers[best]
		if answer.Err != "" {
			return []byte(answer.Stdout), errors.New(answer.Err)
		}
		return []byte(answer.Stdout), nil
	}
	return nil, errors.New("no scripted answer for: " + line)
}

type caseResult struct {
	Name string `json:"name"`
	// Output is nil when the call failed; Error then carries the failure text.
	Output map[string]any `json:"output"`
	Error  string         `json:"error"`
	// Files is the generated output directory's name -> sha256, plus the
	// native unit directory when the platform installed units.
	Files map[string]string `json:"files"`
	// Mode is path -> octal permission string for the files above.
	Mode  map[string]string `json:"mode"`
	Calls []recordedCall    `json:"calls"`
}

type fixture struct {
	Schema         string                `json:"schema"`
	SourceRevision string                `json:"source_revision"`
	Source         string                `json:"source"`
	RunnerScript   map[string]string     `json:"runner_script"`
	Cases          map[string]caseResult `json:"cases"`
	Notes          map[string]string     `json:"notes"`
}

var runnerScript = map[string]scriptedAnswer{
	// Status answers: an installed-but-inactive launchd unit, and a systemd
	// timer that reports active.
	"launchctl list com.symeraseme.tick":                 {Stdout: "com.symeraseme.tick\n"},
	"launchctl list com.symeraseme.poll":                 {Err: "Could not find service"},
	"launchctl list com.symeraseme.rescan":               {Err: "Could not find service"},
	"systemctl --user is-active symeraseme-tick.timer":   {Stdout: "active\n"},
	"systemctl --user is-active symeraseme-poll.timer":   {Stdout: "inactive\n"},
	"systemctl --user is-active symeraseme-rescan.timer": {Err: "Unit not found."},
	// Path-carrying commands are matched by their stable prefix, so the private
	// temp paths never have to be scripted.
	"launchctl unload": {},
	"launchctl load":   {},
	// Bare "crontab" covers the write-back ("crontab <file>"); the read is the
	// longer "crontab -l" key and wins on the longest match.
	"crontab": {},
	// Install/uninstall side-effect commands succeed with empty output.
	"systemctl --user daemon-reload":                         {},
	"systemctl --user enable --now symeraseme-tick.timer":    {},
	"systemctl --user enable --now symeraseme-poll.timer":    {},
	"systemctl --user enable --now symeraseme-rescan.timer":  {},
	"systemctl --user disable --now symeraseme-tick.timer":   {},
	"systemctl --user disable --now symeraseme-poll.timer":   {},
	"systemctl --user disable --now symeraseme-rescan.timer": {},
	// Cron: an existing crontab without our block, and the write back.
	"crontab -l": {Stdout: "17 * * * * /usr/local/bin/other-job\n"},
}

// cronAnswers are per-case crontab answers; the block-installing case must see
// an existing Symaira block so the replacement path is exercised.
func answersFor(name string) map[string]scriptedAnswer {
	answers := map[string]scriptedAnswer{}
	for key, value := range runnerScript {
		answers[key] = value
	}
	switch name {
	case "cron_install_replaces_existing_block":
		answers["crontab -l"] = scriptedAnswer{Stdout: "17 * * * * /other\n# Symaira EraseMe scheduled tasks\n0 10 * * * stale\n# End Symaira EraseMe scheduled tasks\n"}
	}
	return answers
}

// normalize replaces volatile temp paths with stable markers so the fixture is
// reproducible: the private case root, the oracle's own temp root, and the
// crontab staging file Go creates under TMPDIR.
func normalize(value, caseRoot string) string {
	out := filepath.ToSlash(value)
	// Fold the shared oracle temp root, which carries a random suffix.
	if root := oracleTempRoot; root != "" {
		out = strings.ReplaceAll(out, filepath.ToSlash(root), "<TMPROOT>")
	}
	out = strings.ReplaceAll(out, filepath.ToSlash(caseRoot), "<CASE>")
	staging := filepath.ToSlash(os.TempDir())
	out = strings.ReplaceAll(out, staging+"/.crontab-", "<TMP>/.crontab-")
	out = strings.ReplaceAll(out, staging+"/.symeraseme-crontab-", "<TMP>/.symeraseme-crontab-")
	// The staging suffix itself is a random number.
	out = collapseRandom(out, ".crontab-")
	out = collapseRandom(out, ".symeraseme-crontab-")
	return out
}

// collapseRandom replaces the digits after every occurrence of marker with
// <RANDOM>, wherever the marker sits (the staging file lives inside the case
// root for cron installs and in TMPDIR for cron uninstall).
func collapseRandom(value, marker string) string {
	var out strings.Builder
	rest := value
	for {
		index := strings.Index(rest, marker)
		if index < 0 {
			out.WriteString(rest)
			return out.String()
		}
		end := index + len(marker)
		for end < len(rest) && rest[end] >= '0' && rest[end] <= '9' {
			end++
		}
		out.WriteString(rest[:index])
		out.WriteString(marker)
		out.WriteString("<RANDOM>")
		rest = rest[end:]
	}
}

// oracleTempRoot is the run's shared temp root; recorded paths fold it to
// <TMPROOT> so a random suffix never reaches the fixture.
var oracleTempRoot string

func digest(content string) string {
	sum := sha256.Sum256([]byte(content))
	return hex.EncodeToString(sum[:])
}

// capture walks walkRoot and records name -> sha256 plus name -> mode for
// regular files only, so directories and the temp staging files Go removes are
// absent. Each file's bytes are normalized first: the generated units embed the
// wrapper directory, which lives under the volatile case root, so hashing the
// raw bytes would make the fixture irreproducible.
func capture(walkRoot, caseRoot string) (map[string]string, map[string]string) {
	hashes := map[string]string{}
	modes := map[string]string{}
	if _, err := os.Stat(walkRoot); err != nil {
		return hashes, modes
	}
	_ = filepath.Walk(walkRoot, func(path string, info os.FileInfo, err error) error {
		if err != nil || info.IsDir() || !info.Mode().IsRegular() {
			return nil
		}
		rel, relErr := filepath.Rel(walkRoot, path)
		if relErr != nil {
			return nil
		}
		rel = filepath.ToSlash(rel)
		data, readErr := os.ReadFile(path)
		if readErr != nil {
			return nil
		}
		hashes[rel] = digest(normalize(string(data), caseRoot))
		modes[rel] = fmt.Sprintf("%04o", info.Mode().Perm())
		return nil
	})
	return hashes, modes
}

// seedLegacy writes the known unit names so the legacy-scan branches run.
//
// note: Go's Status builds its path as "<name>.plist" for launchd while
// Uninstall and the legacy scan use "<nameToLaunchdLabel>.plist"; a case that
// seeds one set shows which of the two a given entry point reads.
func seedLegacy(home string, platform scheduler.Platform, python bool) {
	var dir string
	var names []string
	switch platform {
	case scheduler.PlatformLaunchd:
		dir = filepath.Join(home, "Library", "LaunchAgents")
		names = []string{"com.symeraseme.tick.plist", "com.symeraseme.poll.plist", "com.symeraseme.rescan.plist"}
	case scheduler.PlatformSystemd:
		dir = filepath.Join(home, ".config", "systemd", "user")
		names = []string{"symeraseme-tick.timer", "symeraseme-poll.timer", "symeraseme-rescan.timer"}
	default:
		return
	}
	if err := os.MkdirAll(dir, 0o755); err != nil {
		panic(err)
	}
	content := "# Generated by symeraseme generate-scheduler (Go)\n"
	if python {
		content = "# Generated by symeraseme generate-scheduler\n/usr/bin/python3 -m symeraseme.core.scheduler\n"
	}
	for _, name := range names {
		if err := os.WriteFile(filepath.Join(dir, name), []byte(content), 0o644); err != nil {
			panic(err)
		}
	}
}

// seedStatusNames writes the names Status itself resolves, so the
// installed-flags branch is exercised even where it disagrees with Uninstall.
func seedStatusNames(home string, platform scheduler.Platform) {
	var dir string
	var names []string
	switch platform {
	case scheduler.PlatformLaunchd:
		dir = filepath.Join(home, "Library", "LaunchAgents")
		names = []string{"symeraseme-tick.plist", "symeraseme-poll.plist", "symeraseme-rescan.plist"}
	case scheduler.PlatformSystemd:
		dir = filepath.Join(home, ".config", "systemd", "user")
		names = []string{"symeraseme-tick.timer", "symeraseme-poll.timer", "symeraseme-rescan.timer"}
	default:
		return
	}
	if err := os.MkdirAll(dir, 0o755); err != nil {
		panic(err)
	}
	for _, name := range names {
		if err := os.WriteFile(filepath.Join(dir, name), []byte("# Generated by symeraseme generate-scheduler (Go)\n"), 0o644); err != nil {
			panic(err)
		}
	}
}

func baseConfig(platform scheduler.Platform, outputDir string) scheduler.Config {
	return scheduler.Config{
		Platform:   platform,
		OutputDir:  outputDir,
		TickHour:   10,
		TickMinute: 0,
		ProjectDir: "/srv/erase",
		BinaryPath: "/usr/local/bin/symeraseme",
	}
}

func main() {
	out := flag.String("fixture", "", "write the recorded fixture to this path")
	revision := flag.String("revision", "", "source revision the capture names")
	flag.Parse()

	// The revision this capture was taken from; the harness passes it in.
	oracleRoot, err := os.MkdirTemp("", "symeraseme-sched-install-oracle-")
	if err != nil {
		panic(err)
	}
	oracleTempRoot = oracleRoot

	sourceRevision := *revision
	if sourceRevision == "" {
		sourceRevision = "working-tree"
	}
	recorded := fixture{
		Schema:         schema,
		SourceRevision: sourceRevision,
		Source:         "internal/scheduler/scheduler.go (Install/Status/Uninstall/installRoot/ScanLegacyPythonUnits)",
		RunnerScript:   map[string]string{},
		Cases:          map[string]caseResult{},
		Notes: map[string]string{
			"environment": "each case runs in a private HOME under a per-case temp root; the Runner is a recording fake",
			"paths":       "file hashes are relative to the recorded root that the case names in files_root",
			"clock":       "no case depends on the wall clock",
		},
	}
	for line, answer := range runnerScript {
		recorded.RunnerScript[line] = answer.Stdout
		if answer.Err != "" {
			recorded.RunnerScript[line] = "ERROR: " + answer.Err
		}
	}

	root := oracleTempRoot
	defer os.RemoveAll(root)

	type scenario struct {
		name     string
		platform scheduler.Platform
		legacy   bool
		replace  bool
		// op selects which entry point the case exercises.
		op string
		// seedInstallDir pre-creates native units for the status cases.
		seedInstallDir bool
		// statusNames writes exactly the filenames Status looks up, which differ
		// from the legacy/label names on launchd.
		statusNames bool
		// secondInstall runs Install twice in the same HOME. The first writes
		// units under the same names the legacy scan treats as replacement
		// candidates, so the second run shows whether a re-install self-blocks.
		secondInstall bool
	}
	scenarios := []scenario{
		{name: "launchd_install_no_legacy", platform: scheduler.PlatformLaunchd, op: "install"},
		{name: "launchd_install_refuses_python_legacy", platform: scheduler.PlatformLaunchd, legacy: true, op: "install"},
		{name: "launchd_install_replaces_python_legacy", platform: scheduler.PlatformLaunchd, legacy: true, replace: true, op: "install"},
		{name: "launchd_reinstall_after_a_successful_install", platform: scheduler.PlatformLaunchd, secondInstall: true, op: "install"},
		{name: "systemd_reinstall_after_a_successful_install", platform: scheduler.PlatformSystemd, secondInstall: true, op: "install"},
		{name: "launchd_status_reports_active_and_errors", platform: scheduler.PlatformLaunchd, seedInstallDir: true, op: "status"},
		{name: "launchd_status_names_differ_from_uninstall_names", platform: scheduler.PlatformLaunchd, statusNames: true, op: "status"},
		{name: "launchd_status_after_uninstall_names_are_gone", platform: scheduler.PlatformLaunchd, seedInstallDir: true, statusNames: true, op: "status"},
		{name: "launchd_uninstall_removes_units", platform: scheduler.PlatformLaunchd, seedInstallDir: true, op: "uninstall"},
		{name: "systemd_install_no_legacy", platform: scheduler.PlatformSystemd, op: "install"},
		{name: "systemd_status_reports_active_and_errors", platform: scheduler.PlatformSystemd, seedInstallDir: true, op: "status"},
		{name: "systemd_uninstall_removes_units", platform: scheduler.PlatformSystemd, seedInstallDir: true, op: "uninstall"},
		{name: "cron_install_writes_block", platform: scheduler.PlatformCron, op: "install"},
		{name: "cron_install_replaces_existing_block", platform: scheduler.PlatformCron, op: "install"},
		{name: "cron_status_reports_missing_block", platform: scheduler.PlatformCron, op: "status"},
		{name: "cron_uninstall_rewrites_crontab", platform: scheduler.PlatformCron, op: "uninstall"},
		{name: "unsupported_platform_is_rejected", platform: scheduler.Platform("windows"), op: "install"},
	}

	for _, sc := range scenarios {
		caseRoot := filepath.Join(root, sc.name)
		home := filepath.Join(caseRoot, "home")
		outputDir := filepath.Join(caseRoot, "schedules")
		if err := os.MkdirAll(home, 0o755); err != nil {
			panic(err)
		}
		if sc.legacy {
			seedLegacy(home, sc.platform, true)
		}
		if sc.seedInstallDir {
			seedLegacy(home, sc.platform, false)
		}
		if sc.statusNames {
			seedStatusNames(home, sc.platform)
		}

		runner := &recordingRunner{Answers: answersFor(sc.name)}
		config := baseConfig(sc.platform, outputDir)
		opts := scheduler.InstallOptions{Config: config, HomeDir: home, ReplaceLegacy: sc.replace, Runner: runner}

		result := caseResult{Name: sc.name, Files: map[string]string{}, Mode: map[string]string{}}
		ctx, cancel := context.WithTimeout(context.Background(), 20*time.Second)
		switch sc.op {
		case "install":
			if sc.secondInstall {
				if _, firstErr := scheduler.Install(ctx, opts); firstErr != nil {
					result.Error = "first install: " + normalize(firstErr.Error(), caseRoot)
					break
				}
				// A fresh runner so the recorded calls belong to the second run.
				runner.Calls = nil
			}
			installed, callErr := scheduler.Install(ctx, opts)
			if callErr != nil {
				result.Error = normalize(callErr.Error(), caseRoot)
			}
			result.Output = map[string]any{
				"platform":             string(installed.Platform),
				"output_dir":           relPath(installed.OutputDir, caseRoot, home),
				"files":                relFiles(installed.Files, caseRoot),
				"legacy":               relUnits(installed.Legacy, caseRoot, home),
				"replacement_required": installed.ReplacementRequired,
			}
		case "status":
			status, callErr := scheduler.Status(ctx, opts)
			if callErr != nil {
				result.Error = normalize(callErr.Error(), caseRoot)
			}
			entries := make([]map[string]any, 0, len(status.Entries))
			for _, entry := range status.Entries {
				entries = append(entries, map[string]any{
					"label":     entry.Label,
					"installed": entry.Installed,
					"active":    entry.Active,
					"path":      relPath(entry.Path, caseRoot, home),
					"legacy":    entry.Legacy,
					"error":     entry.Error,
				})
			}
			result.Output = map[string]any{"platform": string(status.Platform), "entries": entries}
		case "uninstall":
			callErr := scheduler.Uninstall(ctx, opts)
			if callErr != nil {
				result.Error = normalize(callErr.Error(), caseRoot)
			}
			result.Output = map[string]any{"ok": callErr == nil}
		}
		cancel()

		hashes, modes := capture(caseRoot, caseRoot)
		result.Files = map[string]string{}
		for name, hash := range hashes {
			result.Files[normalize(name, caseRoot)] = hash
		}
		result.Mode = map[string]string{}
		for name, mode := range modes {
			result.Mode[normalize(name, caseRoot)] = mode
		}
		result.Calls = make([]recordedCall, 0, len(runner.Calls))
		for _, call := range runner.Calls {
			result.Calls = append(result.Calls, recordedCall{
				Name: call.Name,
				Args: []string{normalize(strings.Join(call.Args, "\x1f"), caseRoot)},
				Line: normalize(call.Line, caseRoot),
			})
		}

		recorded.Cases[sc.name] = result
	}

	document, err := json.MarshalIndent(recorded, "", "  ")
	if err != nil {
		panic(err)
	}
	document = append(document, '\n')

	if *out != "" {
		if err := os.WriteFile(*out, document, 0o644); err != nil {
			panic(err)
		}
	}
	fmt.Printf("recorded %d install/status/uninstall cases\n", len(recorded.Cases))
	fmt.Print(prettyNames(recorded.Cases))
	if *out == "" {
		os.Stdout.Write(document)
	}
}

func prettyNames(cases map[string]caseResult) string {
	names := make([]string, 0, len(cases))
	for name := range cases {
		names = append(names, name)
	}
	sort.Strings(names)
	var buffer bytes.Buffer
	for _, name := range names {
		state := "ok"
		if cases[name].Error != "" {
			state = "error: " + cases[name].Error
		}
		fmt.Fprintf(&buffer, "  %-42s %s\n", name, state)
	}
	return buffer.String()
}

// relFiles maps generated wrapper paths relative to the case root so the
// fixture stays machine-independent.
func relFiles(files []string, caseRoot string) []string {
	out := make([]string, 0, len(files))
	for _, file := range files {
		out = append(out, relPath(file, caseRoot, ""))
	}
	sort.Strings(out)
	return out
}

// relUnits drops the absolute home prefix so the fixture is machine-independent.
func relUnits(units []scheduler.LegacyUnit, caseRoot, home string) []map[string]any {
	out := make([]map[string]any, 0, len(units))
	for _, unit := range units {
		out = append(out, map[string]any{
			"platform":  string(unit.Platform),
			"kind":      unit.Kind,
			"name":      unit.Name,
			"path":      relPath(unit.Path, caseRoot, home),
			"is_python": unit.IsPython,
			"reason":    unit.Reason,
		})
	}
	return out
}

// relPath strips the private HOME and then the volatile case root so recorded
// paths are stable across machines and runs.
func relPath(path, caseRoot, home string) string {
	value := filepath.ToSlash(path)
	if home != "" {
		homeSlash := filepath.ToSlash(home)
		if value == homeSlash || strings.HasPrefix(value, homeSlash+"/") {
			return "<HOME>" + strings.TrimPrefix(value, homeSlash)
		}
	}
	// The oracle temp root is shared by all cases; fold it first so a path
	// inside another case never leaks its random name.
	if index := strings.Index(value, "/symeraseme-sched-install-oracle-"); index >= 0 {
		rest := value[index:]
		if slash := strings.Index(rest[len("/symeraseme-sched-install-oracle-"):], "/"); slash >= 0 {
			offset := index + len("/symeraseme-sched-install-oracle-") + slash
			value = "<TMPROOT>" + value[offset:]
		}
	}
	if strings.HasPrefix(value, filepath.ToSlash(caseRoot)+"/") {
		value = strings.TrimPrefix(value, filepath.ToSlash(caseRoot)+"/")
	}
	return value
}

// Command config is the committed Go configuration provenance oracle.
//
// The parent process creates isolated HOME/XDG/CWD/TEMP inputs under /tmp and
// invokes the existing internal/config package in a child process. This keeps
// process-global state out of the Rust test while exercising the Go contract.
package main

import (
	"encoding/json"
	"fmt"
	"os"
	"os/exec"
	"path/filepath"
	"runtime"
	"sort"
	"strings"

	"github.com/danieljustus/symaira-eraseme/internal/config"
)

const (
	inputFile      = "inputs.json"
	sourceRevision = "119ee9f84fe7c9e1485d25ab10aac8582e98395c"
	oracleSchema   = "symaira-eraseme.config-parity.v1"
	temporaryRoot  = "/tmp"
)

type provenance struct {
	SourceRevision string `json:"source_revision"`
	SourcePath     string `json:"source_path"`
	Schema         string `json:"schema"`
}

type runInput struct {
	Files       map[string]string `json:"files"`
	Environment map[string]string `json:"environment"`
}

type caseInput struct {
	Runs []runInput `json:"runs"`
}

type inputDocument struct {
	Provenance provenance           `json:"provenance"`
	Cases      map[string]caseInput `json:"cases"`
}

type childResult struct {
	Config     *config.Config  `json:"config,omitempty"`
	Storage    *config.Storage `json:"storage,omitempty"`
	ErrorField string          `json:"error_field,omitempty"`
}

type oracleOutput struct {
	Provenance provenance                 `json:"provenance"`
	Cases      map[string]json.RawMessage `json:"cases"`
}

func main() {
	if len(os.Args) == 3 && os.Args[1] == "--child" {
		runChild(os.Args[2])
	}

	inputs := readInputs()
	if inputs.Provenance.SourceRevision != sourceRevision ||
		inputs.Provenance.SourcePath != "internal/config/config.go" ||
		inputs.Provenance.Schema != oracleSchema {
		fatal("oracle input provenance does not match the committed contract")
	}

	cases := make(map[string]json.RawMessage, len(inputs.Cases))
	caseNames := make([]string, 0, len(inputs.Cases))
	for name := range inputs.Cases {
		caseNames = append(caseNames, name)
	}
	sort.Strings(caseNames)
	for _, name := range caseNames {
		result := runCase(name, inputs.Cases[name])
		encoded, err := json.Marshal(result)
		if err != nil {
			fatal("encode oracle result")
		}
		cases[name] = encoded
	}

	output := oracleOutput{Provenance: inputs.Provenance, Cases: cases}
	encoder := json.NewEncoder(os.Stdout)
	encoder.SetIndent("", "  ")
	if err := encoder.Encode(output); err != nil {
		fatal("write oracle result")
	}
}

func readInputs() inputDocument {
	_, source, _, ok := runtimeCaller()
	if !ok {
		fatal("oracle source path unavailable")
	}
	contents, err := os.ReadFile(filepath.Join(filepath.Dir(source), inputFile))
	if err != nil {
		fatal("oracle inputs unavailable")
	}
	var inputs inputDocument
	if err := json.Unmarshal(contents, &inputs); err != nil {
		fatal("oracle inputs are invalid")
	}
	return inputs
}

// runtimeCaller is kept as a variable so the source path remains resolved from
// the executable rather than from the caller's working directory.
var runtimeCaller = func() (uintptr, string, int, bool) {
	return runtime.Caller(0)
}

func runCase(name string, input caseInput) map[string]any {
	root, err := os.MkdirTemp(temporaryRoot, "symeraseme-config-oracle-")
	if err != nil {
		fatal("create oracle temporary root")
	}
	root, err = filepath.EvalSymlinks(root)
	if err != nil {
		fatal("resolve oracle temporary root")
	}
	defer os.RemoveAll(root)

	results := make([]childResult, 0, len(input.Runs))
	for index, run := range input.Runs {
		if err := resetRunTree(root); err != nil {
			fatal(fmt.Sprintf("reset oracle tree for %s/%d", name, index))
		}
		if err := writeRunFiles(root, run.Files); err != nil {
			fatal(fmt.Sprintf("write oracle files for %s/%d", name, index))
		}
		results = append(results, runChildProcess(root, run.Environment))
	}

	if name == "CFG-005" {
		errors := make([]string, 0, len(results))
		for _, result := range results {
			if result.ErrorField == "" {
				fatal("expected CFG-005 to contain only classified errors")
			}
			errors = append(errors, result.ErrorField)
		}
		return map[string]any{"errors": errors}
	}
	if len(results) != 1 || results[0].Config == nil || results[0].Storage == nil {
		fatal(fmt.Sprintf("case %s did not produce one successful result", name))
	}
	return map[string]any{
		"config":  results[0].Config,
		"storage": normalizePaths(root, results[0].Storage),
	}
}

func runChildProcess(root string, environment map[string]string) childResult {
	inputPath := filepath.Join(root, "child-input.json")
	encoded, err := json.Marshal(runInput{Environment: environment})
	if err != nil {
		fatal("encode child input")
	}
	if err := os.WriteFile(inputPath, encoded, 0o600); err != nil {
		fatal("write child input")
	}

	command := exec.Command(os.Args[0], "--child", inputPath)
	command.Dir = filepath.Join(root, "project")
	command.Env = isolatedEnvironment(root, environment)
	output, err := command.CombinedOutput()
	if err != nil {
		var result childResult
		if json.Unmarshal(output, &result) == nil && result.ErrorField != "" {
			return result
		}
		fatal("Go configuration child failed")
	}
	var result childResult
	if err := json.Unmarshal(output, &result); err != nil {
		fatal("decode Go configuration child")
	}
	return result
}

func runChild(path string) {
	// The input file is deliberately read to prove that the child received an
	// explicit run descriptor; configuration itself reads only the isolated env
	// and current directory, matching the production Go loader contract.
	if _, err := os.ReadFile(path); err != nil {
		fatal("read child input")
	}
	loader := config.Load()
	cfg, err := loader.Load()
	if err != nil {
		writeChildResult(childResult{ErrorField: classifyField(err)})
	}
	storage, err := config.ResolveStorage()
	if err != nil {
		writeChildResult(childResult{ErrorField: classifyField(err)})
	}
	writeChildResult(childResult{Config: cfg, Storage: &storage})
}

func writeChildResult(result childResult) {
	if err := json.NewEncoder(os.Stdout).Encode(result); err != nil {
		fatal("write child result")
	}
	if result.ErrorField != "" {
		os.Exit(0)
	}
	os.Exit(0)
}

func classifyField(err error) string {
	message := err.Error()
	for _, field := range []string{"data_dir", "db_dir", "encrypt_db", "port", "allow_remote"} {
		if strings.Contains(message, field) {
			return field
		}
	}
	for name, field := range map[string]string{
		"SYMERASEME_DATA_DIR":     "data_dir",
		"SYMERASEME_DB_DIR":       "db_dir",
		"SYMERASEME_ENCRYPT_DB":   "encrypt_db",
		"SYMERASEME_PORT":         "port",
		"SYMERASEME_ALLOW_REMOTE": "allow_remote",
	} {
		if strings.Contains(message, name) {
			return field
		}
	}
	return "unknown"
}

func isolatedEnvironment(root string, environment map[string]string) []string {
	values := map[string]string{
		"HOME":        filepath.Join(root, "home"),
		"USERPROFILE": filepath.Join(root, "home"),
		"TMPDIR":      filepath.Join(root, "tmp"),
		"TEMP":        filepath.Join(root, "tmp"),
		"TMP":         filepath.Join(root, "tmp"),
	}
	for key, value := range environment {
		values[key] = strings.ReplaceAll(value, "$ROOT", root)
	}
	keys := make([]string, 0, len(values))
	for key := range values {
		keys = append(keys, key)
	}
	sort.Strings(keys)
	env := make([]string, 0, len(keys))
	for _, key := range keys {
		env = append(env, key+"="+values[key])
	}
	return env
}

func resetRunTree(root string) error {
	for _, name := range []string{"home", "project", "xdg", "tmp"} {
		if err := os.RemoveAll(filepath.Join(root, name)); err != nil {
			return err
		}
		if err := os.MkdirAll(filepath.Join(root, name), 0o700); err != nil {
			return err
		}
	}
	return nil
}

func writeRunFiles(root string, files map[string]string) error {
	for relative, contents := range files {
		clean := filepath.Clean(relative)
		if filepath.IsAbs(clean) || clean == "." || clean == ".." || strings.HasPrefix(clean, ".."+string(filepath.Separator)) {
			return fmt.Errorf("unsafe oracle input path")
		}
		path := filepath.Join(root, clean)
		if err := os.MkdirAll(filepath.Dir(path), 0o700); err != nil {
			return err
		}
		if err := os.WriteFile(path, []byte(contents), 0o600); err != nil {
			return err
		}
	}
	return nil
}

func normalizePaths(root string, storage *config.Storage) *config.Storage {
	copy := *storage
	copy.DataDir = normalizePath(root, copy.DataDir)
	copy.DBDir = normalizePath(root, copy.DBDir)
	copy.DBPath = normalizePath(root, copy.DBPath)
	copy.TempDir = normalizePath(root, copy.TempDir)
	return &copy
}

func normalizePath(root, value string) string {
	if strings.HasPrefix(value, root) {
		return "$ROOT" + strings.TrimPrefix(value, root)
	}
	return value
}

func fatal(message string) {
	fmt.Fprintln(os.Stderr, message)
	os.Exit(1)
}

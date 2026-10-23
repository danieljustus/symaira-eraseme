// Executes production migration.Run; no replacement decoder or expected errors.
package main

import (
	"bytes"
	"context"
	"encoding/hex"
	"encoding/json"
	"io/fs"
	"os"
	"path/filepath"

	"github.com/danieljustus/symaira-eraseme/internal/migration"
	"github.com/danieljustus/symaira-eraseme/internal/scheduler"
)

func must(err error) {
	if err != nil {
		panic(err)
	}
}

// Only ephemeral path prefixes are normalized; JSON diagnostics are untouched.
func paths(data []byte, root string, expand bool) []byte {
	for _, name := range []string{"source", "destination", "backup"} {
		encoded, err := json.Marshal(filepath.Join(root, name))
		must(err)
		actual, placeholder := encoded[1:len(encoded)-1], []byte("@ROOT@/"+name)
		if expand {
			data = bytes.ReplaceAll(data, placeholder, actual)
		} else {
			data = bytes.ReplaceAll(data, actual, placeholder)
		}
	}
	return data
}
func snapshot(root string) map[string]string {
	result := map[string]string{}
	entries, err := os.ReadDir(root)
	must(err)
	for _, entry := range entries {
		name := entry.Name()
		if name == "home" {
			continue
		}
		base := filepath.Join(root, name)
		if _, err := os.Stat(base); os.IsNotExist(err) {
			continue
		}
		must(filepath.WalkDir(base, func(path string, d fs.DirEntry, err error) error {
			if err != nil {
				return err
			}
			rel, err := filepath.Rel(root, path)
			if err != nil {
				return err
			}
			if d.IsDir() {
				result[filepath.ToSlash(rel)+"/"] = ""
				return nil
			}
			data, err := os.ReadFile(path)
			if err != nil {
				return err
			}
			data = paths(data, root, false)
			result[filepath.ToSlash(rel)] = hex.EncodeToString(data)
			return nil
		}))
	}
	return result
}
func main() {
	root, err := os.Getwd()
	must(err)
	var input struct {
		StateHex  *string `json:"state_hex"`
		MarkerHex *string `json:"marker_hex"`
	}
	must(json.NewDecoder(os.Stdin).Decode(&input))
	for _, dir := range []string{"source", "destination", "home"} {
		must(os.MkdirAll(filepath.Join(root, dir), 0700))
	}
	must(os.WriteFile(filepath.Join(root, "source/config.toml"), []byte("source config\n"), 0600))
	must(os.WriteFile(filepath.Join(root, "destination/config.toml"), []byte("destination sentinel\n"), 0600))
	for name, encoded := range map[string]*string{"destination/.migration-state.json": input.StateHex, "backup/.complete.json": input.MarkerHex} {
		if encoded == nil {
			continue
		}
		data, err := hex.DecodeString(*encoded)
		must(err)
		data = paths(data, root, true)
		path := filepath.Join(root, name)
		must(os.MkdirAll(filepath.Dir(path), 0700))
		must(os.WriteFile(path, data, 0600))
	}
	before := snapshot(root)
	report, runErr := migration.Run(context.Background(), migration.Options{
		SourceRoot: filepath.Join(root, "source"), DestinationRoot: filepath.Join(root, "destination"),
		BackupDir: filepath.Join(root, "backup"), HomeDir: filepath.Join(root, "home"),
		Platform: scheduler.PlatformCron, BinaryPath: filepath.Join(root, "bin/symeraseme"), ProjectDir: root,
	})
	errorText := ""
	if runErr != nil {
		errorText = runErr.Error()
	}
	statuses := []string{}
	for _, item := range report.Items {
		statuses = append(statuses, item.Status)
	}
	must(json.NewEncoder(os.Stdout).Encode(map[string]any{
		"error": errorText, "resumed": report.Resumed, "complete": report.Complete, "statuses": statuses,
		"backup_reported": report.BackupDir != "", "before": before, "after": snapshot(root),
	}))
}

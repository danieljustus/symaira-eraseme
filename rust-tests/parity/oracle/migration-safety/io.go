//go:build ignore

// Probe the production migration entrypoint in a caller-owned empty root.
package main

import (
	"context"
	"encoding/json"
	"fmt"
	"os"
	"path/filepath"
	"runtime"

	"github.com/danieljustus/symaira-eraseme/internal/migration"
)

func must(err error) {
	if err != nil {
		panic(err)
	}
}

func main() {
	if len(os.Args) != 3 || runtime.GOOS == "windows" {
		panic("requires a Unix scenario and an isolated runtime root")
	}
	id, root := os.Args[1], os.Args[2]
	opts := migration.Options{
		SourceRoot: filepath.Join(root, "source"), DestinationRoot: filepath.Join(root, "destination"),
		HomeDir: filepath.Join(root, "home"), Platform: "systemd",
		BinaryPath: filepath.Join(root, "bin", "symeraseme"), ProjectDir: filepath.Join(root, "project"),
	}
	must(os.MkdirAll(opts.SourceRoot, 0o700))
	must(os.MkdirAll(opts.HomeDir, 0o700))
	unit := filepath.Join(opts.SourceRoot, "schedules", "symeraseme-tick.service")
	if id == "native-unreadable" || id == "native-directory" {
		unit = filepath.Join(opts.HomeDir, ".config", "systemd", "user", "symeraseme-tick.service")
	}
	must(os.MkdirAll(filepath.Dir(unit), 0o700))
	isDirectory := id == "generated-directory" || id == "native-directory"
	if isDirectory {
		must(os.Mkdir(unit, 0o700))
	} else if id == "generated-unreadable" || id == "native-unreadable" {
		must(os.WriteFile(unit, []byte("ExecStart=python3 -m symeraseme tick\n"), 0o600))
		must(os.Chmod(unit, 0))
		if _, err := os.ReadFile(unit); err == nil {
			panic("unreadable fixture is readable; run as a non-root user")
		}
	} else {
		panic("unknown scenario")
	}
	report, err := migration.Run(context.Background(), opts)
	errorText := ""
	if err != nil {
		errorText = err.Error()
	}
	info, statErr := os.Stat(unit)
	must(statErr)
	mode := uint32(info.Mode().Perm())
	contents := ""
	if !isDirectory {
		must(os.Chmod(unit, 0o600))
		data, readErr := os.ReadFile(unit)
		must(readErr)
		contents = string(data)
		must(os.Chmod(unit, info.Mode().Perm()))
	}
	_, destinationErr := os.Stat(opts.DestinationRoot)
	_, backupErr := os.Stat(opts.DestinationRoot + ".migration-backup")
	if destinationErr != nil && !os.IsNotExist(destinationErr) || backupErr != nil && !os.IsNotExist(backupErr) {
		panic("unexpected stat error")
	}
	out := map[string]any{
		"error": errorText, "report": report, "scheduler_contents": contents,
		"scheduler_mode": mode, "scheduler_is_directory": info.IsDir(),
		"destination_exists": destinationErr == nil, "backup_exists": backupErr == nil,
	}
	if err := json.NewEncoder(os.Stdout).Encode(out); err != nil {
		panic(fmt.Sprintf("encode observation: %v", err))
	}
}

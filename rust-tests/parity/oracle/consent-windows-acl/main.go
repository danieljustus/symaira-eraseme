// Generate real Go consent files in an explicitly supplied disposable root.
package main

import (
	"bytes"
	"crypto/rand"
	"crypto/sha256"
	"encoding/hex"
	"encoding/json"
	"errors"
	"os"
	"path/filepath"
	"runtime"
	"sort"

	"github.com/danieljustus/symaira-eraseme/internal/identity"
)

func main() {
	if len(os.Args) != 3 {
		panic("explicit consent directory and case required")
	}
	directory, mode := os.Args[1], os.Args[2]
	rand.Reader = bytes.NewReader(bytes.Repeat([]byte{7}, 1024))
	identity.SetNowFunc(func() int64 { return 1000 })
	_, err := identity.IssueTokenInDir(directory, "before", 60)
	if err != nil {
		panic(err)
	}
	if mode == "readonly-token" {
		entries, readErr := os.ReadDir(directory)
		if readErr != nil {
			panic(readErr)
		}
		for _, entry := range entries {
			if entry.Name() != "sentinel" {
				if err := os.Chmod(filepath.Join(directory, entry.Name()), 0o400); err != nil {
					panic(err)
				}
			}
		}
		_, err = identity.IssueTokenInDir(directory, "after", 60)
	}
	class := "ok"
	if errors.Is(err, os.ErrPermission) {
		class = "permission"
	} else if err != nil {
		class = "other"
	}
	entries, readErr := os.ReadDir(directory)
	if readErr != nil {
		panic(readErr)
	}
	files := []string{}
	for _, entry := range entries {
		files = append(files, entry.Name())
	}
	sort.Strings(files)
	sources := map[string]string{}
	for _, source := range []string{"go.mod", "internal/identity/consent.go", "internal/identity/gate.go", "rust-tests/parity/oracle/consent-windows-acl/main.go"} {
		body, err := os.ReadFile(filepath.Join(os.Getenv("SYMERASEME_ORACLE_SOURCE_ROOT"), source))
		if err != nil {
			panic(err)
		}
		digest := sha256.Sum256(body)
		sources[source] = hex.EncodeToString(digest[:])
	}
	observation := struct {
		Schema    string            `json:"schema"`
		Platform  string            `json:"platform"`
		GoVersion string            `json:"go_version"`
		Sources   map[string]string `json:"sources_sha256"`
		Failed    bool              `json:"failed"`
		Class     string            `json:"error_class"`
		Files     []string          `json:"files"`
	}{"symeraseme.go-oracle.consent-windows-acl.v1", runtime.GOOS + "/" + runtime.GOARCH, runtime.Version(), sources, err != nil, class, files}
	if err := json.NewEncoder(os.Stdout).Encode(observation); err != nil {
		panic(err)
	}
}

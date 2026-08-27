// Package registry implements the Go port of the broker registry loader
// (issue #712). It decodes broker YAML documents against the pinned
// Registry Data Contract (schema_version 1, see docs/registry-contract.md),
// validates them, supports embedding the registry into the binary and
// syncing it over HTTPS (replacing the Python git-pull design, issue #700).
package registry

import (
	"fmt"
	"io/fs"
	"os"
	"path/filepath"
	"sort"
	"strings"
)

// embeddedRegistry is populated from the repo-root registry directory via
// regdata.go (package symeraseme) — go:embed cannot reach outside the
// package directory, so the embed directive lives at the root and is handed
// over through SetEmbedded.
var embedded fs.FS

// SetEmbedded registers the embedded registry filesystem (called from the
// root package's regdata.go).
func SetEmbedded(f fs.FS) { embedded = f }

// ErrNoEmbeddedRegistry is returned when no embedded registry was registered.
var ErrNoEmbeddedRegistry = fmt.Errorf("registry: no embedded registry available")

// LoadEmbedded loads all broker documents from the embedded registry.
func LoadEmbedded() ([]Broker, error) {
	if embedded == nil {
		return nil, ErrNoEmbeddedRegistry
	}
	return Load(embedded)
}

// LoadFromDir loads all broker documents from a directory on disk
// (filesystem override for development and locally maintained registries).
func LoadFromDir(root string) ([]Broker, error) {
	return Load(os.DirFS(root))
}

// Load walks root for broker YAML files under brokers/<jurisdiction>/*.yaml,
// skipping documentation-only files starting with "_", decodes and validates
// each document, and returns brokers sorted by id. The first validation
// error aborts; use LoadReporting to collect all errors.
func Load(root fs.FS) ([]Broker, error) {
	brokers, errs := LoadReporting(root)
	if len(errs) > 0 {
		return nil, errs[0]
	}
	return brokers, nil
}

// LoadReporting behaves like Load but collects all validation errors
// instead of failing on the first document.
func LoadReporting(root fs.FS) (brokers []Broker, errs []error) {
	docs, err := collectDocs(root)
	if err != nil {
		return nil, []error{err}
	}
	ids := make([]string, 0, len(docs))
	for id := range docs {
		ids = append(ids, id)
	}
	sort.Strings(ids)
	for _, id := range ids {
		b, err := decodeAndValidate(docs[id])
		if err != nil {
			errs = append(errs, fmt.Errorf("registry: broker %q: %w", id, err))
			continue
		}
		brokers = append(brokers, b)
	}
	return brokers, errs
}

// doc captures a raw YAML document before decoding.
type doc struct {
	id      string
	path    string
	content []byte
}

// collectDocs walks the registry filesystem for broker documents.
func collectDocs(root fs.FS) (map[string]*doc, error) {
	docs := map[string]*doc{}
	walk := func(p string, d fs.DirEntry, err error) error {
		if err != nil {
			return err
		}
		if d.IsDir() {
			return nil
		}
		if !strings.HasSuffix(p, ".yaml") && !strings.HasSuffix(p, ".yml") {
			return nil
		}
		rel := strings.TrimPrefix(p, "registry/")
		parts := strings.Split(rel, "/")
		if len(parts) < 3 || parts[0] != "brokers" {
			return nil
		}
		if strings.HasPrefix(d.Name(), "_") {
			return nil // documentation-only (contract §2)
		}
		content, err := fs.ReadFile(root, p)
		if err != nil {
			return fmt.Errorf("registry: read %s: %w", p, err)
		}
		stem := strings.TrimSuffix(filepath.Base(p), filepath.Ext(p))
		docs[stem] = &doc{id: stem, path: p, content: content}
		return nil
	}
	if err := fs.WalkDir(root, ".", walk); err != nil {
		return nil, fmt.Errorf("registry: walk: %w", err)
	}
	return docs, nil
}

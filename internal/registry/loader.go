// Package registry implements the Go port of the broker registry loader
// (issue #712). It decodes broker YAML documents against the pinned
// Registry Data Contract (schema_version 1, see docs/registry-contract.md),
// validates them, supports embedding the registry into the binary and
// syncing it over HTTPS (replacing the Python git-pull design, issue #700).
package registry

import (
	"encoding/json"
	"fmt"
	"io"
	"io/fs"
	"os"
	"path/filepath"
	"sort"
	"strings"
)

const (
	maxDocumentBytes               = 1 << 20
	maxYAMLNodes                   = 16_384
	maxYAMLDepth                   = 64
	maxDirectoryDepth              = 8
	maxDirectoryEntries            = 16_384
	maxBrokerFiles                 = 4_096
	maxOutputBrokers               = 4_096
	maxAggregateInputBytes         = 16 << 20
	maxAggregateYAMLNodes          = 1 << 20
	maxMetadataBytes               = 64 << 10
	supportedRegistrySchemaVersion = 1
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
	handle, err := os.OpenRoot(root)
	if err != nil {
		return nil, fmt.Errorf("registry: open root: %w", err)
	}
	defer handle.Close()
	return Load(openedRootFS{FS: handle.FS(), root: handle})
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
	if err := validateRegistryMetadata(root); err != nil {
		return nil, []error{err}
	}
	docs, err := collectDocs(root)
	if err != nil {
		return nil, []error{err}
	}
	ids := make([]string, 0, len(docs))
	for id := range docs {
		ids = append(ids, id)
	}
	sort.Strings(ids)
	var inputBytes, yamlNodes int
	for _, id := range ids {
		d := docs[id]
		if d.content == nil {
			content, n, readErr := readBounded(root, d.path, inputBytes)
			if readErr != nil {
				return nil, []error{readErr}
			}
			d.content = content
			inputBytes += n
		}
		b, nodes, decodeErr := decodeAndValidateMetrics(d)
		if decodeErr != nil {
			errs = append(errs, fmt.Errorf("registry: broker %q: %w", id, decodeErr))
			continue
		}
		yamlNodes += nodes
		if yamlNodes > maxAggregateYAMLNodes {
			return nil, []error{verr("aggregate YAML node limit %d exceeded", maxAggregateYAMLNodes)}
		}
		if len(brokers) >= maxOutputBrokers {
			return nil, []error{verr("output broker limit %d exceeded", maxOutputBrokers)}
		}
		brokers = append(brokers, b)
	}
	return brokers, errs
}

type registryManifest struct {
	SchemaVersion *int `json:"schema_version"`
	Schemas       *struct {
		Broker *string `json:"broker"`
	} `json:"schemas"`
}

type registrySchemaMetadata struct {
	SchemaVersion *int `json:"schema_version"`
}

const brokerSchemaPath = "schemas/broker.schema.json"

func validateRegistryMetadata(root fs.FS) error {
	manifestBytes, err := readMetadata(root, "manifest.json")
	if err != nil {
		return fmt.Errorf("registry: manifest.json: %w", err)
	}
	var manifest registryManifest
	if err := json.Unmarshal(manifestBytes, &manifest); err != nil {
		return verr("manifest.json is malformed: %v", err)
	}
	if manifest.SchemaVersion == nil || *manifest.SchemaVersion != supportedRegistrySchemaVersion {
		return verr("manifest.json schema_version must be supported version %d", supportedRegistrySchemaVersion)
	}
	if manifest.Schemas == nil || manifest.Schemas.Broker == nil {
		return verr("manifest.json schemas.broker is required")
	}
	if *manifest.Schemas.Broker != brokerSchemaPath {
		return verr("manifest.json schemas.broker must be %q", brokerSchemaPath)
	}
	schemaBytes, err := readMetadata(root, *manifest.Schemas.Broker)
	if err != nil {
		return fmt.Errorf("registry: broker schema: %w", err)
	}
	var schema registrySchemaMetadata
	if err := json.Unmarshal(schemaBytes, &schema); err != nil {
		return verr("broker schema is malformed: %v", err)
	}
	if schema.SchemaVersion == nil || *schema.SchemaVersion != supportedRegistrySchemaVersion {
		return verr("broker schema schema_version must be supported version %d", supportedRegistrySchemaVersion)
	}
	if *schema.SchemaVersion != *manifest.SchemaVersion {
		return verr("manifest and broker schema schema_version mismatch")
	}
	return nil
}

func readMetadata(root fs.FS, path string) ([]byte, error) {
	if !fs.ValidPath(path) {
		return nil, verr("unsafe metadata path %q", path)
	}
	var (
		file fs.File
		err  error
	)
	if opener, ok := root.(interface {
		OpenFile(string, int, os.FileMode) (*os.File, error)
	}); ok {
		file, err = opener.OpenFile(path, os.O_RDONLY|registryOpenNonblock, 0)
	} else {
		file, err = root.Open(path)
	}
	if err != nil {
		return nil, err
	}
	defer file.Close()
	info, err := file.Stat()
	if err != nil {
		return nil, err
	}
	if !info.Mode().IsRegular() {
		return nil, verr("metadata file is not regular: %s", path)
	}
	if info.Size() > maxMetadataBytes {
		return nil, verr("metadata file %s exceeds %d bytes", path, maxMetadataBytes)
	}
	data, err := io.ReadAll(io.LimitReader(file, maxMetadataBytes+1))
	if err != nil {
		return nil, err
	}
	if len(data) > maxMetadataBytes {
		return nil, verr("metadata file %s exceeds %d bytes", path, maxMetadataBytes)
	}
	return data, nil
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
	entries := 0
	files := 0
	walk := func(p string, d fs.DirEntry, err error) error {
		if err != nil {
			return err
		}
		entries++
		if entries > maxDirectoryEntries {
			return verr("directory entry limit %d exceeded", maxDirectoryEntries)
		}
		if strings.Count(strings.TrimPrefix(p, "./"), "/") > maxDirectoryDepth+1 {
			return verr("directory nesting exceeds %d levels", maxDirectoryDepth)
		}
		if d.IsDir() {
			return nil
		}
		if !strings.HasSuffix(p, ".yaml") && !strings.HasSuffix(p, ".yml") {
			return nil
		}
		if d.Type()&os.ModeSymlink != 0 {
			return verr("symlink is not allowed: %s", p)
		}
		info, infoErr := d.Info()
		if infoErr != nil {
			return fmt.Errorf("registry: inspect %s: %w", p, infoErr)
		}
		if !info.Mode().IsRegular() {
			return verr("YAML entry is not a regular file: %s", p)
		}
		rel := strings.TrimPrefix(p, "registry/")
		parts := strings.Split(rel, "/")
		if len(parts) < 3 || parts[0] != "brokers" {
			return nil
		}
		if strings.HasPrefix(d.Name(), "_") {
			return nil // documentation-only (contract §2)
		}
		files++
		if files > maxBrokerFiles {
			return verr("broker YAML file limit %d exceeded", maxBrokerFiles)
		}
		stem := strings.TrimSuffix(filepath.Base(p), filepath.Ext(p))
		if prior, exists := docs[stem]; exists {
			return verr("duplicate broker id %q in %s and %s", stem, prior.path, p)
		}
		docs[stem] = &doc{id: stem, path: p}
		return nil
	}
	if err := fs.WalkDir(root, ".", walk); err != nil {
		return nil, fmt.Errorf("registry: walk: %w", err)
	}
	return docs, nil
}

func readBounded(root fs.FS, path string, inputBytes int) ([]byte, int, error) {
	if inputBytes >= maxAggregateInputBytes {
		return nil, 0, verr("aggregate input byte limit %d exceeded", maxAggregateInputBytes)
	}
	if lstat, ok := root.(interface {
		Lstat(string) (os.FileInfo, error)
	}); ok {
		info, err := lstat.Lstat(path)
		if err != nil {
			return nil, 0, fmt.Errorf("registry: inspect %s: %w", path, err)
		}
		if info.Mode()&os.ModeSymlink != 0 {
			return nil, 0, verr("symlink is not allowed: %s", path)
		}
		if !info.Mode().IsRegular() {
			return nil, 0, verr("YAML entry is not a regular file: %s", path)
		}
	}
	var (
		file fs.File
		err  error
	)
	if opener, ok := root.(interface {
		OpenFile(string, int, os.FileMode) (*os.File, error)
	}); ok {
		file, err = opener.OpenFile(path, os.O_RDONLY|registryOpenNonblock, 0)
	} else {
		file, err = root.Open(path)
	}
	if err != nil {
		return nil, 0, fmt.Errorf("registry: read %s: %w", path, err)
	}
	defer file.Close()
	if lstat, ok := root.(interface {
		Lstat(string) (os.FileInfo, error)
	}); ok {
		info, err := lstat.Lstat(path)
		if err != nil {
			return nil, 0, fmt.Errorf("registry: inspect %s: %w", path, err)
		}
		if info.Mode()&os.ModeSymlink != 0 {
			return nil, 0, verr("symlink is not allowed: %s", path)
		}
	}
	info, err := file.Stat()
	if err != nil {
		return nil, 0, fmt.Errorf("registry: inspect %s: %w", path, err)
	}
	if !info.Mode().IsRegular() {
		return nil, 0, verr("YAML entry is not a stable regular file: %s", path)
	}
	remaining := maxAggregateInputBytes - inputBytes
	limit := maxDocumentBytes
	if remaining < limit {
		limit = remaining
	}
	if info.Size() > int64(limit) {
		if remaining < maxDocumentBytes {
			return nil, 0, verr("aggregate input byte limit %d exceeded", maxAggregateInputBytes)
		}
		return nil, 0, verr("document %s exceeds %d bytes", path, maxDocumentBytes)
	}
	data, err := io.ReadAll(io.LimitReader(file, int64(limit)+1))
	if err != nil {
		return nil, 0, fmt.Errorf("registry: read %s: %w", path, err)
	}
	if len(data) > limit {
		if remaining < maxDocumentBytes {
			return nil, 0, verr("aggregate input byte limit %d exceeded", maxAggregateInputBytes)
		}
		return nil, 0, verr("document %s exceeds %d bytes", path, maxDocumentBytes)
	}
	return data, len(data), nil
}

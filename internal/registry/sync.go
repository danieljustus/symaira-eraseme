package registry

import (
	"archive/tar"
	"compress/gzip"
	"context"
	"fmt"
	"io"
	"net/http"
	"net/netip"
	"net/url"
	"os"
	"path/filepath"
	"strings"
	"time"
)

// DefaultSyncURL is the release artefact URL for registry sync. Production
// callers must use HTTPS; loopback HTTP is accepted only for injected test
// servers.
const DefaultSyncURL = "https://github.com/danieljustus/symaira-eraseme/releases/latest/download/registry.tar.gz"

const (
	syncTimeout           = 60 * time.Second
	syncMaxCompressed     = 64 << 20
	syncMaxExpanded       = 64 << 20
	syncMaxFile           = 1 << 20
	syncMaxFiles          = 4096
	syncMaxArchiveEntries = 8192
	syncMaxPathBytes      = 4096
)

var (
	httpClient    = &http.Client{Timeout: syncTimeout}
	renamePath    = os.Rename
	removeAllPath = os.RemoveAll
)

// Sync downloads, validates, and atomically installs a registry archive.
// Until validation succeeds, dst is not modified. A failed replacement restores
// the previous destination from its same-filesystem backup.
func Sync(ctx context.Context, dst string, rawURL string) error {
	if rawURL == "" {
		rawURL = DefaultSyncURL
	}
	parsed, err := url.Parse(rawURL)
	if err != nil || parsed.Scheme == "" || parsed.Host == "" {
		return fmt.Errorf("registry sync: invalid URL")
	}
	if parsed.Scheme != "https" && !loopbackHTTP(parsed) {
		return fmt.Errorf("registry sync: HTTPS is required")
	}

	rootPath, err := filepath.Abs(dst)
	if err != nil {
		return fmt.Errorf("registry sync: resolve destination: %w", err)
	}
	parent := filepath.Dir(rootPath)
	if err := os.MkdirAll(parent, 0o700); err != nil {
		return fmt.Errorf("registry sync: create destination parent: %w", err)
	}
	stage, err := os.MkdirTemp(parent, ".registry-sync-")
	if err != nil {
		return fmt.Errorf("registry sync: create staging directory: %w", err)
	}
	stageReady := true
	defer func() {
		if stageReady {
			_ = removeAllPath(stage)
		}
	}()
	if err := os.Chmod(stage, 0o700); err != nil {
		return fmt.Errorf("registry sync: secure staging directory: %w", err)
	}

	reqCtx, cancel := context.WithTimeout(ctx, syncTimeout)
	defer cancel()
	req, err := http.NewRequestWithContext(reqCtx, http.MethodGet, rawURL, nil)
	if err != nil {
		return fmt.Errorf("registry sync: request: %w", err)
	}
	resp, err := httpClient.Do(req)
	if err != nil {
		return fmt.Errorf("registry sync: download: %w", err)
	}
	defer resp.Body.Close()
	if resp.StatusCode != http.StatusOK {
		return fmt.Errorf("registry sync: download: HTTP %d", resp.StatusCode)
	}

	counted := &countingReader{reader: resp.Body}
	limited := io.LimitReader(counted, syncMaxCompressed+1)
	gz, err := gzip.NewReader(limited)
	if err != nil {
		return fmt.Errorf("registry sync: gzip: malformed archive")
	}
	if err := extractArchive(gz, stage); err != nil {
		_ = gz.Close()
		return err
	}
	if err := gz.Close(); err != nil {
		return fmt.Errorf("registry sync: gzip: malformed archive")
	}
	if _, err := io.Copy(io.Discard, limited); err != nil {
		return fmt.Errorf("registry sync: download body read failed")
	}
	// Drain trailing response bytes so the compressed-body bound also covers
	// bytes after a valid gzip member.
	if counted.n > syncMaxCompressed {
		return fmt.Errorf("registry sync: compressed archive limit %d bytes exceeded", syncMaxCompressed)
	}

	if _, err := VerifySynced(stage); err != nil {
		return fmt.Errorf("registry sync: staged registry validation failed: %w", err)
	}
	if err := replaceDirectory(rootPath, stage); err != nil {
		return err
	}
	stageReady = false
	return nil
}

func loopbackHTTP(parsed *url.URL) bool {
	if parsed.Scheme != "http" {
		return false
	}
	host := parsed.Hostname()
	if host == "localhost" {
		return true
	}
	addr, err := netip.ParseAddr(host)
	return err == nil && addr.IsLoopback()
}

type countingReader struct {
	reader io.Reader
	n      int64
}

func (r *countingReader) Read(p []byte) (int, error) {
	n, err := r.reader.Read(p)
	r.n += int64(n)
	return n, err
}

type archiveBudget struct {
	entries int
	files   int
	bytes   int64
}

func extractArchive(reader io.Reader, stage string) error {
	root, err := os.OpenRoot(stage)
	if err != nil {
		return fmt.Errorf("registry sync: open staging root: %w", err)
	}
	defer root.Close()
	budget := archiveBudget{}
	tr := tar.NewReader(reader)
	for {
		hdr, err := tr.Next()
		if err == io.EOF {
			break
		}
		if err != nil {
			return fmt.Errorf("registry sync: tar: malformed archive")
		}
		budget.entries++
		if budget.entries > syncMaxArchiveEntries {
			return fmt.Errorf("registry sync: archive entry limit %d exceeded", syncMaxArchiveEntries)
		}
		name, err := safeArchiveName(hdr.Name)
		if err != nil {
			return fmt.Errorf("registry sync: archive entry rejected: %w", err)
		}
		switch hdr.Typeflag {
		case tar.TypeDir:
			if hdr.Size != 0 {
				return fmt.Errorf("registry sync: directory entry has data")
			}
			if err := root.MkdirAll(name, 0o700); err != nil {
				return fmt.Errorf("registry sync: create directory: %w", err)
			}
		case tar.TypeReg:
			if hdr.Size < 0 || hdr.Size > syncMaxFile {
				return fmt.Errorf("registry sync: file size limit %d bytes exceeded", syncMaxFile)
			}
			budget.files++
			if budget.files > syncMaxFiles {
				return fmt.Errorf("registry sync: file limit %d exceeded", syncMaxFiles)
			}
			if budget.bytes > syncMaxExpanded-hdr.Size {
				return fmt.Errorf("registry sync: expanded archive limit %d bytes exceeded", syncMaxExpanded)
			}
			if err := root.MkdirAll(filepath.Dir(name), 0o700); err != nil {
				return fmt.Errorf("registry sync: create parent directory: %w", err)
			}
			file, err := root.OpenFile(name, os.O_WRONLY|os.O_CREATE|os.O_EXCL, 0o600)
			if err != nil {
				return fmt.Errorf("registry sync: create archive file: %w", err)
			}
			_, copyErr := io.CopyN(file, tr, hdr.Size)
			closeErr := file.Close()
			if copyErr != nil {
				return fmt.Errorf("registry sync: read archive file: %w", copyErr)
			}
			if closeErr != nil {
				return fmt.Errorf("registry sync: close archive file: %w", closeErr)
			}
			budget.bytes += hdr.Size
		default:
			return fmt.Errorf("registry sync: unsupported archive entry type")
		}
	}
	return nil
}

func safeArchiveName(raw string) (string, error) {
	if raw == "" || len(raw) > syncMaxPathBytes || strings.IndexByte(raw, 0) >= 0 {
		return "", fmt.Errorf("invalid archive path")
	}
	normalized := filepath.FromSlash(strings.ReplaceAll(raw, "\\", "/"))
	if filepath.IsAbs(normalized) || filepath.VolumeName(normalized) != "" {
		return "", fmt.Errorf("absolute archive path")
	}
	parts := strings.Split(filepath.ToSlash(normalized), "/")
	clean := make([]string, 0, len(parts))
	for _, part := range parts {
		if part == "" {
			continue
		}
		if part == "." || part == ".." {
			return "", fmt.Errorf("traversal archive path")
		}
		clean = append(clean, part)
	}
	if len(clean) == 0 {
		return "", fmt.Errorf("empty archive path")
	}
	return filepath.Join(clean...), nil
}

func replaceDirectory(dst, stage string) error {
	backup := dst + ".registry-backup"
	_ = removeAllPath(backup)
	hadOld := false
	if _, err := os.Lstat(dst); err == nil {
		if err := renamePath(dst, backup); err != nil {
			return fmt.Errorf("registry sync: preserve old destination: %w", err)
		}
		hadOld = true
	} else if !os.IsNotExist(err) {
		return fmt.Errorf("registry sync: inspect destination: %w", err)
	}
	if err := renamePath(stage, dst); err != nil {
		if hadOld {
			_ = renamePath(backup, dst)
		}
		return fmt.Errorf("registry sync: install staged registry: %w", err)
	}
	if hadOld {
		if err := removeAllPath(backup); err != nil {
			// Installation succeeded; retaining the backup is safer than claiming
			// cleanup succeeded and does not affect the installed registry.
			return fmt.Errorf("registry sync: remove replacement backup: %w", err)
		}
	}
	return nil
}

// VerifySynced loads a synced directory through the same loader/validator.
func VerifySynced(dst string) (int, error) {
	brokers, err := LoadFromDir(dst)
	if err != nil {
		return 0, err
	}
	return len(brokers), nil
}

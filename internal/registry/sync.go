package registry

import (
	"archive/tar"
	"compress/gzip"
	"context"
	"fmt"
	"io"
	"io/fs"
	"net/http"
	"os"
	"path/filepath"
	"strings"
	"time"
)

// DefaultSyncURL is the release-artefact base URL for registry sync
// (issue #700: fetch over HTTPS, never git pull).
const DefaultSyncURL = "https://github.com/danieljustus/symaira-eraseme/releases/latest/download/registry.tar.gz"

// httpClient is the shared sync HTTP client.
var httpClient = &http.Client{Timeout: 60 * time.Second}

// Sync downloads the registry archive from url (default DefaultSyncURL) and
// unpacks it into dst (which is created if missing). NO git commands — the
// Python git-pull design (bug #700) is deliberately not reproduced.
func Sync(ctx context.Context, dst string, url string) error {
	if url == "" {
		url = DefaultSyncURL
	}
	if err := os.MkdirAll(dst, 0o755); err != nil {
		return fmt.Errorf("registry sync: create dst: %w", err)
	}
	root, err := filepath.Abs(dst)
	if err != nil {
		return fmt.Errorf("registry sync: resolve dst: %w", err)
	}
	req, err := http.NewRequestWithContext(ctx, http.MethodGet, url, nil)
	if err != nil {
		return fmt.Errorf("registry sync: %w", err)
	}
	resp, err := httpClient.Do(req)
	if err != nil {
		return fmt.Errorf("registry sync: download: %w", err)
	}
	defer resp.Body.Close()
	if resp.StatusCode != http.StatusOK {
		return fmt.Errorf("registry sync: download: HTTP %d", resp.StatusCode)
	}

	gz, err := gzip.NewReader(resp.Body)
	if err != nil {
		return fmt.Errorf("registry sync: gzip: %w", err)
	}
	defer gz.Close()

	tr := tar.NewReader(gz)
	for {
		hdr, err := tr.Next()
		if err == io.EOF {
			break
		}
		if err != nil {
			return fmt.Errorf("registry sync: tar: %w", err)
		}
		// Guard against absolute paths and traversal outside the destination.
		name := filepath.Clean(filepath.FromSlash(hdr.Name))
		if filepath.IsAbs(name) {
			continue
		}
		target := filepath.Join(root, name)
		relative, err := filepath.Rel(root, target)
		if err != nil || relative == ".." || strings.HasPrefix(relative, ".."+string(filepath.Separator)) {
			continue
		}
		switch hdr.Typeflag {
		case tar.TypeDir:
			if err := os.MkdirAll(target, 0o755); err != nil {
				return fmt.Errorf("registry sync: mkdir %s: %w", target, err)
			}
		case tar.TypeReg:
			if err := os.MkdirAll(filepath.Dir(target), 0o755); err != nil {
				return fmt.Errorf("registry sync: mkdir: %w", err)
			}
			f, err := os.OpenFile(target, os.O_CREATE|os.O_TRUNC|os.O_WRONLY, os.FileMode(hdr.Mode)&0o755)
			if err != nil {
				return fmt.Errorf("registry sync: write %s: %w", target, err)
			}
			if _, err := io.Copy(f, tr); err != nil {
				f.Close()
				return fmt.Errorf("registry sync: copy %s: %w", target, err)
			}
			f.Close()
		}
	}
	return nil
}

// VerifySynced loads a synced directory through the same loader/validator —
// a sync is only done when its result loads.
func VerifySynced(dst string) (int, error) {
	brokers, err := LoadFromDir(dst)
	if err != nil {
		return 0, err
	}
	return len(brokers), nil
}

// silence unused import when fs is not otherwise used
var _ fs.FS

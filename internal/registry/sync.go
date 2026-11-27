package registry

import (
	"archive/tar"
	"bufio"
	"compress/gzip"
	"context"
	"errors"
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
	syncMaxRedirects      = 10
	syncMaxCompressed     = 64 << 20
	syncMaxExpanded       = 64 << 20
	syncMaxFile           = 1 << 20
	syncMaxFiles          = 4096
	syncMaxArchiveEntries = 8192
	syncMaxPathBytes      = 4096
)

// Sync downloads, validates, and atomically installs a registry archive.
// Until validation succeeds, dst is not modified. A failed replacement restores
// the previous destination from its same-filesystem backup.
func Sync(ctx context.Context, dst string, rawURL string) error {
	if rawURL == "" {
		rawURL = DefaultSyncURL
	}
	parsed, err := parseSyncURL(rawURL)
	if err != nil {
		return fmt.Errorf("registry sync: invalid URL")
	}
	loopbackTest := loopbackHTTP(parsed)
	if err := validateSyncURL(parsed, loopbackTest); err != nil {
		return fmt.Errorf("registry sync: %w", err)
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
			_ = os.RemoveAll(stage)
		}
	}()
	if err := os.Chmod(stage, 0o700); err != nil {
		return fmt.Errorf("registry sync: secure staging directory: %w", err)
	}

	reqCtx, cancel := context.WithTimeout(ctx, syncTimeout)
	defer cancel()
	req, err := http.NewRequestWithContext(reqCtx, http.MethodGet, rawURL, nil)
	if err != nil {
		// Do not return the request error: net/url errors can echo userinfo.
		return fmt.Errorf("registry sync: invalid URL")
	}
	client := newSyncHTTPClient(loopbackTest)
	resp, err := client.Do(req)
	if err != nil {
		// Do not wrap *url.Error: it may include a credential-bearing URL.
		return fmt.Errorf("registry sync: download failed")
	}
	defer resp.Body.Close()
	if resp.StatusCode != http.StatusOK {
		return fmt.Errorf("registry sync: download: HTTP %d", resp.StatusCode)
	}

	counted := &countingReader{reader: resp.Body}
	limited := io.LimitReader(counted, syncMaxCompressed+1)
	compressed := bufio.NewReader(limited)
	gz, err := gzip.NewReader(compressed)
	if err != nil {
		return fmt.Errorf("registry sync: gzip: malformed archive")
	}
	gz.Multistream(false)
	if err := extractArchive(gz, stage); err != nil {
		_ = gz.Close()
		return err
	}
	// Tar can stop at its end marker before gzip has consumed its checksum.
	// Drain the decoder so corrupt trailers are rejected deterministically.
	if _, err := io.Copy(io.Discard, gz); err != nil {
		_ = gz.Close()
		return fmt.Errorf("registry sync: gzip: malformed archive")
	}
	if err := gz.Close(); err != nil {
		return fmt.Errorf("registry sync: gzip: malformed archive")
	}
	// Multistream(false) leaves a second gzip member or raw compressed bytes in
	// the buffered reader. Neither is part of the canonical one-member format.
	trailingCompressed, err := io.ReadAll(compressed)
	if err != nil {
		return fmt.Errorf("registry sync: compressed body read failed")
	}
	if len(trailingCompressed) != 0 || counted.n > syncMaxCompressed {
		return fmt.Errorf("registry sync: compressed archive limit %d bytes exceeded or trailing bytes present", syncMaxCompressed)
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

func newSyncHTTPClient(loopbackTest bool) *http.Client {
	return &http.Client{
		Timeout: syncTimeout,
		CheckRedirect: func(next *http.Request, via []*http.Request) error {
			if len(via) >= syncMaxRedirects {
				return errors.New("redirect limit exceeded")
			}
			parsed, err := parseSyncURL(next.URL.String())
			if err != nil {
				return errors.New("invalid redirect URL")
			}
			if err := validateSyncURL(parsed, loopbackTest); err != nil {
				return errors.New("redirect URL rejected")
			}
			return nil
		},
	}
}

func parseSyncURL(raw string) (*url.URL, error) {
	if raw == "" || strings.ContainsAny(raw, "\\\x00") {
		return nil, errors.New("invalid URL")
	}
	parsed, err := url.Parse(raw)
	if err != nil || parsed.Scheme == "" || parsed.Host == "" || parsed.Opaque != "" || parsed.User != nil {
		return nil, errors.New("invalid URL")
	}
	if parsed.Scheme != "https" && parsed.Scheme != "http" {
		return nil, errors.New("invalid URL")
	}
	if parsed.Hostname() == "" || strings.ContainsAny(parsed.Host, "\r\n\x00") {
		return nil, errors.New("invalid URL")
	}
	// url.Parse rejects most malformed authorities, but explicitly require a
	// numeric port so the policy never relies on a later network dial failure.
	if port := parsed.Port(); port != "" {
		for _, character := range port {
			if character < '0' || character > '9' {
				return nil, errors.New("invalid URL")
			}
		}
	}
	return parsed, nil
}

func validateSyncURL(parsed *url.URL, loopbackTest bool) error {
	if parsed.Scheme == "https" {
		if loopbackTest && !isLoopbackHost(parsed.Hostname()) {
			return errors.New("redirect must remain on loopback")
		}
		return nil
	}
	if parsed.Scheme == "http" && loopbackTest && isLoopbackHost(parsed.Hostname()) {
		return nil
	}
	return errors.New("HTTPS is required")
}

func loopbackHTTP(parsed *url.URL) bool {
	return parsed.Scheme == "http" && isLoopbackHost(parsed.Hostname())
}

func isLoopbackHost(host string) bool {
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
	// Reject bytes after tar's two zero blocks rather than silently installing a
	// valid prefix. This also leaves gzip checksum verification to Sync.
	var trailing [1]byte
	if n, err := reader.Read(trailing[:]); n != 0 || (err != nil && err != io.EOF) {
		return fmt.Errorf("registry sync: decompressed bytes after tar end")
	} else if n != 0 {
		return fmt.Errorf("registry sync: decompressed bytes after tar end")
	}
	return nil
}

func safeArchiveName(raw string) (string, error) {
	if raw == "" || len(raw) > syncMaxPathBytes || strings.IndexByte(raw, 0) >= 0 {
		return "", fmt.Errorf("invalid archive path")
	}
	// Backslashes are rejected before any host-dependent normalization. This
	// closes Windows drive, UNC, and separator interpretations on every host.
	if strings.Contains(raw, "\\") || strings.HasPrefix(raw, "/") || windowsAbsolute(raw) {
		return "", fmt.Errorf("absolute or invalid archive path")
	}
	clean := make([]string, 0, strings.Count(raw, "/")+1)
	for _, part := range strings.Split(raw, "/") {
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

func windowsAbsolute(raw string) bool {
	return len(raw) >= 2 && ((raw[0] >= 'a' && raw[0] <= 'z') || (raw[0] >= 'A' && raw[0] <= 'Z')) && raw[1] == ':'
}

type replacementOps struct {
	rename    func(string, string) error
	removeAll func(string) error
}

func replaceDirectory(dst, stage string) error {
	return replaceDirectoryWithOps(dst, stage, replacementOps{
		rename:    os.Rename,
		removeAll: os.RemoveAll,
	})
}

func replaceDirectoryWithOps(dst, stage string, ops replacementOps) error {
	parent := filepath.Dir(dst)
	backupRoot, err := os.MkdirTemp(parent, ".registry-backup-")
	if err != nil {
		return fmt.Errorf("registry sync: create replacement backup: %w", err)
	}
	if err := os.Chmod(backupRoot, 0o700); err != nil {
		_ = ops.removeAll(backupRoot)
		return fmt.Errorf("registry sync: secure replacement backup: %w", err)
	}
	backup := filepath.Join(backupRoot, "old")
	cleanupBackup := func() error { return ops.removeAll(backupRoot) }
	hadOld := false
	if _, err := os.Lstat(dst); err == nil {
		if err := ops.rename(dst, backup); err != nil {
			cleanupErr := cleanupBackup()
			if cleanupErr != nil {
				return fmt.Errorf("registry sync: preserve old destination failed; backup cleanup failed: %w", errors.Join(err, cleanupErr))
			}
			return fmt.Errorf("registry sync: preserve old destination: %w", err)
		}
		hadOld = true
	} else if !os.IsNotExist(err) {
		_ = cleanupBackup()
		return fmt.Errorf("registry sync: inspect destination: %w", err)
	}
	if err := ops.rename(stage, dst); err != nil {
		if !hadOld {
			_ = cleanupBackup()
			return fmt.Errorf("registry sync: install staged registry: %w", err)
		}
		if rollbackErr := ops.rename(backup, dst); rollbackErr != nil {
			return fmt.Errorf("registry sync: install staged registry failed; rollback failed; backup retained at %s: %w", backupRoot, errors.Join(err, rollbackErr))
		}
		if cleanupErr := cleanupBackup(); cleanupErr != nil {
			return fmt.Errorf("registry sync: install staged registry failed; rollback succeeded; backup cleanup failed at %s: %w", backupRoot, errors.Join(err, cleanupErr))
		}
		return fmt.Errorf("registry sync: install staged registry: %w", err)
	}
	if err := cleanupBackup(); err != nil {
		return fmt.Errorf("registry sync: remove replacement backup: %w", err)
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

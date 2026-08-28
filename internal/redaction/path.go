package redaction

import (
	"errors"
	"fmt"
	"os"
	"path/filepath"
	"strings"

	"github.com/danieljustus/symaira-eraseme/internal/identity"
)

var (
	// ErrPathOutsideWorkspace is returned before opening a path that is not
	// contained by the canonical workspace root.
	ErrPathOutsideWorkspace = errors.New("Path is outside the MCP workspace")
	// ErrPathNullByte mirrors Python's explicit path validation.
	ErrPathNullByte = errors.New("Path contains a null byte")
	// ErrPathNotString is retained as a typed equivalent for callers that
	// validate decoded JSON before converting to a Go string.
	ErrPathNotString = errors.New("Path must be a string")
)

// ResolveWorkspacePath canonicalizes path and verifies that it remains inside
// workspaceRoot. If workspaceRoot is omitted, the current working directory
// is used, matching the Python MCP server. Existing symlinks are resolved, so
// a symlink inside the workspace pointing outside is rejected too.
func ResolveWorkspacePath(path string, workspaceRoots ...string) (string, error) {
	if strings.IndexByte(path, 0) >= 0 {
		return "", ErrPathNullByte
	}
	root := ""
	if len(workspaceRoots) > 0 {
		root = workspaceRoots[0]
	}
	if root == "" {
		var err error
		root, err = os.Getwd()
		if err != nil {
			return "", fmt.Errorf("resolve workspace root: %w", err)
		}
	}
	root, err := canonicalExistingPath(root)
	if err != nil {
		return "", fmt.Errorf("resolve workspace root: %w", err)
	}

	path = expandUser(path)
	candidate := path
	if !filepath.IsAbs(candidate) {
		candidate = filepath.Join(root, candidate)
	}
	candidate = filepath.Clean(candidate)
	candidate, err = canonicalPath(candidate)
	if err != nil {
		return "", err
	}

	rel, err := filepath.Rel(root, candidate)
	if err != nil || rel == ".." || strings.HasPrefix(rel, ".."+string(filepath.Separator)) {
		return "", ErrPathOutsideWorkspace
	}
	return candidate, nil
}

// ReadWorkspaceFile reads a UTF-8 file under the workspace boundary. It
// returns bytes rather than a string so callers can preserve file bytes that
// are not part of a replacement.
func ReadWorkspaceFile(path string, workspaceRoots ...string) ([]byte, error) {
	candidate, err := ResolveWorkspacePath(path, workspaceRoots...)
	if err != nil {
		return nil, err
	}
	return os.ReadFile(candidate) //nolint:gosec // canonicalized and confined above
}

// ReadWorkspaceText is the string-oriented counterpart to ReadWorkspaceFile.
func ReadWorkspaceText(path string, workspaceRoots ...string) (string, error) {
	content, err := ReadWorkspaceFile(path, workspaceRoots...)
	return string(content), err
}

// RedactFile reads and redacts a workspace file. An optional explicit root is
// accepted for servers and tests; without one, the current working directory
// is the workspace root.
func RedactFile(path string, workspaceRoots ...string) ([]byte, error) {
	content, err := ReadWorkspaceFile(path, workspaceRoots...)
	if err != nil {
		return nil, err
	}
	return []byte(Redact(string(content))), nil
}

// RedactFileText is the string-oriented counterpart to RedactFile.
func RedactFileText(path string, workspaceRoots ...string) (string, error) {
	content, err := RedactFile(path, workspaceRoots...)
	return string(content), err
}

// RedactFileWithProfile is RedactFile with an already-loaded identity profile.
func RedactFileWithProfile(path, workspaceRoot string, profile *identity.Profile) ([]byte, error) {
	content, err := ReadWorkspaceFile(path, workspaceRoot)
	if err != nil {
		return nil, err
	}
	return []byte(Redact(string(content), profile)), nil
}

func expandUser(path string) string {
	if path != "~" && !strings.HasPrefix(path, "~/") {
		return path
	}
	home, err := os.UserHomeDir()
	if err != nil {
		return path
	}
	if path == "~" {
		return home
	}
	return filepath.Join(home, path[2:])
}

// canonicalPath mirrors os.path.realpath for existing files while preserving
// the useful Python behavior of returning a normalized path for a missing
// final component, allowing the subsequent os.ReadFile to report not found.
func canonicalPath(path string) (string, error) {
	if resolved, err := filepath.EvalSymlinks(path); err == nil {
		return filepath.Clean(resolved), nil
	}
	abs, err := filepath.Abs(path)
	if err != nil {
		return "", err
	}
	parent, err := filepath.EvalSymlinks(filepath.Dir(abs))
	if err != nil {
		return filepath.Clean(abs), nil
	}
	return filepath.Join(parent, filepath.Base(abs)), nil
}

func canonicalExistingPath(path string) (string, error) {
	abs, err := filepath.Abs(path)
	if err != nil {
		return "", err
	}
	resolved, err := filepath.EvalSymlinks(abs)
	if err != nil {
		return "", err
	}
	info, err := os.Stat(resolved)
	if err != nil {
		return "", err
	}
	if !info.IsDir() {
		return "", fmt.Errorf("workspace root is not a directory")
	}
	return filepath.Clean(resolved), nil
}

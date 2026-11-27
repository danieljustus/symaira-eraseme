package redaction

import (
	"errors"
	"fmt"
	"io"
	"os"
	"path/filepath"
	"strings"
	"unicode"
	"unicode/utf8"

	"github.com/danieljustus/symaira-eraseme/internal/identity"
)

var (
	// ErrPathOutsideWorkspace is returned before opening a path that is not
	// contained by the canonical workspace root.
	ErrPathOutsideWorkspace = errors.New("path is outside the MCP workspace")
	// ErrPathNullByte mirrors Python's explicit path validation.
	ErrPathNullByte = errors.New("path contains a null byte")
	// ErrPathNotString is retained as a typed equivalent for callers that
	// validate decoded JSON before converting to a Go string.
	ErrPathNotString = errors.New("path must be a string")
	// ErrPathInvalid is returned for ambiguous or unsafe relative names.
	ErrPathInvalid = errors.New("path is not a safe workspace-relative file name")
	// ErrFileTooLarge prevents an unbounded read into memory.
	ErrFileTooLarge = errors.New("workspace file exceeds the maximum size")
	// ErrNotRegularFile rejects directories, FIFOs, devices and sockets.
	ErrNotRegularFile = errors.New("workspace target is not a regular file")
)

type workspaceReadError struct{ cause error }

func (e workspaceReadError) Error() string { return "workspace file read failed" }
func (e workspaceReadError) Unwrap() error { return e.cause }
func opaqueWorkspaceError(err error) error {
	if err == nil {
		return nil
	}
	return workspaceReadError{cause: err}
}

// ResolveWorkspacePath canonicalizes path and verifies that it remains inside
// workspaceRoot. If workspaceRoot is omitted, the current working directory
// is used, matching the Python MCP server. Existing symlinks are resolved, so
// a symlink inside the workspace pointing outside is rejected too.
func ResolveWorkspacePath(path string, workspaceRoots ...string) (string, error) {
	if strings.IndexByte(path, 0) >= 0 {
		return "", ErrPathNullByte
	}
	if !utf8.ValidString(path) {
		return "", ErrPathInvalid
	}
	for _, r := range path {
		if unicode.IsControl(r) {
			return "", ErrPathInvalid
		}
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

const maxWorkspaceFileBytes int64 = 16 << 20

// ReadWorkspaceFile acquires a workspace capability and reads a root-relative
// regular file through that capability. It deliberately does not call
// ResolveWorkspacePath: path validation followed by ambient os.ReadFile is a
// TOCTOU authorization check. The opened handle is statted and streamed with a
// practical bound; special files are opened nonblocking to prevent hangs.
func ReadWorkspaceFile(path string, workspaceRoots ...string) ([]byte, error) {
	if strings.IndexByte(path, 0) >= 0 {
		return nil, ErrPathNullByte
	}
	rootPath := ""
	if len(workspaceRoots) > 0 {
		rootPath = workspaceRoots[0]
	}
	if rootPath == "" {
		var err error
		rootPath, err = os.Getwd()
		if err != nil {
			return nil, opaqueWorkspaceError(err)
		}
	}
	canonical, err := canonicalExistingPath(rootPath)
	if err != nil {
		return nil, opaqueWorkspaceError(err)
	}
	relative, err := workspaceReadRelative(path, canonical)
	if err != nil {
		return nil, err
	}

	// Keep every opened directory alive until the operation returns. Besides
	// making early returns leak-free, this prevents a later path component from
	// being resolved through a handle that was already closed.
	preRootInfo, err := os.Stat(canonical)
	if err != nil {
		return nil, opaqueWorkspaceError(err)
	}
	root, err := os.OpenRoot(canonical)
	if err != nil {
		return nil, opaqueWorkspaceError(err)
	}
	defer root.Close()
	rootInfo, err := root.Stat(".")
	if err != nil {
		return nil, opaqueWorkspaceError(err)
	}
	if !os.SameFile(preRootInfo, rootInfo) {
		return nil, ErrPathOutsideWorkspace
	}

	parts := strings.Split(relative, "/")
	current := root
	for _, part := range parts[:len(parts)-1] {
		info, statErr := current.Lstat(part)
		if statErr != nil {
			return nil, opaqueWorkspaceError(statErr)
		}
		if info.Mode()&os.ModeSymlink != 0 || !info.IsDir() {
			return nil, ErrPathOutsideWorkspace
		}
		next, openErr := current.OpenRoot(part)
		if openErr != nil {
			return nil, opaqueWorkspaceError(openErr)
		}
		// Defer rather than closing the previous child immediately: an error at
		// any later component must close the complete traversal stack.
		defer next.Close()
		openedInfo, statErr := next.Stat(".")
		if statErr != nil {
			return nil, opaqueWorkspaceError(statErr)
		}
		if !os.SameFile(info, openedInfo) {
			return nil, ErrPathOutsideWorkspace
		}
		current = next
	}

	name := parts[len(parts)-1]
	info, err := current.Lstat(name)
	if err != nil {
		return nil, opaqueWorkspaceError(err)
	}
	if info.Mode()&os.ModeSymlink != 0 {
		return nil, ErrPathOutsideWorkspace
	}
	file, err := current.OpenFile(name, os.O_RDONLY|redactionOpenNonblock|redactionOpenNoFollow, 0)
	if err != nil {
		return nil, opaqueWorkspaceError(err)
	}
	defer file.Close()
	openedInfo, err := file.Stat()
	if err != nil {
		return nil, opaqueWorkspaceError(err)
	}
	if !os.SameFile(info, openedInfo) {
		return nil, ErrPathOutsideWorkspace
	}
	if !openedInfo.Mode().IsRegular() {
		return nil, ErrNotRegularFile
	}
	if openedInfo.Size() < 0 || openedInfo.Size() > maxWorkspaceFileBytes {
		return nil, ErrFileTooLarge
	}
	data, err := io.ReadAll(io.LimitReader(file, maxWorkspaceFileBytes+1))
	if err != nil {
		return nil, opaqueWorkspaceError(err)
	}
	if int64(len(data)) > maxWorkspaceFileBytes {
		return nil, ErrFileTooLarge
	}
	return data, nil
}

func workspaceReadRelative(path, canonicalRoot string) (string, error) {
	if filepath.IsAbs(path) {
		candidate, err := canonicalPath(path)
		if err != nil {
			return "", opaqueWorkspaceError(err)
		}
		rel, err := filepath.Rel(canonicalRoot, candidate)
		if err != nil || rel == "." || rel == ".." || strings.HasPrefix(rel, ".."+string(filepath.Separator)) {
			return "", ErrPathOutsideWorkspace
		}
		path = filepath.ToSlash(rel)
	}
	if err := validateWorkspaceRelativePath(path); err != nil {
		return "", err
	}
	return path, nil
}

func validateWorkspaceRelativePath(path string) error {
	if path == "" {
		return ErrPathInvalid
	}
	if !utf8.ValidString(path) {
		return ErrPathInvalid
	}
	if filepath.IsAbs(path) || strings.HasPrefix(path, "/") || (len(path) >= 2 && path[1] == ':' && ((path[0] >= 'A' && path[0] <= 'Z') || (path[0] >= 'a' && path[0] <= 'z'))) {
		return ErrPathOutsideWorkspace
	}
	if strings.Contains(path, "\\") {
		return ErrPathInvalid
	}
	for _, r := range path {
		if unicode.IsControl(r) {
			return ErrPathInvalid
		}
	}
	for _, part := range strings.Split(path, "/") {
		if part == ".." {
			return ErrPathOutsideWorkspace
		}
		if part == "" || part == "." {
			return ErrPathInvalid
		}
	}
	return nil
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
	return RedactBytesChecked(content)
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
	return RedactBytesChecked(content, profile)
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

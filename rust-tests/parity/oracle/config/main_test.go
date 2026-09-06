package main

import (
	"context"
	"errors"
	"os"
	"os/exec"
	"path/filepath"
	"strings"
	"testing"
	"time"
)

func TestVerifySourceHashRejectsWrongExpectedHash(t *testing.T) {
	_, source, _, ok := runtimeCaller()
	if !ok {
		t.Fatal("runtime caller unavailable")
	}
	configSource := filepath.Clean(filepath.Join(filepath.Dir(source), "../../../../internal/config/config.go"))
	if err := verifySourceHash(configSource, strings.Repeat("0", 64)); err == nil {
		t.Fatal("wrong expected source hash was accepted")
	}
}

func TestIsolatedEnvironmentRejectsReservedFixtureOverrides(t *testing.T) {
	reserved := []string{
		"HOME", "USERPROFILE", "TMPDIR", "TEMP", "TMP",
		"XDG_DATA_HOME", "XDG_CACHE_HOME",
	}
	for _, key := range reserved {
		t.Run(key, func(t *testing.T) {
			_, err := isolatedEnvironment(t.TempDir(), fixtureEnvironment{
				Values: map[string]string{key: "/outside/sandbox"},
			})
			if err == nil {
				t.Fatalf("reserved environment key %s was accepted", key)
			}
		})
	}
	if _, err := isolatedEnvironment(t.TempDir(), fixtureEnvironment{
		Values: map[string]string{"XDG_CONFIG_HOME": "/outside/sandbox"},
	}); err == nil {
		t.Fatal("absolute XDG_CONFIG_HOME override was accepted")
	}
	if _, err := isolatedEnvironment(t.TempDir(), fixtureEnvironment{
		SandboxXDGPath: "$ROOT/not-the-generated-sandbox",
	}); err == nil {
		t.Fatal("unvalidated sandbox XDG path was accepted")
	}
}

func TestReadCappedFileRejectsOutputOverLimit(t *testing.T) {
	path := filepath.Join(t.TempDir(), "stdout")
	if err := os.WriteFile(path, []byte("0123456789"), 0o600); err != nil {
		t.Fatal(err)
	}
	if _, err := readCappedFile(path, 9); err == nil {
		t.Fatal("output over the limit was accepted")
	}
}

func TestBoundedCommandRejectsLargeOutput(t *testing.T) {
	if _, err := exec.LookPath("sh"); err != nil {
		t.Skip("Unix shell unavailable")
	}
	root := t.TempDir()
	_, err := runBoundedCommand(
		context.Background(),
		"sh",
		[]string{"-c", "head -c 4194305 /dev/zero"},
		root,
		[]string{"PATH=" + os.Getenv("PATH")},
		filepath.Join(root, "stdout"),
		filepath.Join(root, "stderr"),
		5*time.Second,
	)
	if err == nil {
		t.Fatal("output over the bounded limit was accepted")
	}
}

func TestBoundedCommandKillsDescendantsOnTimeout(t *testing.T) {
	if _, err := exec.LookPath("sh"); err != nil {
		t.Skip("Unix shell unavailable")
	}
	root := t.TempDir()
	started := time.Now()
	_, err := runBoundedCommand(
		context.Background(),
		"sh",
		[]string{"-c", "sleep 30 & wait"},
		root,
		[]string{"PATH=" + os.Getenv("PATH")},
		filepath.Join(root, "stdout"),
		filepath.Join(root, "stderr"),
		50*time.Millisecond,
	)
	if !errors.Is(err, context.DeadlineExceeded) {
		t.Fatalf("expected deadline error, got %v", err)
	}
	if elapsed := time.Since(started); elapsed > 2*time.Second {
		t.Fatalf("timeout cleanup took too long: %s", elapsed)
	}
}

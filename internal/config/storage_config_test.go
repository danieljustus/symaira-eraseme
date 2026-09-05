package config

import (
	"os"
	"path/filepath"
	"strings"
	"testing"
)

func TestStorageConfigUsesPersistentDefault(t *testing.T) {
	home := t.TempDir()
	t.Setenv("HOME", home)
	t.Setenv("XDG_CONFIG_HOME", filepath.Join(home, "xdg-config"))
	for _, key := range []string{"SYMERASEME_DATA_DIR", "SYMERASEME_DB_DIR", "SYMERASEME_ENCRYPT_DB"} {
		t.Setenv(key, "")
	}

	storage, err := ResolveStorage()
	if err != nil {
		t.Fatalf("ResolveStorage() error = %v", err)
	}
	want := filepath.Join(home, ".local", "share", "symeraseme", "symeraseme.db")
	if storage.DBPath != want {
		t.Fatalf("DBPath = %q, want %q", storage.DBPath, want)
	}
	if storage.Encrypt {
		t.Fatal("default encryption = true, want false")
	}
	if storage.DBPath == filepath.Join(os.TempDir(), "symeraseme", "symeraseme.db") {
		t.Fatalf("persistent default unexpectedly uses legacy temp path: %q", storage.DBPath)
	}
}

func TestStorageConfigPrecedenceAndBooleanFalse(t *testing.T) {
	home := t.TempDir()
	project := t.TempDir()
	globalConfig := filepath.Join(home, "xdg-config", "symeraseme", "config.toml")
	if err := os.MkdirAll(filepath.Dir(globalConfig), 0o700); err != nil {
		t.Fatal(err)
	}
	if err := os.WriteFile(globalConfig, []byte("data_dir = \"global\"\ndb_dir = \"global-db\"\nencrypt_db = true\n"), 0o600); err != nil {
		t.Fatal(err)
	}
	if err := os.WriteFile(filepath.Join(project, ".symeraseme.toml"), []byte("data_dir = \"project\"\nencrypt_db = false\n"), 0o600); err != nil {
		t.Fatal(err)
	}
	t.Setenv("HOME", home)
	t.Setenv("XDG_CONFIG_HOME", filepath.Join(home, "xdg-config"))
	t.Setenv("SYMERASEME_DATA_DIR", "")
	t.Setenv("SYMERASEME_DB_DIR", "")
	t.Setenv("SYMERASEME_ENCRYPT_DB", "")
	t.Chdir(project)

	storage, err := ResolveStorage()
	if err != nil {
		t.Fatalf("ResolveStorage() error = %v", err)
	}
	if storage.Encrypt {
		t.Fatal("project false did not override global true")
	}
	if storage.DataDir != filepath.Join(project, "project") {
		t.Fatalf("DataDir = %q, want project-relative path", storage.DataDir)
	}
	if storage.DBPath != filepath.Join(project, "global-db", "symeraseme.db") {
		t.Fatalf("DBPath = %q, want global db path", storage.DBPath)
	}

	t.Setenv("SYMERASEME_DATA_DIR", filepath.Join(t.TempDir(), "env-data"))
	t.Setenv("SYMERASEME_DB_DIR", filepath.Join(t.TempDir(), "env-db"))
	t.Setenv("SYMERASEME_ENCRYPT_DB", "true")
	storage, err = ResolveStorage()
	if err != nil {
		t.Fatalf("ResolveStorage() with env error = %v", err)
	}
	if !storage.Encrypt || !strings.Contains(storage.DBPath, "env-db") {
		t.Fatalf("environment did not win: %+v", storage)
	}
}

func TestStorageConfigRejectsMalformedValues(t *testing.T) {
	project := t.TempDir()
	t.Chdir(project)
	t.Setenv("SYMERASEME_DATA_DIR", t.TempDir())
	t.Setenv("SYMERASEME_DB_DIR", "")
	t.Setenv("SYMERASEME_ENCRYPT_DB", "definitely-not-a-bool")
	if _, err := ResolveStorage(); err == nil || !strings.Contains(err.Error(), "SYMERASEME_ENCRYPT_DB") {
		t.Fatalf("malformed encryption setting error = %v", err)
	}

	t.Setenv("SYMERASEME_ENCRYPT_DB", "false")
	t.Setenv("SYMERASEME_DB_DIR", "")
	if err := os.WriteFile(filepath.Join(project, ".symeraseme.toml"), []byte("db_dir = \"\x00\"\n"), 0o600); err != nil {
		t.Fatal(err)
	}
	if _, err := ResolveStorage(); err == nil || !strings.Contains(err.Error(), "db_dir") {
		t.Fatalf("malformed database directory error = %v", err)
	}

	// Unrelated and legacy nested fields remain compatible with configkit's
	// permissive loader; storage fields are still validated explicitly.
	if err := os.WriteFile(filepath.Join(project, ".symeraseme.toml"), []byte("unknown_key = true\n[legacy.server]\nport = 1234\n"), 0o600); err != nil {
		t.Fatal(err)
	}
	if _, err := ResolveStorage(); err != nil {
		t.Fatalf("unrelated/nested legacy config should be ignored: %v", err)
	}

	// Invalid port
	if err := os.WriteFile(filepath.Join(project, ".symeraseme.toml"), []byte("port = 99999\n"), 0o600); err != nil {
		t.Fatal(err)
	}
	if _, err := ResolveStorage(); err == nil || !strings.Contains(err.Error(), "outside 1..65535") {
		t.Fatalf("invalid port error = %v", err)
	}
}

func TestStorageConfigGlobalWithoutXDG(t *testing.T) {
	home := t.TempDir()
	project := t.TempDir()
	t.Setenv("HOME", home)
	t.Setenv("XDG_CONFIG_HOME", "")
	for _, key := range []string{"SYMERASEME_DATA_DIR", "SYMERASEME_DB_DIR", "SYMERASEME_ENCRYPT_DB"} {
		t.Setenv(key, "")
	}
	globalConfig := filepath.Join(home, ".config", "symeraseme", "config.toml")
	if err := os.MkdirAll(filepath.Dir(globalConfig), 0o700); err != nil {
		t.Fatal(err)
	}
	if err := os.WriteFile(globalConfig, []byte("data_dir = \"default-global\"\nencrypt_db = 1\nport = 9000\n"), 0o600); err != nil {
		t.Fatal(err)
	}
	t.Chdir(project)

	storage, err := ResolveStorage()
	if err != nil {
		t.Fatalf("ResolveStorage() error = %v", err)
	}
	if !storage.Encrypt {
		t.Fatal("integer 1 boolean not parsed as true")
	}
	if storage.DataDir != filepath.Join(project, "default-global") {
		t.Fatalf("DataDir = %q, want %q", storage.DataDir, filepath.Join(project, "default-global"))
	}

	cfg, err := Load().Load()
	if err != nil {
		t.Fatal(err)
	}
	if cfg.Port != 9000 {
		t.Fatalf("port = %d, want 9000", cfg.Port)
	}
}

func TestLoaderReloadAndReset(t *testing.T) {
	home := t.TempDir()
	project := t.TempDir()
	t.Setenv("HOME", home)
	t.Setenv("XDG_CONFIG_HOME", filepath.Join(home, "xdg-config"))
	for _, key := range []string{"SYMERASEME_DATA_DIR", "SYMERASEME_DB_DIR", "SYMERASEME_ENCRYPT_DB"} {
		t.Setenv(key, "")
	}
	t.Chdir(project)

	loader := Load()
	cfg1, err := loader.Load()
	if err != nil {
		t.Fatal(err)
	}
	if cfg1.Port != 8000 {
		t.Fatalf("port = %d, want 8000", cfg1.Port)
	}

	if err := os.WriteFile(filepath.Join(project, ".symeraseme.toml"), []byte("port = 8080\n"), 0o600); err != nil {
		t.Fatal(err)
	}
	// Cached load should still see 8000
	cfgCached, err := loader.Load()
	if err != nil {
		t.Fatal(err)
	}
	if cfgCached.Port != 8000 {
		t.Fatalf("cached port = %d, want 8000", cfgCached.Port)
	}

	// Reload should see 8080
	cfgReload, err := loader.Reload()
	if err != nil {
		t.Fatal(err)
	}
	if cfgReload.Port != 8080 {
		t.Fatalf("reload port = %d, want 8080", cfgReload.Port)
	}
	cfgCached, err = loader.Load()
	if err != nil {
		t.Fatal(err)
	}
	if cfgCached.Port != 8000 {
		t.Fatalf("reload changed cached port = %d, want 8000", cfgCached.Port)
	}

	// ResetCache
	loader.ResetCache()
	cfgReset, err := loader.Load()
	if err != nil {
		t.Fatal(err)
	}
	if cfgReset.Port != 8080 {
		t.Fatalf("reset load port = %d, want 8080", cfgReset.Port)
	}
}

func TestConfigPreservesConfigkitEnvironmentNames(t *testing.T) {
	t.Setenv("HOME", t.TempDir())
	t.Setenv("XDG_CONFIG_HOME", t.TempDir())
	t.Setenv("SYMERASEME_PORT", "9123")
	t.Setenv("SYMERASEME_ALLOW_REMOTE", "true")
	cfg, err := Load().Load()
	if err != nil {
		t.Fatal(err)
	}
	if cfg.Port != 9123 || !cfg.AllowRemote {
		t.Fatalf("configkit-compatible env mapping changed: %+v", cfg)
	}
}

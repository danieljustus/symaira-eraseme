// Package config provides the symeraseme configuration and storage path
// contract. Configuration is loaded in precedence order: defaults, the global
// TOML file, the project TOML file, and SYMERASEME_* environment variables.
package config

import (
	"fmt"
	"os"
	"path/filepath"
	"strconv"
	"strings"
	"sync"

	"github.com/BurntSushi/toml"
)

// Config is the configuration surface shared by the CLI and MCP server.
type Config struct {
	DataDir     string `json:"data_dir"`
	DBDir       string `json:"db_dir"`
	Encrypt     bool   `json:"encrypt_db"`
	Port        int    `json:"port"`
	AllowRemote bool   `json:"allow_remote"`
}

// Storage is the validated, absolute storage configuration used by the
// production database opener.
type Storage struct {
	DataDir string `json:"data_dir"`
	DBDir   string `json:"db_dir"`
	DBPath  string `json:"db_path"`
	TempDir string `json:"temp_dir"`
	Encrypt bool   `json:"encrypt_db"`
}

// Defaults returns the documented persistent configuration.
func Defaults() *Config {
	return &Config{
		DataDir: "~/.local/share/symeraseme",
		Port:    8000,
	}
}

// Loader provides a cached configuration snapshot. A new Load call returns a
// fresh loader, which keeps short-lived CLI commands independent while still
// providing the same Load/Reload API as the shared config loader.
type Loader struct {
	mu        sync.Mutex
	loaded    bool
	cached    *Config
	cachedErr error
}

// Load returns a configuration loader for symeraseme.
func Load() *Loader { return &Loader{} }

// Load reads and caches the effective configuration.
func (l *Loader) Load() (*Config, error) {
	l.mu.Lock()
	defer l.mu.Unlock()
	if !l.loaded {
		l.cached, l.cachedErr = loadConfig()
		l.loaded = true
	}
	return cloneConfig(l.cached), l.cachedErr
}

// Reload reads a fresh configuration, bypassing the cache.
func (l *Loader) Reload() (*Config, error) {
	l.mu.Lock()
	defer l.mu.Unlock()
	cfg, err := loadConfig()
	return cloneConfig(cfg), err
}

// ResetCache clears the loader's cached snapshot.
func (l *Loader) ResetCache() {
	l.mu.Lock()
	defer l.mu.Unlock()
	l.loaded = false
	l.cached = nil
	l.cachedErr = nil
}

func cloneConfig(cfg *Config) *Config {
	if cfg == nil {
		return nil
	}
	copy := *cfg
	return &copy
}

var allowedKeys = map[string]bool{
	"data_dir": true, "db_dir": true, "encrypt_db": true,
	"port": true, "allow_remote": true,
}

func loadConfig() (*Config, error) {
	cfg := Defaults()
	home, err := os.UserHomeDir()
	if err != nil {
		return cfg, fmt.Errorf("cannot determine home directory: %w", err)
	}

	globalPath := filepath.Join(home, ".config", "symeraseme", "config.toml")
	if xdg := os.Getenv("XDG_CONFIG_HOME"); filepath.IsAbs(xdg) {
		globalPath = filepath.Join(xdg, "symeraseme", "config.toml")
	}
	if err := mergeFile(cfg, globalPath); err != nil {
		return cfg, fmt.Errorf("global config error: %w", err)
	}

	cwd, err := os.Getwd()
	if err != nil {
		return cfg, fmt.Errorf("cannot determine current directory: %w", err)
	}
	if err := mergeFile(cfg, filepath.Join(cwd, ".symeraseme.toml")); err != nil {
		return cfg, fmt.Errorf("project config error: %w", err)
	}

	if err := applyEnv(cfg); err != nil {
		return cfg, fmt.Errorf("environment config error: %w", err)
	}
	return cfg, validateConfig(cfg)
}

func mergeFile(cfg *Config, path string) error {
	if _, err := os.Stat(path); os.IsNotExist(err) {
		return nil
	} else if err != nil {
		return err
	}
	var values map[string]any
	if _, err := toml.DecodeFile(path, &values); err != nil {
		return fmt.Errorf("failed to parse %s: %w", path, err)
	}
	for key, value := range values {
		// configkit historically ignored unrelated and legacy keys, including
		// nested TOML tables. Storage fields remain explicitly validated below.
		if !allowedKeys[key] {
			continue
		}
		if err := applyValue(cfg, key, value); err != nil {
			return fmt.Errorf("field %q in %s: %w", key, path, err)
		}
	}
	return nil
}

func applyEnv(cfg *Config) error {
	for key, target := range map[string]string{
		"SYMERASEME_DATA_DIR":     "data_dir",
		"SYMERASEME_DB_DIR":       "db_dir",
		"SYMERASEME_ENCRYPT_DB":   "encrypt_db",
		"SYMERASEME_PORT":         "port",
		"SYMERASEME_ALLOW_REMOTE": "allow_remote",
	} {
		value, ok := os.LookupEnv(key)
		if !ok || value == "" {
			continue
		}
		if err := applyValue(cfg, target, value); err != nil {
			return fmt.Errorf("%s: %w", key, err)
		}
	}
	return nil
}

func applyValue(cfg *Config, key string, value any) error {
	switch key {
	case "data_dir", "db_dir":
		text, ok := value.(string)
		if !ok {
			return fmt.Errorf("must be a string, got %T", value)
		}
		if strings.TrimSpace(text) == "" {
			return errorsForEmptyPath(key)
		}
		switch key {
		case "data_dir":
			cfg.DataDir = text
		case "db_dir":
			cfg.DBDir = text
		}
	case "encrypt_db", "allow_remote":
		parsed, err := parseBool(value)
		if err != nil {
			return err
		}
		if key == "encrypt_db" {
			cfg.Encrypt = parsed
		} else {
			cfg.AllowRemote = parsed
		}
	case "port":
		parsed, err := parseInt(value)
		if err != nil {
			return err
		}
		cfg.Port = parsed
	default:
		return fmt.Errorf("unknown field %q", key)
	}
	return nil
}

func parseBool(value any) (bool, error) {
	switch v := value.(type) {
	case bool:
		return v, nil
	case int64:
		if v == 1 {
			return true, nil
		}
		if v == 0 {
			return false, nil
		}
		return false, fmt.Errorf("invalid boolean integer %d (use 1 or 0)", v)
	case int:
		if v == 1 {
			return true, nil
		}
		if v == 0 {
			return false, nil
		}
		return false, fmt.Errorf("invalid boolean integer %d (use 1 or 0)", v)
	case string:
		switch strings.ToLower(strings.TrimSpace(v)) {
		case "1", "true", "yes", "on":
			return true, nil
		case "0", "false", "no", "off":
			return false, nil
		default:
			return false, fmt.Errorf("invalid boolean %q (use true/false, yes/no, or 1/0)", v)
		}
	default:
		return false, fmt.Errorf("must be boolean, got %T", value)
	}
}

func parseInt(value any) (int, error) {
	switch parsed := value.(type) {
	case int64:
		return int(parsed), nil
	case int:
		return parsed, nil
	case string:
		return strconv.Atoi(strings.TrimSpace(parsed))
	default:
		return 0, fmt.Errorf("must be an integer, got %T", value)
	}
}

func errorsForEmptyPath(key string) error {
	return fmt.Errorf("%s must not be empty", key)
}

func validateConfig(cfg *Config) error {
	if cfg.Port < 1 || cfg.Port > 65535 {
		return fmt.Errorf("port %d is outside 1..65535", cfg.Port)
	}
	if _, err := resolvePath(cfg.DataDir, "data_dir"); err != nil {
		return err
	}
	if cfg.DBDir != "" {
		if _, err := resolvePath(cfg.DBDir, "db_dir"); err != nil {
			return err
		}
	}
	return nil
}

func resolvePath(raw, field string) (string, error) {
	if strings.TrimSpace(raw) == "" {
		return "", fmt.Errorf("%s must not be empty", field)
	}
	if strings.IndexByte(raw, 0) >= 0 {
		return "", fmt.Errorf("%s contains a NUL byte", field)
	}
	if strings.HasPrefix(raw, "~/") || raw == "~" {
		home, err := os.UserHomeDir()
		if err != nil {
			return "", fmt.Errorf("%s: cannot expand home directory: %w", field, err)
		}
		if raw == "~" {
			raw = home
		} else {
			raw = filepath.Join(home, raw[2:])
		}
	}
	absolute, err := filepath.Abs(filepath.Clean(raw))
	if err != nil {
		return "", fmt.Errorf("%s: %w", field, err)
	}
	return absolute, nil
}

// DefaultEncryptedTempDir returns a user-scoped cache directory for decrypted
// database copies. It deliberately avoids the shared OS temp directory; on
// Windows the directory inherits the current user's profile ACLs (Go's mode
// bits do not express Windows ACLs).
func DefaultEncryptedTempDir() string {
	base, err := os.UserCacheDir()
	if err != nil || strings.TrimSpace(base) == "" {
		base, err = os.UserHomeDir()
		if err != nil || strings.TrimSpace(base) == "" {
			base = os.TempDir()
		}
	}
	return filepath.Join(base, "symeraseme", "database")
}

// ResolveStorage loads and validates the effective storage configuration.
func ResolveStorage() (Storage, error) {
	cfg, err := Load().Load()
	if err != nil {
		return Storage{}, err
	}
	dataDir, err := resolvePath(cfg.DataDir, "data_dir")
	if err != nil {
		return Storage{}, err
	}
	dbDir := dataDir
	if cfg.DBDir != "" {
		dbDir, err = resolvePath(cfg.DBDir, "db_dir")
		if err != nil {
			return Storage{}, err
		}
	}
	return Storage{
		DataDir: dataDir,
		DBDir:   dbDir,
		DBPath:  filepath.Join(dbDir, "symeraseme.db"),
		TempDir: DefaultEncryptedTempDir(),
		Encrypt: cfg.Encrypt,
	}, nil
}

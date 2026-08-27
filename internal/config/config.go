// Package config wires the corekit configkit loader for symeraseme.
//
// Configuration precedence (later wins): defaults → global TOML
// (~/.config/symeraseme/config.toml) → project TOML (./.symeraseme.toml)
// → environment vars (SYMERASEME_*). The Go port reuses the corekit loader
// so the Python tree's env-var conventions (SYMERASEME_DATA_DIR,
// SYMERASEME_DB_DIR, SYMERASEME_ENCRYPT_DB, SYMERASEME_RESOURCES) map onto
// the same config path.
package config

import (
	"github.com/danieljustus/symaira-corekit/configkit"
)

// Config is the Go-port configuration surface. Fields are additive as
// packages land; the data contract (registry), the MCP tool surface and the
// event store are specified independently in docs/.
type Config struct {
	DataDir     string `json:"data_dir"`     // SYMERASEME_DATA_DIR equivalent
	DBDir       string `json:"db_dir"`       // SYMERASEME_DB_DIR equivalent
	Encrypt     bool   `json:"encrypt_db"`   // SYMERASEME_ENCRYPT_DB equivalent
	Port        int    `json:"port"`         // MCP HTTP server port (temporary Python-compat surface)
	AllowRemote bool   `json:"allow_remote"` // MCP server bind policy
}

// Defaults returns the baseline configuration.
func Defaults() *Config {
	return &Config{
		DataDir:     "",
		DBDir:       "",
		Encrypt:     false,
		Port:        8000,
		AllowRemote: false,
	}
}

// Load returns a cached config loader for the symeraseme app.
func Load() *configkit.Loader[Config] {
	return configkit.NewLoader(configkit.Options{AppName: "symeraseme"}, Defaults)
}

package symeraseme

import (
	"embed"
	"io/fs"

	"github.com/danieljustus/symaira-eraseme/internal/registry"
)

// Embedded registry data (5.1 MB YAML). go:embed cannot reach outside the
// package directory, so the embed directive lives in this root-level file
// and hands the filesystem to internal/registry.
//
//go:embed all:registry
var registryFS embed.FS

func init() {
	sub, err := fs.Sub(registryFS, "registry")
	if err == nil {
		registry.SetEmbedded(sub)
	}
}

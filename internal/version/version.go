// Package version provides the versionkit handshake payload for symeraseme.
//
// Every Symaira CLI reports `version --json` as the versionkit.Info payload
// ({tool, version, schema_version}); GUI clients (symaira-appkit's
// SymairaToolKit) handshake against exactly these field names. Bump
// SchemaVersion whenever a machine-readable JSON output of this tool changes
// incompatibly — never rename the payload fields.
package version

import "github.com/danieljustus/symaira-corekit/versionkit"

// SchemaVersion is the schema version of this tool's machine-readable JSON
// outputs. Starts at 1 with the Go port.
const SchemaVersion = 1

// Info returns the standard handshake payload for the given build version.
func Info(toolVersion string) versionkit.Info {
	return versionkit.New("symeraseme", toolVersion, SchemaVersion)
}

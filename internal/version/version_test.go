package version

import "testing"

func TestInfoUsesStableHandshakeContract(t *testing.T) {
	info := Info("v0.12.0")
	if info.Tool != "symeraseme" || info.Version != "v0.12.0" || info.SchemaVersion != SchemaVersion {
		t.Fatalf("info = %#v", info)
	}
	if info.String() != "symeraseme v0.12.0" {
		t.Fatalf("string = %q", info.String())
	}
}

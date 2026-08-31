package config

import "testing"

func TestDefaultsAndLoader(t *testing.T) {
	cfg := Defaults()
	if cfg == nil || cfg.Port != 8000 || cfg.AllowRemote || cfg.Encrypt {
		t.Fatalf("defaults = %#v", cfg)
	}
	if Load() == nil {
		t.Fatal("Load returned nil")
	}
}

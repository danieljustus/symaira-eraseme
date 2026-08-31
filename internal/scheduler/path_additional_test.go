package scheduler

import "testing"

func TestSchedulerPathHelpers(t *testing.T) {
	if got := contentWithWrapperDir("{wrapper_dir}/tick", "/tmp/schedules"); got != "/tmp/schedules/tick" {
		t.Fatalf("contentWithWrapperDir = %q", got)
	}
	if got := nameToLaunchdLabel("symeraseme-tick"); got != "com.symeraseme.tick" {
		t.Fatalf("nameToLaunchdLabel = %q", got)
	}
}

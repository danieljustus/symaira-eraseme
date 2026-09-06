//go:build !aix && !darwin && !dragonfly && !freebsd && !hurd && !illumos && !ios && !linux && !netbsd && !openbsd && !solaris && !windows

package main

import (
	"fmt"
	"os/exec"
)

func configureProcessGroup(_ *exec.Cmd) error {
	return fmt.Errorf("process-tree cleanup is unsupported on this platform")
}

func killProcessTree(_ *exec.Cmd) error {
	return fmt.Errorf("process-tree cleanup is unsupported on this platform")
}

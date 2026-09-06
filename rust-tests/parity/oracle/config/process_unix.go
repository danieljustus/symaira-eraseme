//go:build aix || darwin || dragonfly || freebsd || hurd || illumos || ios || linux || netbsd || openbsd || solaris

package main

import (
	"os/exec"
	"syscall"
)

func configureProcessGroup(command *exec.Cmd) error {
	command.SysProcAttr = &syscall.SysProcAttr{Setpgid: true}
	return nil
}

func killProcessTree(command *exec.Cmd) error {
	if command.Process == nil {
		return nil
	}
	pid := command.Process.Pid
	if err := syscall.Kill(-pid, syscall.SIGKILL); err != nil && err != syscall.ESRCH {
		return err
	}
	return nil
}

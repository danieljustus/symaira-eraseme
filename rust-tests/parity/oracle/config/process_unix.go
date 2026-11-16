//go:build aix || darwin || dragonfly || freebsd || hurd || illumos || ios || linux || netbsd || openbsd || solaris

package main

import (
	"os/exec"
	"runtime"
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
	err := syscall.Kill(-pid, syscall.SIGKILL)
	// Darwin's killpg reports EPERM when every member is an unreaped zombie.
	// All group members share our uid, so EPERM there means nothing live is left.
	if err == nil || err == syscall.ESRCH || (err == syscall.EPERM && runtime.GOOS == "darwin") {
		return nil
	}
	return err
}

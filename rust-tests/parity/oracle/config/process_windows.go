//go:build windows

package main

import (
	"fmt"
	"os/exec"
	"strconv"
	"syscall"
)

func configureProcessGroup(command *exec.Cmd) error {
	command.SysProcAttr = &syscall.SysProcAttr{CreationFlags: syscall.CREATE_NEW_PROCESS_GROUP}
	return nil
}

func killProcessTree(command *exec.Cmd) error {
	if command.Process == nil {
		return nil
	}
	result := exec.Command("taskkill", "/PID", strconv.Itoa(command.Process.Pid), "/T", "/F").Run()
	if result != nil {
		return fmt.Errorf("taskkill process tree: %w", result)
	}
	return nil
}

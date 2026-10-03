// Native negative controls for the owned Windows AppContainer CLI boundary.
package main

import (
	"encoding/json"
	"errors"
	"fmt"
	"net"
	"os"
	"os/exec"
	"runtime"
	"syscall"
	"time"
)

func errorCode(err error) uintptr {
	var errno syscall.Errno
	if errors.As(err, &errno) {
		return uintptr(errno)
	}
	return 0
}

func isDenied(err error) bool {
	// Windows ERROR_ACCESS_DENIED and WSAEACCES. Connection refusal/timeout
	// do not prove that the no-network AppContainer boundary blocked the call.
	return err != nil && (errorCode(err) == 5 || errorCode(err) == 10013)
}

func main() {
	if len(os.Args) == 2 && os.Args[1] == "stall" {
		for {
			time.Sleep(time.Second)
		}
	}
	if len(os.Args) == 2 && os.Args[1] == "failure" {
		os.Exit(23)
	}
	if len(os.Args) != 3 {
		panic("expected outside read and write markers")
	}
	checks := map[string]bool{}
	_, err := os.ReadFile(os.Args[1])
	checks["outside_read_denied"] = os.IsPermission(err)
	err = os.WriteFile(os.Args[2], []byte("must not escape"), 0600)
	checks["outside_write_denied"] = os.IsPermission(err)
	inside := os.Getenv("HOME") + string(os.PathSeparator) + "inside-marker"
	err = os.WriteFile(inside, []byte("owned write"), 0600)
	checks["owned_write_allowed"] = err == nil
	if err == nil {
		err = os.Remove(inside)
		checks["owned_remove_allowed"] = err == nil
	}
	conn, err := net.DialTimeout("tcp", "127.0.0.1:9", time.Second)
	checks["tcp_denied"] = isDenied(err)
	if conn != nil {
		_ = conn.Close()
	}
	packet, err := net.DialTimeout("udp", "198.51.100.1:9", time.Second)
	if err == nil {
		_, err = packet.Write([]byte("synthetic-network-negative-control"))
		_ = packet.Close()
	}
	checks["udp_denied"] = isDenied(err)
	err = exec.Command(os.Args[0], "failure").Run()
	// ERROR_CHILD_PROCESS_BLOCKED (367) is an actual kernel policy denial.
	checks["child_creation_denied"] = isDenied(err) || errorCode(err) == 367
	all := true
	for _, passed := range checks {
		all = all && passed
	}
	_ = json.NewEncoder(os.Stdout).Encode(map[string]any{"checks": checks, "passed": all,
		"go_version": runtime.Version(), "os": runtime.GOOS, "arch": runtime.GOARCH})
	if !all {
		fmt.Fprintln(os.Stderr, "native sandbox negative control failed")
		os.Exit(1)
	}
}

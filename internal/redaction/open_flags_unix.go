//go:build aix || darwin || dragonfly || freebsd || linux || netbsd || openbsd || solaris

package redaction

import "syscall"

const (
	redactionOpenNonblock = syscall.O_NONBLOCK
	redactionOpenNoFollow = syscall.O_NOFOLLOW
)

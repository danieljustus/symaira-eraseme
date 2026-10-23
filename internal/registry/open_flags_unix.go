//go:build aix || darwin || dragonfly || freebsd || linux || netbsd || openbsd || solaris

package registry

import "syscall"

const registryOpenNonblock = syscall.O_NONBLOCK

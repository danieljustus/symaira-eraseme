#!/usr/bin/env python3
"""Privileged namespace launcher for disposable Linux store switchbacks.

The trusted Python harness remains outside this boundary. Each CLI/probe child
gets a private mount, network and PID namespace. Writable virtiofs mounts are
made read-only in that namespace, and Landlock grants filesystem access only to
the disposable run root plus read-only system runtime files. The child drops to
the invoking uid/gid and sets no_new_privs before it is executed.
"""
import ctypes
import json
import os
import platform
import subprocess
import sys


LIBC = ctypes.CDLL(None, use_errno=True)
SYS_LANDLOCK_CREATE_RULESET = 444
SYS_LANDLOCK_ADD_RULE = 445
SYS_LANDLOCK_RESTRICT_SELF = 446
LANDLOCK_CREATE_RULESET_VERSION = 1
LANDLOCK_RULE_PATH_BENEATH = 1
PR_SET_NO_NEW_PRIVS = 38

MS_RDONLY = 1
MS_REMOUNT = 32
MS_BIND = 4096
MS_REC = 16384
MS_PRIVATE = 1 << 18

O_PATH = getattr(os, "O_PATH", 0o10000000)
O_CLOEXEC = getattr(os, "O_CLOEXEC", 0)

FS_EXECUTE = 1 << 0
FS_WRITE_FILE = 1 << 1
FS_READ_FILE = 1 << 2
FS_READ_DIR = 1 << 3
FS_REMOVE_DIR = 1 << 4
FS_REMOVE_FILE = 1 << 5
FS_MAKE_CHAR = 1 << 6
FS_MAKE_DIR = 1 << 7
FS_MAKE_REG = 1 << 8
FS_MAKE_SOCK = 1 << 9
FS_MAKE_FIFO = 1 << 10
FS_MAKE_BLOCK = 1 << 11
FS_MAKE_SYM = 1 << 12
FS_REFER = 1 << 13
FS_TRUNCATE = 1 << 14
NET_BIND_TCP = 1 << 0
NET_CONNECT_TCP = 1 << 1
FS_ALL_ABI4 = (1 << 15) - 1
FS_READ = FS_READ_FILE | FS_READ_DIR
FS_WRITE = (FS_WRITE_FILE | FS_REMOVE_DIR | FS_REMOVE_FILE | FS_MAKE_DIR |
            FS_MAKE_REG | FS_REFER | FS_TRUNCATE)


class RulesetAttr(ctypes.Structure):
    _fields_ = [("handled_access_fs", ctypes.c_uint64),
                ("handled_access_net", ctypes.c_uint64),
                ("scoped", ctypes.c_uint64)]


class PathBeneathAttr(ctypes.Structure):
    _fields_ = [("allowed_access", ctypes.c_uint64),
                ("parent_fd", ctypes.c_int32),
                ("reserved", ctypes.c_uint32)]


def syscall(number, *args):
    result = LIBC.syscall(number, *args)
    if result < 0:
        code = ctypes.get_errno()
        raise OSError(code, os.strerror(code))
    return result


def mount(source, target, filesystem, flags, data=None):
    source_b = None if source is None else os.fsencode(source)
    target_b = os.fsencode(target)
    fs_b = None if filesystem is None else os.fsencode(filesystem)
    data_b = None if data is None else os.fsencode(data)
    if LIBC.mount(source_b, target_b, fs_b, ctypes.c_ulong(flags), data_b) != 0:
        code = ctypes.get_errno()
        raise OSError(code, os.strerror(code), target)


def set_no_new_privs():
    if LIBC.prctl(PR_SET_NO_NEW_PRIVS, 1, 0, 0, 0) != 0:
        code = ctypes.get_errno()
        raise OSError(code, os.strerror(code), "PR_SET_NO_NEW_PRIVS")


def unescape_mount_field(value):
    return (value.replace("\\040", " ").replace("\\011", "\t")
            .replace("\\012", "\n").replace("\\134", "\\"))


def readonly_writable_virtiofs():
    """Remount every writable host-share mount read-only in this namespace."""
    changed = []
    with open("/proc/self/mountinfo", encoding="utf-8") as stream:
        for line in stream:
            fields = line.rstrip("\n").split()
            try:
                separator = fields.index("-")
                filesystem = fields[separator + 1]
            except (ValueError, IndexError):
                continue
            mount_options = set(fields[5].split(","))
            if filesystem != "virtiofs" or "rw" not in mount_options:
                continue
            target = unescape_mount_field(fields[4])
            mount(target, target, None, MS_BIND | MS_REC)
            mount(None, target, None, MS_BIND | MS_REMOUNT | MS_RDONLY)
            matching_mount_options = []
            with open('/proc/self/mountinfo', encoding='utf-8') as current:
                for current_line in current:
                    current_fields = current_line.rstrip('\n').split()
                    if (len(current_fields) > 5 and
                            unescape_mount_field(current_fields[4]) == target):
                        matching_mount_options.append(set(current_fields[5].split(',')))
            # A bind remount can stack a new mount over the original. In that
            # case mountinfo lists both at the same target; the final matching
            # entry is the visible topmost mount whose flags govern access.
            if not matching_mount_options:
                raise RuntimeError('virtiofs mount disappeared during confinement: ' + target)
            visible_options = matching_mount_options[-1]
            if 'ro' not in visible_options or 'rw' in visible_options:
                raise RuntimeError('writable virtiofs remount did not become read-only: ' + target)
            changed.append(target)
    return changed


def landlock_version():
    return syscall(SYS_LANDLOCK_CREATE_RULESET, None, 0,
                   LANDLOCK_CREATE_RULESET_VERSION)


def add_path_rule(ruleset_fd, path, rights):
    fd = os.open(path, O_PATH | O_CLOEXEC)
    try:
        rule = PathBeneathAttr(rights, fd, 0)
        syscall(SYS_LANDLOCK_ADD_RULE, ruleset_fd, LANDLOCK_RULE_PATH_BENEATH,
                ctypes.byref(rule), 0)
    finally:
        os.close(fd)


def resolved_if_exists(path):
    candidate = os.path.realpath(path)
    return candidate if os.path.exists(candidate) else None


def restrict_landlock(config):
    version = landlock_version()
    if version < 4:
        raise RuntimeError("Landlock ABI 4 is required to deny TCP networking")
    attr = RulesetAttr(FS_ALL_ABI4, NET_BIND_TCP | NET_CONNECT_TCP, 0)
    fd = syscall(SYS_LANDLOCK_CREATE_RULESET, ctypes.byref(attr),
                 ctypes.sizeof(attr), 0)
    try:
        root = os.path.realpath(config["root"])
        executable = os.path.realpath(config["executable"])
        add_path_rule(fd, root, FS_READ | FS_WRITE)
        # Read-only runtime files support ELF loading, Rust's stdlib and the
        # Python negative-control probe. The repo, HOME and scratch are omitted.
        for path in ("/usr", "/lib", "/lib64", "/proc"):
            if os.path.exists(path):
                add_path_rule(fd, path, FS_READ)
        go_root = config.get("env", {}).get("GOROOT")
        if go_root:
            go_root = os.path.realpath(go_root)
            if not os.path.isdir(go_root):
                raise RuntimeError("configured Go runtime root does not exist")
            add_path_rule(fd, go_root, FS_READ)
        for path in ("/etc/ld.so.cache", "/dev/null"):
            if os.path.exists(path):
                rights = FS_READ_FILE | (FS_WRITE_FILE if path == "/dev/null" else 0)
                add_path_rule(fd, path, rights)
        add_path_rule(fd, executable, FS_EXECUTE)
        # Allow only the ELF interpreter required to load a dynamic binary.
        machine = platform.machine().lower()
        loader = {"aarch64": "/lib/ld-linux-aarch64.so.1",
                  "arm64": "/lib/ld-linux-aarch64.so.1",
                  "x86_64": "/lib64/ld-linux-x86-64.so.2"}.get(machine)
        if loader:
            loader = resolved_if_exists(loader)
            if loader:
                add_path_rule(fd, loader, FS_EXECUTE)
        set_no_new_privs()
        syscall(SYS_LANDLOCK_RESTRICT_SELF, fd, 0)
    finally:
        os.close(fd)
    return version


def namespace_ids():
    return {name: os.stat("/proc/self/ns/" + name).st_ino
            for name in ("mnt", "net", "pid")}


def write_audit(path, record):
    raw = (json.dumps(record, sort_keys=True, indent=2) + "\n").encode()
    fd = os.open(path, os.O_WRONLY | os.O_CREAT | os.O_EXCL, 0o600)
    try:
        with os.fdopen(fd, "wb", closefd=False) as stream:
            stream.write(raw)
            stream.flush()
            os.fsync(stream.fileno())
    finally:
        os.close(fd)


def launch(config_path):
    """Capture the caller namespaces, then enter fresh private namespaces."""
    with open(config_path, "rb") as stream:
        config = json.load(stream)
    config["parent_namespace"] = namespace_ids()
    with open(config_path, "wb") as stream:
        stream.write((json.dumps(config, sort_keys=True) + "\n").encode())
    os.chmod(config_path, 0o600)
    command = ["/usr/bin/sudo", "-n", "/usr/bin/unshare",
               "--mount", "--net", "--pid", "--fork", "--mount-proc",
               "/usr/bin/python3", "-I", "-S", os.path.realpath(__file__),
               "--inside", os.path.realpath(config_path)]
    os.execv(command[0], command)


def main():
    if os.geteuid() != 0:
        raise RuntimeError("Linux sandbox helper must run as root in private namespaces")
    if len(sys.argv) == 3 and sys.argv[1] == "--launch":
        launch(os.path.realpath(sys.argv[2]))
        return
    if len(sys.argv) != 3 or sys.argv[1] != "--inside":
        raise RuntimeError("usage: linux_store_sandbox.py --launch CONFIG.json")
    config_path = os.path.realpath(sys.argv[2])
    with open(config_path, "rb") as stream:
        config = json.load(stream)

    expected_namespaces = config.get("parent_namespace")
    if not isinstance(expected_namespaces, dict):
        raise RuntimeError("missing caller namespace identity")
    actual_namespaces = namespace_ids()
    if any(actual_namespaces[name] == int(expected_namespaces[name])
           for name in ("mnt", "net", "pid")):
        # Must fail before touching mount propagation or any mount point.
        raise RuntimeError("required private mount, network and PID namespaces are absent")
    root = os.path.realpath(config["root"])
    executable = os.path.realpath(config["executable"])
    uid, gid = int(config["uid"]), int(config["gid"])
    argv = list(config["argv"])
    env = dict(config["env"])
    if not argv or os.path.realpath(argv[0]) != executable:
        raise RuntimeError("sandbox argv[0] must be the explicitly allowed executable")
    if not os.path.isdir(root) or not os.path.isfile(executable):
        raise RuntimeError("sandbox root and executable must already exist")
    if os.path.dirname(config_path) != os.path.join(root, ".sandbox"):
        raise RuntimeError("sandbox config must be directly inside the disposable run root")
    # The config carries the child environment, including a synthetic fixture
    # key for encrypted switchbacks. Remove only this helper-owned config once
    # it has been validated, before entering the child process.
    os.unlink(config_path)

    mount(None, "/", None, MS_REC | MS_PRIVATE)
    readonly_mounts = readonly_writable_virtiofs()
    if os.path.ismount("/proc"):
        mount(None, "/proc", None, MS_REMOUNT | MS_RDONLY)

    record = {"status": "prepared",
              "mechanism": "private-mount-net-pid-namespaces+landlock-abi4",
              "read_only_virtiofs_mounts": readonly_mounts,
              "mount_namespace": os.readlink("/proc/self/ns/mnt"),
              "network_namespace": os.readlink("/proc/self/ns/net"),
              "pid_namespace": os.readlink("/proc/self/ns/pid"),
              "caller_namespace_ids": expected_namespaces,
              "sandbox_namespace_ids": actual_namespaces,
              "uid": uid, "gid": gid, "executable": config["executable"]}

    # Do not retain supplementary groups or capabilities across the uid change.
    os.setgroups([])
    os.setgid(gid)
    os.setuid(uid)
    version = restrict_landlock(config)
    record["landlock_abi"] = version
    record["no_new_privs"] = True
    record["status"] = "running"
    write_audit(config["audit_path"], record)
    os.chdir(config["cwd"])
    os.execve(argv[0], argv, env)


if __name__ == "__main__":
    try:
        if len(sys.argv) == 3 and sys.argv[1] == "--launch":
            launch(os.path.realpath(sys.argv[2]))
        else:
            main()
    except BaseException as error:
        print("linux sandbox setup failed: " + repr(error), file=sys.stderr, flush=True)
        sys.exit(125)

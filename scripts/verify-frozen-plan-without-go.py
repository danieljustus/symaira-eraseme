#!/usr/bin/env python3
"""Execute recorded whole plan and Unix process comparators with Go absent."""
import os
import json
import pathlib
import re
import shutil
import subprocess
import sys
import tempfile

assert sys.platform in ("linux", "win32", "darwin"), "unrecorded native host"
with tempfile.TemporaryDirectory(prefix="native-plan-no-go-") as temporary:
    environment = os.environ.copy()
    environment.pop("SYMERASEME_CAPTURE_PLAN_PROCESSES", None)
    environment["SYMERASEME_PARITY_LIVE_GO"] = "0"
    if sys.platform == "win32":
        # Retain native MSVC/Rust/Git locations, including their adjacent DLLs.
        # Go's SDK directories are removed only from this subprocess PATH.
        names = ("go", "go.exe", "go.cmd", "go.bat", "go.com", "gccgo.exe")
        environment["PATH"] = os.pathsep.join(directory for directory in os.environ["PATH"].split(os.pathsep)
            if directory and not any((pathlib.Path(directory) / name).is_file() for name in names))
        environment["NoDefaultCurrentDirectoryInExePath"] = "1"
    else:
        tools = pathlib.Path(temporary)
        for directory in os.environ["PATH"].split(os.pathsep):
            path = pathlib.Path(directory)
            if not directory or not path.is_dir():
                continue
            for program in path.iterdir():
                if program.name.startswith(("go", "gccgo")):
                    continue
                destination = tools / program.name
                if not destination.exists() and program.is_file() and os.access(program, os.X_OK):
                    destination.symlink_to(program.resolve())
        environment["PATH"] = str(tools)
        os.umask(0o022)
    assert shutil.which("go", path=environment["PATH"]) is None, "Go must actually be absent"
    cargo = shutil.which("cargo", path=environment["PATH"])
    assert cargo is not None and shutil.which("git", path=environment["PATH"]) is not None
    print("Verified: Go absent; both original whole-plan comparators execute frozen native records", flush=True)
    result = subprocess.run([cargo, "+1.98.0", "test", "-p", "symeraseme-cli", "--test", "plan_execute_live_process",
                             "--locked", "--offline"], env=environment, stdout=subprocess.PIPE, stderr=subprocess.STDOUT)
    assert len(result.stdout) <= 2 * 1024 * 1024, "bounded comparator log"
    sys.stdout.buffer.write(result.stdout)
    assert result.returncode == 0, "native Go-absent comparator failed"
    totals = re.findall(rb"test result: ok\. (\d+) passed; (\d+) failed; (\d+) ignored;", result.stdout)
    assert totals == [(b"2", b"0", b"0")], "both real cases must execute; no skips or zero-case acceptance"
    if sys.platform != "win32":
        print("Verified: Go absent; all three original Unix process comparators execute native records", flush=True)
        result = subprocess.run([cargo, "+1.98.0", "test", "-p", "symeraseme-cli",
                                 "--test", "triage_commands", "--test", "mcp_triage_process",
                                 "--test", "mcp_agent_error_json", "--locked", "--offline"],
                                env=environment, stdout=subprocess.PIPE, stderr=subprocess.STDOUT)
        assert len(result.stdout) <= 2 * 1024 * 1024, "bounded Unix comparator log"
        sys.stdout.buffer.write(result.stdout)
        assert result.returncode == 0, "native Go-absent Unix comparator failed"
        totals = re.findall(rb"test result: ok\. (\d+) passed; (\d+) failed; (\d+) ignored;", result.stdout)
        assert totals == [(b"1", b"0", b"0")] * 3, "all three real cases must execute"
    # The sole permitted retained Go runtime is the actual published rollback
    # sibling. Keep it outside PATH; no SDK or source build is needed here.
    retained = json.loads(subprocess.check_output(
        [sys.executable, "scripts/fetch-retained-go-release.py", "--output-dir",
         str(pathlib.Path(temporary).resolve() / "published-rollback")], env=environment))
    assert retained["sdk_required"] is False
    environment["SYMERASEME_ROLLBACK_GO_BINARY"] = retained["binary"]
    assert shutil.which("go", path=environment["PATH"]) is None
    print("Verified: Go SDK absent; real published rollback sibling selected only for explicit fallback", flush=True)
    result = subprocess.run([cargo, "+1.98.0", "test", "-p", "symeraseme-cli", "--test", "backend_fallback_process",
                             "--locked", "--offline"], env=environment, stdout=subprocess.PIPE, stderr=subprocess.STDOUT)
    assert len(result.stdout) <= 2 * 1024 * 1024, "bounded rollback comparator log"
    sys.stdout.buffer.write(result.stdout)
    assert result.returncode == 0, "actual published rollback comparator failed"
    totals = re.findall(rb"test result: ok\. (\d+) passed; (\d+) failed; (\d+) ignored;", result.stdout)
    assert totals == [(b"1", b"0", b"0")], "the original rollback case must execute"

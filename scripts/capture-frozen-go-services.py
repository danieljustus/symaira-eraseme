#!/usr/bin/env python3
"""Capture actual native Go service, projection, scheduler, config and DB data.

Never synthesizes an output or changes a committed fixture. The output directory
must be new, and a manifest is written only after every real oracle succeeds.
"""

import argparse
import concurrent.futures
import datetime
import hashlib
import json
import os
import pathlib
import platform
import shutil
import subprocess
import time


def digest(data):
    return {"bytes": len(data), "sha256": hashlib.sha256(data).hexdigest()}


def capture_process(command, cwd, environment, destination, deadline_seconds):
    stdout = destination.with_suffix(".stdout")
    stderr = destination.with_suffix(".stderr")
    with stdout.open("xb") as out, stderr.open("xb") as err:
        child = subprocess.Popen(command, cwd=cwd, env=environment,
                                 stdin=subprocess.DEVNULL, stdout=out, stderr=err)
        try:
            deadline = time.monotonic() + deadline_seconds
            while child.poll() is None:
                if stdout.stat().st_size > 1024 * 1024 or stderr.stat().st_size > 65536:
                    raise RuntimeError("capture exceeded its output limit")
                if time.monotonic() >= deadline:
                    raise TimeoutError("capture exceeded its existing oracle budget")
                time.sleep(0.025)
        finally:
            if child.poll() is None:
                child.kill()
            child.wait(timeout=5)
    output, errors = stdout.read_bytes(), stderr.read_bytes()
    assert len(output) <= 1024 * 1024 and len(errors) <= 65536
    return child.returncode, output, errors


def capture(command, cwd, environment, destination, deadline_seconds):
    status, output, errors = capture_process(command, cwd, environment, destination, deadline_seconds)
    assert status == 0, f"actual oracle failed; inspect {destination.with_suffix('.stderr')}"
    return output, errors


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--output", required=True, type=pathlib.Path)
    args = parser.parse_args()
    source = pathlib.Path(__file__).resolve().parent.parent
    assert not args.output.resolve().is_relative_to(source), "capture output must be outside the source checkout"
    go = shutil.which("go")
    git = shutil.which("git")
    assert go and git, "native pinned Go and git are required"
    revision = subprocess.check_output([git, "rev-parse", "HEAD"], cwd=source).decode().strip()
    assert not subprocess.check_output([git, "status", "--porcelain"], cwd=source), "clean source required"

    # Locate build caches before replacing HOME; disable Go's user config.
    query_environment = {key: os.environ[key] for key in
                         ("PATH", "HOME", "USERPROFILE", "APPDATA", "LOCALAPPDATA", "SystemRoot", "WINDIR")
                         if key in os.environ}
    query_environment.update(GOENV="off", GOTOOLCHAIN="local", GOWORK="off")
    info = json.loads(subprocess.check_output(
        [go, "env", "-json", "GOVERSION", "GOHOSTOS", "GOHOSTARCH", "GOOS", "GOARCH", "GOCACHE", "GOMODCACHE"],
        env=query_environment, timeout=10))
    assert info["GOVERSION"] == "go1.26.6"
    assert info["GOHOSTOS"] == info["GOOS"] and info["GOHOSTARCH"] == info["GOARCH"], "cross-compilation is not native evidence"
    assert info["GOHOSTARCH"] in ("amd64", "arm64")
    native_arch = {"x86_64": "amd64", "amd64": "amd64", "aarch64": "arm64", "arm64": "arm64"}[platform.machine().lower()]
    assert info["GOHOSTARCH"] == native_arch, "Go must execute the actual host architecture"
    assert info["GOHOSTOS"] == platform.system().lower(), "Go must execute the actual host OS"
    if info["GOHOSTOS"] != "windows":
        os.umask(0o022)  # The existing install fixture includes exact permission bits.
    args.output.mkdir(mode=0o700)  # Refuse existing files, directories and symlinks.
    output_root = args.output.resolve()
    private = output_root / "private"
    for name in ("home", "config", "cache", "data", "state", "temp"):
        (private / name).mkdir(mode=0o700, parents=True)
    environment = {key: query_environment[key] for key in ("PATH", "SystemRoot", "WINDIR")
                   if key in query_environment}
    environment.update(
        HOME=str(private / "home"), USERPROFILE=str(private / "home"),
        APPDATA=str(private / "config"), LOCALAPPDATA=str(private / "cache"),
        XDG_CONFIG_HOME=str(private / "config"), XDG_CACHE_HOME=str(private / "cache"),
        XDG_DATA_HOME=str(private / "data"), XDG_STATE_HOME=str(private / "state"),
        SYMERASEME_DATA_DIR=str(private / "data"), TMPDIR=str(private / "temp"),
        TEMP=str(private / "temp"), TMP=str(private / "temp"), TZ="UTC",
        GOENV="off", GOWORK="off", GOTOOLCHAIN="local", GOPROXY="off", GOSUMDB="off",
        GOCACHE=info["GOCACHE"], GOMODCACHE=info["GOMODCACHE"], CGO_ENABLED="0")
    manifest = {
        "schema": 1, "source_revision": revision, "go_version": info["GOVERSION"],
        "native_target": f'{info["GOHOSTOS"]}/{info["GOHOSTARCH"]}',
        "captured_at_utc": datetime.datetime.now(datetime.timezone.utc).isoformat(),
        "source_files": {}, "observations": [],
        "limits": "Actual native capture only; review observations before committing fixtures. Root children are reaped; no compiler-descendant confinement claim.",
    }
    sources = [source / "go.mod", source / "go.sum"]
    for directory in ("cmd", "internal", "internal/eventstore", "internal/triage", "internal/replies", "internal/llm",
                      "internal/timeutil", "internal/scheduler", "internal/config", "internal/campaign",
                      "internal/identity", "internal/manualtasks", "internal/registry", "rust-tests/parity/oracle/triage-service",
                      "rust-tests/parity/oracle/projection", "rust-tests/parity/oracle/scheduler-install", "rust-tests/parity/oracle/config",
                      "rust-tests/parity/oracle/campaign-execution", "rust-tests/parity/oracle/storage",
                      "rust-tests/parity/oracle/llm-failures-next"):
        sources.extend(sorted((source / directory).rglob("*.go")))
    cases_path = source / "rust-tests/parity/oracle/projection/cases.json"
    sources.append(cases_path)
    sources += [source / "rust-tests/parity/oracle/config/inputs.json", source / "rust-tests/parity/oracle/config/config_cases.json"]
    plan_fixture = source / "tests/fixtures/event-store/campaign-plan-bytes-oracle.json"
    sources += [source / "rust-tests/parity/oracle/campaign-execution/cases.json", plan_fixture,
                source / "tests/fixtures/event-store/golden-campaign.db", source / "registry/brokers/eu/adventori-eu.yaml"]
    sources += sorted((source / "tests/fixtures/registry-contract").glob("golden-*.yaml"))
    # Bind the additional runtime oracles and every embedded broker document.
    sources += [source / ("rust-tests/parity/oracle/" + name + "/main.go")
                for name in ("cli-schedule", "mcp-clock", "mcp-auto-confirm", "mcp-tool-gaps")]
    sources += sorted((source / "registry/brokers").rglob("*.yaml"))
    for path in sources:
        manifest["source_files"][path.relative_to(source).as_posix()] = digest(path.read_bytes())
    manifest["controls"] = []
    for package, test_path, tags, test_name in (
        ("campaign-plan-go-test", "./internal/campaign", [], "^TestCampaignPlanBytesOracle$"),
        ("storage-go-test", "./rust-tests/parity/oracle/storage", ["-tags", "storage_oracle"], None),
    ):
        command = [go, "test", "-mod=readonly", *tags, test_path, "-count=1", "-json"]
        if test_name:
            command += ["-run", test_name]
        output, errors = capture(command, source, environment, output_root / package, 120)
        assert not errors
        records = [json.loads(line) for line in output.splitlines()]
        passed = [record["Test"] for record in records if record.get("Action") == "pass" and record.get("Test")]
        assert passed and not any(record.get("Action") == "fail" for record in records)
        metadata = {"package": package, "exit_status": 0, "stdout": digest(output),
                    "stderr": digest(errors), "executed_pass_tests": passed}
        if test_name:
            assert passed == ["TestCampaignPlanBytesOracle"]
            fixture = plan_fixture.read_bytes()
            assert digest(fixture) == manifest["source_files"][plan_fixture.relative_to(source).as_posix()]
            (output_root / "campaign-plan.observations.json").write_bytes(fixture)
            metadata["fixture"] = digest(fixture)
        manifest["controls"].append(metadata)
    for package in ("triage-service", "projection", "scheduler-install", "config", "campaign-execution", "storage"):
        binary = output_root / (package + "-oracle" + (".exe" if info["GOHOSTOS"] == "windows" else ""))
        # runtime.Caller locates projection/cases.json: do not trim its path.
        tags = ["-tags", "storage_oracle"] if package == "storage" else []
        capture([go, "build", "-mod=readonly", "-buildvcs=true", *tags, "-o", str(binary),
                 "./rust-tests/parity/oracle/" + package], source, environment,
                output_root / (package + "-build"), 30 if package == "config" else 120)
        build_info = subprocess.check_output([go, "version", "-m", str(binary)], env=environment, timeout=10)
        assert ("vcs.revision=" + revision).encode() in build_info and b"vcs.modified=false" in build_info
        command = [str(binary)]
        if package == "scheduler-install":
            command += ["-fixture", str(output_root / "scheduler-install.observations.json"), "-revision", revision]
        output, errors = capture(command, source, environment, output_root / package, 30)
        assert not errors
        fixture_metadata = {}
        if package == "scheduler-install":
            fixture_bytes = (output_root / "scheduler-install.observations.json").read_bytes()
            assert len(fixture_bytes) <= 1024 * 1024
            observed = json.loads(fixture_bytes)
            names = sorted(observed["cases"])
            assert len(names) == 20 and observed["source_revision"] == revision
            fixture_metadata = {"fixture": digest(fixture_bytes)}
        elif package == "projection":
            observed = json.loads(output)
            names = [case["name"] for case in json.loads(cases_path.read_bytes())]
            assert set(observed["cases"]) == set(names) and len(names) == 7
        elif package == "config":
            observed = json.loads(output)
            names = [f"CFG-{number:03}" for number in range(1, 7)]
            assert set(observed["cases"]) == set(names)
            assert observed["provenance"]["source_sha256"] == manifest["source_files"]["internal/config/config.go"]["sha256"]
        elif package == "campaign-execution":
            observed = json.loads(output)
            names = ["execution_transitions"]
            assert set(observed) == {"plan_before", "plan_after", "result", "events", "statuses",
                                     "fake_send_plan_before", "fake_send_plan_after", "fake_send_result", "fake_send_events"}
        elif package == "storage":
            observed = json.loads(output)
            names = ["fresh", "golden"]
            assert set(observed) == {"provenance", "fresh", "golden"}
            provenance = observed["provenance"]
            archived = subprocess.check_output([git, "show", provenance["fixture_archive_commit"] + ":" +
                                               provenance["fixture_generator_path"]], cwd=source, timeout=10)
            assert digest(archived)["sha256"] == provenance["fixture_generator_sha256"]
            manifest["archived_sources"] = {provenance["fixture_generator_path"]: {
                **digest(archived), "revision": provenance["fixture_archive_commit"],
                "git_blob": provenance["fixture_generator_git_blob"]}}
        else:
            observed = json.loads(output)
            names = ["classify", "rebuttal", "fallback", "llm_error"]
            assert set(observed) == set(names) | {"source_sha256"}
        manifest["observations"].append({
            "package": package, "cases": names, "exit_status": 0, "stdin": digest(b""),
            "stdout": digest(output), "stderr": digest(errors), "binary": digest(binary.read_bytes()),
            "embedded_build_info": build_info.decode(),
            **fixture_metadata,
        })
    llm_binary = output_root / ("llm-failures-next-oracle" + (".exe" if info["GOHOSTOS"] == "windows" else ""))
    capture([go, "build", "-mod=readonly", "-buildvcs=true", "-o", str(llm_binary),
             "./rust-tests/parity/oracle/llm-failures-next"], source, environment,
            output_root / "llm-failures-next-build", 120)
    llm_build_info = subprocess.check_output([go, "version", "-m", str(llm_binary)], env=environment, timeout=10)
    assert ("vcs.revision=" + revision).encode() in llm_build_info and b"vcs.modified=false" in llm_build_info
    llm_cases = [("case", []), ("case-404", ["--status", "404"]), ("case-400", ["--status", "400"]),
                 ("case-500", ["--status", "500"]), ("case-401", ["--status", "401"]),
                 ("malformed-envelope", ["--status", "403", "--malformed-envelope"]),
                 ("malformed-choice", ["--status", "403", "--malformed-choice"])]

    def observe_llm(case):
        name, arguments = case
        output, errors = capture([str(llm_binary), *arguments], source, environment,
                                 output_root / ("llm-failures-next-" + name), 30)
        assert not errors
        assert output == (source / "tests/fixtures/llm-failures-next" / (name + ".json")).read_bytes()
        return {"name": name, "args": arguments, "exit_status": 0,
                "stdout": digest(output), "stderr": digest(errors)}

    with concurrent.futures.ThreadPoolExecutor(max_workers=7) as pool:
        llm_observations = list(pool.map(observe_llm, llm_cases))
    manifest["llm_failures"] = {"embedded_build_info": llm_build_info.decode(), "cases": llm_observations}
    review_binary = output_root / ("cli-review-oracle" + (".exe" if info["GOHOSTOS"] == "windows" else ""))
    capture([go, "build", "-mod=readonly", "-buildvcs=true", "-o", str(review_binary), "./cmd/symeraseme"],
            source, environment, output_root / "cli-review-build", 120)
    review_build_info = subprocess.check_output([go, "version", "-m", str(review_binary)], env=environment, timeout=10)
    assert ("vcs.revision=" + revision).encode() in review_build_info and b"vcs.modified=false" in review_build_info
    review_cwd = private / "review-cwd"
    review_cwd.mkdir(mode=0o700)
    original = b"Alice Example <alice@example.invalid>\n"
    review_input = review_cwd / "review.txt"
    review_input.write_bytes(original)
    review_environment = {"HOME": str(private / "home"), "USERPROFILE": str(private / "home"),
                          "LC_ALL": "C", "TZ": "UTC", "PWD": str(review_cwd)}
    review_arguments = [["review"], ["review", "--path", "review.txt", "--output", "json"],
                        ["review", "review.txt", "--path", "missing.txt", "--output", "json"],
                        ["review", "missing.txt", "--path", "review.txt"], ["review", "review.txt"],
                        ["--output", "json", "review", "review.txt"],
                        ["review", "--path", "missing.txt", "--output", "json"],
                        ["review", "review.txt", "--output", "yaml"]]
    review_cases = []
    for index, arguments in enumerate(review_arguments):
        status, output, errors = capture_process([str(review_binary), *arguments], review_cwd, review_environment,
                                                output_root / ("cli-review-case-" + str(index)), 10)
        assert status in (0, 1), "actual review CLI unexpected exit"
        assert review_input.read_bytes() == original, "actual Go review changed its input"
        review_cases.append({"argv": arguments, "exit_status": status, "stdout": digest(output), "stderr": digest(errors)})
    manifest["cli_review"] = {"embedded_build_info": review_build_info.decode(), "cases": review_cases,
                              "input_unchanged": True, "input": digest(original)}
    grant_data = private / "grant-data"
    grant_data.mkdir(mode=0o700)
    grant_environment = {**review_environment, "SYMERASEME_DATA_DIR": str(grant_data)}
    grant_arguments = [["grant", "--output", "json"], ["grant", "--list", "--output", "json"],
                       ["grant", "--command", "ignored", "send-removal", "--ttl", "60", "--output", "json"],
                       None, ["grant", "--revoke-all", "--output", "json"],
                       ["grant", "--list-tokens", "--output", "json"]]
    grant_cases = []
    first_token = None
    for index, arguments in enumerate(grant_arguments):
        if index == 3:
            arguments = ["grant", "--revoke", first_token, "--output", "json"]
        status, output, errors = capture_process([str(review_binary), *arguments], review_cwd, grant_environment,
                                                output_root / ("cli-grant-case-" + str(index)), 10)
        assert status == 0 and not errors, "actual native Go grant failed"
        response = json.loads(output)
        if index == 0:
            first_token = response["token"]
        records = []
        for number, path in enumerate(sorted(grant_data.iterdir())):
            assert path.is_file() and not path.is_symlink()
            content = path.read_bytes()
            record = json.loads(content)
            expected_name = "consent_" + hashlib.sha256(record["token"].encode()).hexdigest()[:16] + ".json"
            assert path.name == expected_name
            mode = format(path.stat().st_mode & 0o777, "04o")
            if info["GOHOSTOS"] != "windows":
                assert mode == "0600"
            assert record["expires_at"] - record["issued_at"] == (60 if record["command"] == "send-removal" else 86400)
            filename = "cli-grant-case-" + str(index) + "-consent-" + str(number) + ".observations.json"
            (output_root / filename).write_bytes(content)
            records.append({"file": filename, "name": path.name, "mode": mode, **digest(content)})
        assert len(records) == [1, 1, 2, 1, 0, 0][index]
        grant_cases.append({"argv": arguments, "exit_status": status, "stdout": digest(output),
                            "stderr": digest(errors), "consent_records": records})
    manifest["cli_grants"] = {"embedded_build_info": review_build_info.decode(), "cases": grant_cases}
    manifest["runtime_oracles"] = []
    for package in ("cli-schedule", "mcp-clock", "mcp-auto-confirm", "mcp-tool-gaps"):
        binary = output_root / (package + "-oracle" + (".exe" if info["GOHOSTOS"] == "windows" else ""))
        capture([go, "build", "-mod=readonly", "-buildvcs=true", "-o", str(binary),
                 "./rust-tests/parity/oracle/" + package], source, environment,
                output_root / (package + "-build"), 120)
        build_info = subprocess.check_output([go, "version", "-m", str(binary)], env=environment, timeout=10)
        for expected in ("vcs.revision=" + revision, "vcs.modified=false",
                         "GOOS=" + info["GOHOSTOS"], "GOARCH=" + info["GOHOSTARCH"]):
            assert expected.encode() in build_info
        runtime_environment = environment.copy()
        cwd = source
        isolation = {}
        if package == "mcp-clock":
            hostile = private / "mcp-clock-hostile-project"
            hostile.mkdir(mode=0o700)
            hostile_db = private / "mcp-clock-forbidden-project-db"
            hostile_env = private / "mcp-clock-forbidden-inherited-data"
            # JSON quoted paths are valid TOML strings, including Windows backslashes.
            (hostile / ".symeraseme.toml").write_text("db_dir = " + json.dumps(str(hostile_db)) + "\n")
            runtime_environment.update(SYMERASEME_DB_DIR=str(hostile_env / "db"),
                                       SYMERASEME_DATA_DIR=str(hostile_env / "data"))
            cwd = hostile
        output, errors = capture([str(binary)], cwd, runtime_environment, output_root / package, 30)
        observed = json.loads(output)
        if package == "cli-schedule":
            assert observed["schema"] == "symeraseme.go-oracle.cli.v1"
            cases = observed["cases"]
            assert len(cases) == 13 and len({c["id"] for c in cases}) == 13
            import base64
            for case in cases:
                for stream in ("stdout", "stderr"):
                    raw = base64.b64decode(case[stream + "_base64"], validate=True)
                    assert len(raw) == case[stream + "_bytes"]
                assert case["exit_code"] in (0, 1) and isinstance(case["files"], dict)
            names = [case["id"] for case in cases]
        elif package == "mcp-clock":
            assert not errors and not hostile_db.exists() and not hostile_env.exists()
            assert observed["oracle_source"] == "live current-checkout Go ContractHandler"
            assert observed["now"] == "2026-08-05T12:00:00Z" and len(observed["cases"]) == 6
            names = [case["name"] for case in observed["cases"]]
            isolation = {"hostile_project_db_absent": True, "hostile_inherited_data_absent": True}
        elif package == "mcp-auto-confirm":
            assert not errors and observed["go_version"] == "go1.26.6"
            names = ["dry_run_response", "manual_response", "no_links_dry_response", "no_links_response"]
            assert all(isinstance(observed[name], str) and json.loads(observed[name]) for name in names)
            assert len(observed["sources_sha256"]) == 7
        else:
            assert not errors and observed["schema"] == "symeraseme.go-oracle.mcp-tool-gaps.v1"
            assert observed["go_version"] == "go1.26.6" and len(observed["cases"]) == 25
            assert len(observed["sources_sha256"]) == 9
            names = [case["name"] for case in observed["cases"]]
        if "sources_sha256" in observed:
            for path, expected in observed["sources_sha256"].items():
                assert path in manifest["source_files"]
                assert manifest["source_files"][path]["sha256"] == expected
        assert len(set(names)) == len(names)
        manifest["runtime_oracles"].append({
            "package": package, "cases": names, "exit_status": 0, "stdin": digest(b""),
            "stdout": digest(output), "stderr": digest(errors), "binary": digest(binary.read_bytes()),
            "embedded_build_info": build_info.decode(), **isolation,
        })
    assert not subprocess.check_output([git, "status", "--porcelain"], cwd=source), "capture changed source"
    (output_root / "manifest.json").write_text(json.dumps(manifest, indent=2) + "\n")
    print(json.dumps({"native_target": manifest["native_target"], "source_revision": revision,
                      "operations": 110, "status": "actual observations captured"}))


if __name__ == "__main__":
    main()

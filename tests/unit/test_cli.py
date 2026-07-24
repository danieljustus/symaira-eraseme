"""Basic CLI smoke tests."""

import json
import re
from pathlib import Path

import pytest
from typer.testing import CliRunner

from symeraseme.cli import app
from symeraseme.cli.types import CliResult

runner = CliRunner()


def _strip_ansi(text: str) -> str:
    return re.sub(r"\x1b\[[0-9;]*m", "", text)


def test_version() -> None:
    result = runner.invoke(app, ["version"])
    assert result.exit_code == 0
    assert "Symaira EraseMe" in result.stdout


def test_help() -> None:
    result = runner.invoke(app, ["--help"])
    assert result.exit_code == 0
    assert "symeraseme" in result.stdout


class TestGrant:
    @staticmethod
    def _setup(monkeypatch, tmp_path) -> Path:
        consent_dir = tmp_path / "consent"
        consent_dir.mkdir()
        monkeypatch.setenv("SYMERASEME_DATA_DIR", str(consent_dir))
        return consent_dir

    def test_grant_issues_token(self, monkeypatch, tmp_path):
        self._setup(monkeypatch, tmp_path)
        result = runner.invoke(app, ["grant", "execute"])
        assert result.exit_code == 0
        assert "Consent token:" in result.stdout

    def test_grant_with_ttl(self, monkeypatch, tmp_path):
        self._setup(monkeypatch, tmp_path)
        result = runner.invoke(app, ["grant", "execute", "--ttl", "3600"])
        assert result.exit_code == 0
        assert "TTL: 3600s" in result.stdout

    def test_grant_list_empty(self, monkeypatch, tmp_path):
        self._setup(monkeypatch, tmp_path)
        result = runner.invoke(app, ["grant", "--list"])
        assert result.exit_code == 0
        assert "No active tokens" in result.stdout

    def test_grant_revoke_nonexistent(self, monkeypatch, tmp_path):
        self._setup(monkeypatch, tmp_path)
        result = runner.invoke(app, ["grant", "--revoke", "nonexistent"])
        assert result.exit_code != 0
        assert "Token not found" in result.stderr

    def test_grant_revoke_all_empty(self, monkeypatch, tmp_path):
        self._setup(monkeypatch, tmp_path)
        result = runner.invoke(app, ["grant", "--revoke-all"])
        assert result.exit_code == 0
        assert "No active tokens to revoke" in result.stdout


class TestCliResult:
    """Tests for the structured CliResult type."""

    def test_defaults(self) -> None:
        r = CliResult()
        assert r.success is True
        assert r.data == {}
        assert r.error is None
        assert r.message == ""
        assert r.error_exit_code is None

    def test_with_message(self) -> None:
        r = CliResult(data={"message": "done"})
        assert r.message == "done"

    def test_with_error(self) -> None:
        r = CliResult(success=False, error="something went wrong")
        assert r.success is False
        assert r.message == "something went wrong"

    def test_with_error_exit_code(self) -> None:
        r = CliResult(success=False, error="fail", error_exit_code=2)
        assert r.error_exit_code == 2

    def test_to_json_success(self) -> None:
        r = CliResult(data={"message": "ok", "count": 3})
        j = r.to_json()
        assert '"success": true' in j
        assert '"message": "ok"' in j
        assert '"count": 3' in j

    def test_to_json_error(self) -> None:
        r = CliResult(success=False, error="fail")
        j = r.to_json()
        assert '"success": false' in j
        assert '"error": "fail"' in j

    def test_to_json_serializes_dataclass(self) -> None:
        from dataclasses import dataclass

        @dataclass
        class Broker:
            name: str
            count: int

        r = CliResult(data={"brokers": [Broker(name="A", count=1)]})
        j = r.to_json()
        parsed = json.loads(j)
        assert parsed["brokers"][0]["name"] == "A"
        assert parsed["brokers"][0]["count"] == 1


class TestVerboseLogging:
    def _reset_logging(self):
        import logging

        logging.root.setLevel(logging.WARNING)
        for handler in logging.root.handlers[:]:
            logging.root.removeHandler(handler)
        logging.getLogger("symeraseme").setLevel(logging.NOTSET)

    def test_verbose_scopes_debug_to_symeraseme(self):
        import logging

        self._reset_logging()
        runner = CliRunner()
        result = runner.invoke(app, ["-vv", "version"])
        assert result.exit_code == 0
        symeraseme_logger = logging.getLogger("symeraseme")
        assert symeraseme_logger.getEffectiveLevel() == logging.DEBUG, (
            f"Expected DEBUG (10), got {symeraseme_logger.level} "
            f"(effective: {symeraseme_logger.getEffectiveLevel()})"
        )
        library_logger = logging.getLogger("urllib3")
        assert library_logger.level != logging.DEBUG

    def test_no_verbose_uses_warning(self):
        import logging

        self._reset_logging()
        runner = CliRunner()
        result = runner.invoke(app, ["version"])
        assert result.exit_code == 0
        assert logging.getLogger("symeraseme").level != logging.DEBUG
        assert logging.getLogger("symeraseme").level != logging.INFO


class TestJsonOutput:
    def test_version_json(self) -> None:
        result = runner.invoke(app, ["--output", "json", "version"])
        assert result.exit_code == 0
        parsed = json.loads(result.stdout)
        assert "success" in parsed
        assert parsed["success"] is True

    def test_brokers_list_json(self) -> None:
        result = runner.invoke(app, ["--output", "json", "brokers", "list"])
        assert result.exit_code == 0
        parsed = json.loads(result.stdout)
        assert "success" in parsed


class TestServe:
    def test_serve_allows_loopback_without_flag(self):
        result = runner.invoke(app, ["serve", "--help"])
        stdout = _strip_ansi(result.stdout)
        assert result.exit_code == 0
        assert "127.0.0.1" in stdout
        assert "--allow-remote" in stdout
        assert "unauthenticated" in stdout

    def test_serve_rejects_non_loopback_without_flag(self):
        result = runner.invoke(app, ["serve", "--host", "0.0.0.0"])
        stderr = _strip_ansi(result.stderr)
        assert result.exit_code == 1
        assert "Refusing to bind" in stderr
        assert "--allow-remote" in stderr

    def test_serve_allows_non_loopback_with_flag(self):
        result = runner.invoke(app, ["serve", "--host", "0.0.0.0", "--allow-remote", "--help"])
        stdout = _strip_ansi(result.stdout)
        assert result.exit_code == 0
        assert "--allow-remote" in stdout


class TestExceptionGuard:
    """Tests for the top-level exception guard (_run_app)."""

    def test_unexpected_exception_logs_and_prints_friendly(self, monkeypatch, tmp_path, capsys):
        import sys

        import typer

        monkeypatch.setenv("SYMERASEME_DATA_DIR", str(tmp_path))

        def _boom(**kwargs):
            raise RuntimeError("simulated unexpected failure")

        mod = sys.modules["symeraseme.cli.app"]
        monkeypatch.setattr(mod, "app", _boom)

        from symeraseme.cli.app import _run_app

        with pytest.raises(typer.Exit):
            _run_app()

        stderr_text = capsys.readouterr().err
        assert "unexpected error" in stderr_text.lower()
        assert "Traceback" not in stderr_text
        assert "RuntimeError" not in stderr_text

        log_dir = tmp_path / "logs"
        assert log_dir.exists(), f"Log dir not created: {log_dir}"
        log_files = list(log_dir.glob("crash-*.log"))
        assert len(log_files) == 1, f"Expected 1 crash log, found {len(log_files)}"
        log_content = log_files[0].read_text()
        assert "RuntimeError" in log_content
        assert "simulated unexpected failure" in log_content

    def test_cliresult_errors_use_explicit_exit_code(self):
        """Exit codes are now explicit on SymerasemeError subclasses."""
        from symeraseme.cli.console import _exit_code_for_result
        from symeraseme.cli.types import CliResult
        from symeraseme.core.exceptions import EXIT_CONFIG, EXIT_ERROR

        # CliResult without explicit exit_code → falls back to EXIT_ERROR
        r = CliResult(success=False, error="something went wrong")
        assert _exit_code_for_result(r) == EXIT_ERROR

        # CliResult with explicit exit_code → uses that code
        r2 = CliResult(success=False, error="profile missing", error_exit_code=EXIT_CONFIG)
        assert _exit_code_for_result(r2) == EXIT_CONFIG

    def test_pretty_exceptions_disabled(self):
        assert app.pretty_exceptions_enable is False


class TestErrorExitCodes:
    """Exit codes on SymerasemeError subclasses match expected behavior."""

    def test_profile_error_has_config_exit_code(self):
        from symeraseme.core.exceptions import EXIT_CONFIG, ProfileError

        err = ProfileError("no profile")
        assert err.exit_code == EXIT_CONFIG

    def test_registry_error_has_error_exit_code(self):
        from symeraseme.core.exceptions import EXIT_ERROR, RegistryError

        err = RegistryError("registry not found")
        assert err.exit_code == EXIT_ERROR

    def test_execution_error_has_error_exit_code(self):
        from symeraseme.core.exceptions import EXIT_ERROR, ExecutionError

        err = ExecutionError("send failed", request_id=1)
        assert err.exit_code == EXIT_ERROR

    def test_request_not_found_has_error_exit_code(self):
        from symeraseme.core.exceptions import EXIT_ERROR, RequestNotFoundError

        err = RequestNotFoundError(42)
        assert err.exit_code == EXIT_ERROR


class TestCommandDocstrings:
    """All user-facing commands should have non-empty docstrings for --help."""

    @pytest.mark.parametrize(
        "module_path,name",
        [
            ("symeraseme.cli.commands.account_commands", "show_profile"),
            ("symeraseme.cli.commands.plan_commands", "plan_show"),
            ("symeraseme.cli.commands.monitoring_commands", "poll_inbox"),
            ("symeraseme.cli.commands.monitoring_commands", "classify_reply"),
            ("symeraseme.cli.commands.monitoring_commands", "generate_rebuttal_cmd"),
            ("symeraseme.cli.commands.monitoring_commands", "generate_dashboard_cmd"),
            ("symeraseme.cli.commands.monitoring_commands", "generate_report_cmd"),
            ("symeraseme.cli.commands.web_form_commands", "run_web_form"),
            ("symeraseme.cli.commands.web_form_commands", "auto_confirm_cmd"),
            ("symeraseme.cli.commands.web_form_commands", "solve_captcha_cmd"),
            ("symeraseme.cli.commands.inspection_commands", "events_show"),
            ("symeraseme.cli.commands.inspection_commands", "requests_list"),
            ("symeraseme.cli.commands.inspection_commands", "version"),
        ],
    )
    def test_command_has_docstring(self, module_path, name):
        import importlib

        mod = importlib.import_module(module_path)
        func = getattr(mod, name)
        assert func.__doc__, f"{name} is missing a docstring"

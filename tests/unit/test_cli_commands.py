"""Focused CLI coverage additions.

Targets specific uncovered code paths across all command modules.
Uses lazy app import and correct mock paths.
"""

from __future__ import annotations

import json
from pathlib import Path
from unittest.mock import AsyncMock, patch

import pytest
import typer
from typer.testing import CliRunner

from symeraseme.cli.console import (
    OutputFormat,
    _exit_code_for_result,
    print_error,
    print_info,
    print_success,
    print_warning,
    render_error,
    render_result,
)
from symeraseme.cli.types import CliResult
from symeraseme.core.exceptions import EXIT_CONFIG, EXIT_ERROR, ProfileError, SymerasemeError

runner = CliRunner()


def _setup(monkeypatch, tmp_path) -> Path:
    data_dir = tmp_path / "symeraseme"
    data_dir.mkdir(parents=True)
    monkeypatch.setenv("SYMERASEME_DATA_DIR", str(data_dir))
    return data_dir


def _make_consent_dir(monkeypatch, tmp_path) -> Path:
    data_dir = _setup(monkeypatch, tmp_path)
    d = data_dir / "consent"
    d.mkdir(parents=True, exist_ok=True)
    return d


def _strip_ansi(text: str) -> str:
    import re
    return re.sub(r"\x1b\[[0-9;]*m", "", text)


# ═══════════════════════════════════════════════════════════════════════════
# Console helpers (covered via direct calls)
# ═══════════════════════════════════════════════════════════════════════════


class TestConsoleHelpers:
    def test_print_success(self, capsys):
        print_success("it worked")
        assert "it worked" in capsys.readouterr().out

    def test_print_info(self, capsys):
        print_info("info message")
        assert "info message" in capsys.readouterr().out

    def test_print_warning(self, capsys):
        print_warning("warning message")
        assert "warning message" in capsys.readouterr().out

    def test_print_error(self, capsys):
        print_error("error message")
        assert "error message" in capsys.readouterr().err

    def test_render_result_multiline(self, capsys):
        render_result("text", "line1\nline2\nline3")
        assert "line1" in capsys.readouterr().out or "Output" in capsys.readouterr().out

    def test_render_result_exception(self):
        with pytest.raises(typer.Exit) as exc:
            render_result("text", RuntimeError("boom"))
        assert exc.value.exit_code == EXIT_ERROR

    def test_render_result_symeraseme_error(self, capsys):
        with pytest.raises(typer.Exit) as exc:
            render_result("text", SymerasemeError("domain error"))
        assert exc.value.exit_code == EXIT_ERROR
        assert "domain error" in capsys.readouterr().err

    def test_render_result_profile_error(self, capsys):
        with pytest.raises(typer.Exit) as exc:
            render_result("text", ProfileError("profile missing"))
        assert exc.value.exit_code == EXIT_CONFIG

    def test_render_error_function(self, capsys):
        with pytest.raises(typer.Exit) as exc:
            render_error("fatal error")
        assert exc.value.exit_code == EXIT_ERROR

    def test_exit_code_for_result(self):
        assert _exit_code_for_result(CliResult(success=False, error="err")) == EXIT_ERROR
        assert _exit_code_for_result(CliResult(success=False, error="err", error_exit_code=42)) == 42

    def test_output_format_values(self):
        assert OutputFormat.text.value == "text"
        assert OutputFormat.json.value == "json"

    def test_render_text_success(self, capsys):
        render_result("text", CliResult(success=True, data={"message": "all good"}))
        assert "all good" in capsys.readouterr().out

    def test_render_text_single_line(self, capsys):
        render_result("text", "hello world")
        assert "hello world" in capsys.readouterr().out

    def test_render_result_json_with_cliresult(self, capsys):
        render_result("json", CliResult(success=True, data={"key": "val"}))

    def test_render_result_text_failure_stderr(self, capsys):
        with pytest.raises(typer.Exit) as exc:
            render_result("text", CliResult(success=False, error="failed task"))
        assert exc.value.exit_code == EXIT_ERROR

    def test_render_result_json_failure(self, capsys):
        with pytest.raises(typer.Exit) as exc:
            render_result("json", CliResult(success=False, error="json fail"))
        assert exc.value.exit_code == EXIT_ERROR


# ═══════════════════════════════════════════════════════════════════════════
# CliResult edge cases
# ═══════════════════════════════════════════════════════════════════════════


class TestCliResultEdgeCases:
    def test_data_is_list(self):
        r = CliResult(success=True, data=[1, 2, 3])
        parsed = json.loads(r.to_json())
        assert parsed["data"] == [1, 2, 3]

    def test_no_message_no_error(self):
        assert CliResult(success=True, data={}).message == ""

    def test_spreads_data_fields(self):
        parsed = json.loads(CliResult(success=True, data={"custom_field": 42, "message": "done"}).to_json())
        assert parsed["custom_field"] == 42
        assert parsed["message"] == "done"

    def test_error_strips_message(self):
        parsed = json.loads(CliResult(success=False, data={"message": "hidden", "id": 1}, error="real error").to_json())
        assert parsed["error"] == "real error"
        assert "message" not in parsed
        assert parsed["id"] == 1

    def test_message_sources(self):
        assert CliResult(success=True, data={"message": "from data"}).message == "from data"
        assert CliResult(success=False, error="error msg").message == "error msg"
        assert CliResult(success=True, data=[1, 2], error="err msg").message == "err msg"

    def test_success_defaults(self):
        r = CliResult()
        assert r.success is True
        assert r.data == {}
        assert r.error is None


# ═══════════════════════════════════════════════════════════════════════════
# Doctor check functions (direct unit tests)
# ═══════════════════════════════════════════════════════════════════════════


class TestDoctorChecks:
    def test_check_python_version(self):
        from symeraseme.cli.commands.inspection_commands import _check_python_version
        ok, msg = _check_python_version()
        assert ok is True and "Python" in msg

    def test_check_deps(self):
        from symeraseme.cli.commands.inspection_commands import _check_deps
        ok, msg = _check_deps()
        assert ok is True and "installed" in msg

    def test_check_deps_missing(self):
        from symeraseme.cli.commands.inspection_commands import _check_deps
        with patch("builtins.__import__", side_effect=ImportError("missing")):
            ok, msg = _check_deps()
            assert ok is False and "Missing" in msg

    def test_check_config(self, monkeypatch, tmp_path):
        from symeraseme.cli.commands.inspection_commands import _check_config
        _setup(monkeypatch, tmp_path)
        ok, msg = _check_config()
        assert isinstance(ok, bool) and isinstance(msg, str)

    def test_check_database(self, monkeypatch, tmp_path):
        from symeraseme.cli.commands.inspection_commands import _check_database
        _setup(monkeypatch, tmp_path)
        ok, msg = _check_database()
        assert isinstance(ok, bool) and isinstance(msg, str)

    def test_check_registry(self):
        from symeraseme.cli.commands.inspection_commands import _check_registry
        ok, msg = _check_registry()
        assert isinstance(ok, bool) and isinstance(msg, str)

    def test_check_keyring(self):
        from symeraseme.cli.commands.inspection_commands import _check_keyring
        ok, msg = _check_keyring()
        assert isinstance(ok, bool) and isinstance(msg, str)

    def test_check_llm_variants(self, monkeypatch):
        from symeraseme.cli.commands.inspection_commands import _check_llm

        monkeypatch.delenv("SYMERASEME_LLM_PROVIDER", raising=False)
        monkeypatch.delenv("ANTHROPIC_API_KEY", raising=False)
        monkeypatch.delenv("OPENAI_API_KEY", raising=False)
        ok, msg = _check_llm()
        assert isinstance(ok, bool) and isinstance(msg, str)

        monkeypatch.setenv("SYMERASEME_LLM_PROVIDER", "ollama")
        ok, msg = _check_llm()
        assert ok is True

        monkeypatch.setenv("SYMERASEME_LLM_PROVIDER", "unknown-provider")
        ok, msg = _check_llm()
        assert ok is False and "unknown provider" in msg

        monkeypatch.setenv("SYMERASEME_LLM_PROVIDER", "openai")
        monkeypatch.setenv("OPENAI_API_KEY", "sk-test")
        monkeypatch.setenv("SYMERASEME_LLM_MODEL", "gpt-4")
        ok, msg = _check_llm()
        assert ok is True and "model=gpt-4" in msg

        monkeypatch.setenv("SYMERASEME_LLM_PROVIDER", "openai")
        monkeypatch.delenv("OPENAI_API_KEY", raising=False)
        ok, msg = _check_llm()
        assert ok is False and "OPENAI_API_KEY" in msg

    def test_check_env(self, monkeypatch):
        from symeraseme.cli.commands.inspection_commands import _check_env
        monkeypatch.setenv("SYMERASEME_LLM_PROVIDER", "anthropic")
        ok, msg = _check_env()
        assert ok is True and "LLM provider" in msg

        monkeypatch.setenv("MY_API_KEY", "secret")
        ok, msg = _check_env()
        assert ok is True and "credentials" in msg.lower()

    def test_is_sensitive_env_var(self):
        from symeraseme.cli.commands.inspection_commands import _is_sensitive_env_var
        assert _is_sensitive_env_var("MY_API_KEY") is True
        assert _is_sensitive_env_var("MY_PASSWORD") is True
        assert _is_sensitive_env_var("MY_SECRET") is True
        assert _is_sensitive_env_var("MY_TOKEN") is True
        assert _is_sensitive_env_var("MY_CREDENTIAL") is True
        assert _is_sensitive_env_var("MY_NAME") is False

    def test_check_db_encryption_not_enabled(self, monkeypatch, tmp_path):
        from symeraseme.cli.commands.inspection_commands import _check_db_encryption
        _setup(monkeypatch, tmp_path)
        monkeypatch.delenv("SYMERASEME_ENCRYPT_DB", raising=False)
        ok, msg = _check_db_encryption()
        assert ok is True

    def test_check_db_encryption_enabled_no_db(self, monkeypatch, tmp_path):
        from symeraseme.cli.commands.inspection_commands import _check_db_encryption
        _setup(monkeypatch, tmp_path)
        monkeypatch.setenv("SYMERASEME_ENCRYPT_DB", "1")
        ok, msg = _check_db_encryption()
        assert ok is True

    def test_check_db_encryption_plaintext_db(self, monkeypatch, tmp_path):
        from symeraseme.cli.commands.inspection_commands import _check_db_encryption
        import symeraseme.core.db_connection as dbc

        data_dir = _setup(monkeypatch, tmp_path)
        monkeypatch.setenv("SYMERASEME_ENCRYPT_DB", "1")
        db_dir = data_dir / "db"
        db_dir.mkdir(parents=True)
        db_file = db_dir / "symeraseme.db"
        db_file.write_text("plaintext data")
        monkeypatch.setattr(dbc, "_db_path", lambda: db_file)

        ok, _ = _check_db_encryption()
        assert ok is False

    def test_check_db_encryption_mismatch(self, monkeypatch, tmp_path):
        from symeraseme.cli.commands.inspection_commands import _check_db_encryption
        import symeraseme.core.db_connection as dbc

        data_dir = _setup(monkeypatch, tmp_path)
        monkeypatch.delenv("SYMERASEME_ENCRYPT_DB", raising=False)
        db_dir = data_dir / "db"
        db_dir.mkdir(parents=True)
        db_file = db_dir / "symeraseme.db"
        db_file.write_bytes(b"SYMERASEME_ENCv1\nfake")
        monkeypatch.setattr(dbc, "_db_path", lambda: db_file)

        ok, _ = _check_db_encryption()
        assert ok is False

    def test_check_db_encryption_encrypted_matched(self, monkeypatch, tmp_path):
        from symeraseme.cli.commands.inspection_commands import _check_db_encryption
        import symeraseme.core.db_connection as dbc

        data_dir = _setup(monkeypatch, tmp_path)
        monkeypatch.setenv("SYMERASEME_ENCRYPT_DB", "1")
        db_dir = data_dir / "db"
        db_dir.mkdir(parents=True)
        db_file = db_dir / "symeraseme.db"
        db_file.write_bytes(b"SYMERASEME_ENCv1\nfake")
        monkeypatch.setattr(dbc, "_db_path", lambda: db_file)

        ok, _ = _check_db_encryption()
        assert ok is True


# ═══════════════════════════════════════════════════════════════════════════
# Export helpers
# ═══════════════════════════════════════════════════════════════════════════


class TestExportFunctions:
    def test_format_export_json_empty(self):
        from symeraseme.cli.commands.maintenance_commands import _format_export_json
        parsed = json.loads(_format_export_json([], None))
        assert parsed["totals"]["requests"] == 0

    def test_format_export_csv_empty(self):
        from symeraseme.cli.commands.maintenance_commands import _format_export_csv
        assert "request_id" in _format_export_csv([], [])

    def test_format_export_csv_with_events(self):
        from symeraseme.cli.commands.maintenance_commands import _format_export_csv
        result = _format_export_csv(
            [{"id": 1, "broker_id": "test-broker", "campaign_id": "test", "jurisdiction": "DE"}],
            [{"request_id": 1, "id": 100, "occurred_at": "2026-01-01", "event_type": "SENT", "source": "cli", "payload": {"key": "val"}}],
        )
        assert "test-broker" in result

    def test_write_export_file(self, tmp_path):
        from symeraseme.cli.commands.maintenance_commands import _write_export_file
        p = tmp_path / "export.json"
        _write_export_file(str(p), '{"key": "val"}')
        assert p.read_text() == '{"key": "val"}'


# ═══════════════════════════════════════════════════════════════════════════
# Serve command & app helpers
# ═══════════════════════════════════════════════════════════════════════════


class TestServe:
    def test_rejects_non_loopback(self):
        from symeraseme.cli import app as typer_app
        result = runner.invoke(typer_app, ["serve", "--host", "0.0.0.0"])
        assert result.exit_code == 1
        assert "Refusing to bind" in _strip_ansi(result.stderr)

    def test_allows_loopback(self):
        from symeraseme.cli import app as typer_app
        result = runner.invoke(typer_app, ["serve", "--host", "127.0.0.1", "--help"])
        assert result.exit_code == 0

    def test_warns_with_allow_remote(self):
        from symeraseme.cli import app as typer_app
        result = runner.invoke(typer_app, ["serve", "--host", "0.0.0.0", "--allow-remote", "--help"])
        assert result.exit_code == 0

    def test_is_loopback_host(self):
        from symeraseme.cli.app import _is_loopback_host
        assert _is_loopback_host("127.0.0.1") is True
        assert _is_loopback_host("::1") is True
        assert _is_loopback_host("localhost") is True
        assert _is_loopback_host("0.0.0.0") is False
        assert _is_loopback_host("192.168.1.1") is False
        assert _is_loopback_host("invalid!") is False


# ═══════════════════════════════════════════════════════════════════════════
# Exception guard (_run_app)
# ═══════════════════════════════════════════════════════════════════════════


class TestExceptionGuard:
    def test_logs_crash(self, monkeypatch, tmp_path):
        import sys
        monkeypatch.setenv("SYMERASEME_DATA_DIR", str(tmp_path))

        def _boom(**kwargs):
            raise ValueError("simulated crash")
        monkeypatch.setattr(sys.modules["symeraseme.cli.app"], "app", _boom)

        from symeraseme.cli.app import _run_app
        with pytest.raises(typer.Exit):
            _run_app()

        log_dir = tmp_path / "logs"
        assert log_dir.exists() and len(list(log_dir.glob("crash-*.log"))) >= 1

    def test_keyboard_interrupt(self, monkeypatch):
        import sys

        def _interrupt(**kwargs):
            raise KeyboardInterrupt()
        monkeypatch.setattr(sys.modules["symeraseme.cli.app"], "app", _interrupt)

        from symeraseme.cli.app import _run_app
        with pytest.raises(KeyboardInterrupt):
            _run_app()

    def test_typer_exit_re_raised(self, monkeypatch):
        import sys

        def _exit(**kwargs):
            raise typer.Exit(0)
        monkeypatch.setattr(sys.modules["symeraseme.cli.app"], "app", _exit)

        from symeraseme.cli.app import _run_app
        with pytest.raises(typer.Exit):
            _run_app()

    def test_pretty_exceptions_disabled(self):
        from symeraseme.cli.app import app as ta
        assert ta.pretty_exceptions_enable is False


# ═══════════════════════════════════════════════════════════════════════════
# Events / Requests commands
# ═══════════════════════════════════════════════════════════════════════════


class TestEventsCommands:
    def test_requests_list_empty(self, monkeypatch, tmp_path):
        _setup(monkeypatch, tmp_path)
        from symeraseme.cli import app as typer_app
        result = runner.invoke(typer_app, ["requests", "list"])
        assert "Traceback" not in result.output

    def test_events_show_no_request(self, monkeypatch, tmp_path):
        _setup(monkeypatch, tmp_path)
        from symeraseme.cli import app as typer_app
        result = runner.invoke(typer_app, ["events", "show", "9999"])
        assert "Traceback" not in result.output


# ═══════════════════════════════════════════════════════════════════════════
# Brokers commands
# ═══════════════════════════════════════════════════════════════════════════


class TestBrokersCommands:
    def test_brokers_list(self):
        from symeraseme.cli import app as typer_app
        result = runner.invoke(typer_app, ["brokers", "list"])
        assert result.exit_code == 0
        assert "broker" in _strip_ansi(result.stdout).lower()

    def test_brokers_list_json(self):
        from symeraseme.cli import app as typer_app
        result = runner.invoke(typer_app, ["--output", "json", "brokers", "list"])
        assert result.exit_code == 0
        parsed = json.loads(result.stdout)
        assert "success" in parsed


# ═══════════════════════════════════════════════════════════════════════════
# Web-form runner (issue #638)
# ═══════════════════════════════════════════════════════════════════════════


class TestWebFormRunner:
    """Regression coverage for #638: nested asyncio.run() crashed plan execute."""

    def test_runs_without_running_event_loop(self):
        from symeraseme.cli.commands.plan_commands import _web_form_runner

        with patch(
            "symeraseme.services.web_form.run_web_form_for_broker",
            new_callable=AsyncMock,
            return_value={"success": True, "dry_run": True, "broker_id": "test-broker"},
        ) as mock_run:
            result = _web_form_runner("test-broker", dry_run=True)
        assert result == {"success": True, "dry_run": True, "broker_id": "test-broker"}
        mock_run.assert_awaited_once_with(
            "test-broker", headed=False, screenshot_dir="", dry_run=True
        )

    @pytest.mark.asyncio
    async def test_runs_inside_running_event_loop(self):
        """Must not raise RuntimeError from a nested asyncio.run() call."""
        from symeraseme.cli.commands.plan_commands import _web_form_runner

        with patch(
            "symeraseme.services.web_form.run_web_form_for_broker",
            new_callable=AsyncMock,
            return_value={"success": True, "dry_run": True, "broker_id": "test-broker"},
        ) as mock_run:
            result = _web_form_runner("test-broker", dry_run=True)
        assert result == {"success": True, "dry_run": True, "broker_id": "test-broker"}
        mock_run.assert_awaited_once_with(
            "test-broker", headed=False, screenshot_dir="", dry_run=True
        )


# ═══════════════════════════════════════════════════════════════════════════
# Campaign-id validation (issue #642)
# ═══════════════════════════════════════════════════════════════════════════


class TestPlanCreateCampaignIdValidation:
    """Regression coverage for #642: blank campaign ids must be rejected."""

    @pytest.mark.parametrize("campaign_id", ["", "   ", "\t\n"])
    def test_blank_campaign_id_rejected_with_clear_error(
        self, monkeypatch, tmp_path, campaign_id
    ):
        _setup(monkeypatch, tmp_path)
        from symeraseme.cli import app as typer_app

        result = runner.invoke(typer_app, ["plan", "create", "--campaign", campaign_id])
        assert result.exit_code == EXIT_ERROR
        assert "must not be blank" in _strip_ansi(result.output).lower()

    def test_blank_campaign_id_never_reaches_plan_handler(self, monkeypatch, tmp_path):
        _setup(monkeypatch, tmp_path)
        from symeraseme.cli import app as typer_app
        from symeraseme.cli.commands import plan_commands

        with patch.object(plan_commands, "handle_plan_create") as mock_create:
            result = runner.invoke(typer_app, ["plan", "create", "--campaign", "   "])
        assert result.exit_code == EXIT_ERROR
        mock_create.assert_not_called()


# Account & Profile commands (identity decrypt failure handling)
# ═══════════════════════════════════════════════════════════════════════════


class TestAccountCommands:
    """Broken identity profiles must render friendly errors, not tracebacks."""

    def test_show_profile_decrypt_failure_is_friendly(self, monkeypatch):
        from symeraseme.cli import app as typer_app

        monkeypatch.setattr(
            "symeraseme.cli.commands.account_commands.profile_exists",
            lambda: True,
        )
        monkeypatch.setattr(
            "symeraseme.cli.commands.account_commands.load_profile",
            lambda: (_ for _ in ()).throw(
                RuntimeError(
                    "Your identity profile could not be decrypted. "
                    "Re-initialize with `symeraseme init-profile`."
                )
            ),
        )
        result = runner.invoke(typer_app, ["show-profile"])
        assert result.exit_code == 1
        assert "could not be decrypted" in _strip_ansi(result.stderr)
        assert "Traceback" not in result.output

    def test_render_template_decrypt_failure_is_friendly(self, monkeypatch):
        from symeraseme.cli import app as typer_app

        monkeypatch.setattr(
            "symeraseme.cli.commands.account_commands.profile_exists",
            lambda: True,
        )
        monkeypatch.setattr(
            "symeraseme.cli.commands.account_commands.load_profile",
            lambda: (_ for _ in ()).throw(
                RuntimeError(
                    "Your identity profile could not be decrypted. "
                    "Re-initialize with `symeraseme init-profile`."
                )
            ),
        )
        result = runner.invoke(typer_app, ["render-template", "gdpr-art17.de.md.j2"])
        assert result.exit_code == 1
        assert "could not be decrypted" in _strip_ansi(result.stderr)
        assert "Traceback" not in result.output

    def test_init_profile_invalid_email_reprompts(self):
        """An invalid email must show a short validation message and re-prompt."""
        from symeraseme.cli import app as typer_app

        result = runner.invoke(
            typer_app,
            ["init-profile"],
            input="Jane Doe\nnot-an-email\njane@example.com\n",
        )
        assert result.exit_code == 0
        assert "not a valid email" in _strip_ansi(result.stderr)
        assert "Traceback" not in result.output
        assert "encrypted identity profile" in _strip_ansi(result.stdout)

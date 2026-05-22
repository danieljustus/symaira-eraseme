"""Tests for the Playwright web form runner."""

from __future__ import annotations

from pathlib import Path
from unittest.mock import AsyncMock, patch

import pytest

from openeraseme.adapters.web.playwright_runner import (
    PlaywrightRunnerError,
    _capture_error,
    _resolve_value,
    run_web_form,
)


class TestResolveValue:
    def test_no_placeholders(self):
        assert _resolve_value("hello", {"name": "world"}) == "hello"

    def test_replaces_single_placeholder(self):
        result = _resolve_value("Hello ${name}", {"name": "World"})
        assert result == "Hello World"

    def test_replaces_multiple_placeholders(self):
        result = _resolve_value(
            "${full_name} <${email}>",
            {"full_name": "John Doe", "email": "john@example.com"},
        )
        assert result == "John Doe <john@example.com>"

    def test_unknown_placeholder_raises(self):
        with pytest.raises(
            PlaywrightRunnerError,
            match="Unresolved identity placeholder.*\\${unknown}",
        ):
            _resolve_value("Hello ${unknown}", {"name": "World"})

    def test_multiple_unresolved_placeholders_raises(self):
        with pytest.raises(
            PlaywrightRunnerError,
            match="Unresolved identity placeholder.*\\${missing}",
        ):
            _resolve_value("${missing} and ${missing}", {"name": "World"})

    def test_empty_identity_fields_raises(self):
        with pytest.raises(
            PlaywrightRunnerError,
            match="Unresolved identity placeholder.*\\${name}",
        ):
            _resolve_value("Hello ${name}", {})

    def test_empty_value(self):
        assert _resolve_value("", {"name": "World"}) == ""


class TestCaptureError:
    def test_timeout_error(self):
        exc = TimeoutError("Navigation timed out after 30s")
        msg = _capture_error(exc, "https://example.com")
        assert "timed out" in msg
        assert "example.com" in msg

    def test_network_error(self):
        exc = Exception("net::ERR_CONNECTION_REFUSED")
        msg = _capture_error(exc, "https://example.com")
        assert "Network" in msg
        assert "ERR_CONNECTION_REFUSED" in msg

    def test_generic_error(self):
        exc = ValueError("Invalid form state")
        msg = _capture_error(exc, "https://example.com/form")
        assert "Invalid form state" in msg

    def test_error_without_message(self):
        class CustomError(Exception):
            pass

        exc = CustomError()
        msg = _capture_error(exc, "https://example.com")
        assert "Error" in msg


class TestRunWebFormValidation:
    def test_missing_playwright_raises(self):
        with (
            patch.dict("sys.modules", {"playwright.async_api": None}),
            pytest.raises(PlaywrightRunnerError, match="Playwright is not installed"),
        ):
            import asyncio

            asyncio.run(
                run_web_form(
                    url="https://example.com",
                    steps=[{"fill": {"#name": "Test"}}],
                )
            )


class TestFormStepExecution:
    """Test execution of individual form steps."""

    @pytest.mark.asyncio
    async def test_goto_step(self):
        mock_page = AsyncMock()
        step = {"goto": "https://example.com/form"}
        from openeraseme.adapters.web.playwright_runner import _execute_step

        await _execute_step(
            mock_page,
            step,
            None,
            timeout=30.0,
            identity_fields={},
            screenshot_dir=None,
            step_index=0,
        )
        mock_page.goto.assert_called_once_with(
            "https://example.com/form",
            timeout=30000,
            wait_until="domcontentloaded",
        )

    @pytest.mark.asyncio
    async def test_fill_step(self):
        mock_page = AsyncMock()
        step = {"fill": {"#name": "John Doe", "#email": "john@test.com"}}
        from openeraseme.adapters.web.playwright_runner import _execute_step

        await _execute_step(
            mock_page,
            step,
            None,
            timeout=30.0,
            identity_fields={},
            screenshot_dir=None,
            step_index=0,
        )
        assert mock_page.fill.call_count == 2
        mock_page.fill.assert_any_call("#name", "John Doe", timeout=30000)
        mock_page.fill.assert_any_call("#email", "john@test.com", timeout=30000)

    @pytest.mark.asyncio
    async def test_fill_with_identity_fields(self):
        mock_page = AsyncMock()
        step = {"fill": {"#name": "${full_name}", "#email": "${email}"}}
        identity = {"full_name": "Jane Doe", "email": "jane@test.com"}
        from openeraseme.adapters.web.playwright_runner import _execute_step

        await _execute_step(
            mock_page,
            step,
            None,
            timeout=30.0,
            identity_fields=identity,
            screenshot_dir=None,
            step_index=0,
        )
        mock_page.fill.assert_any_call("#name", "Jane Doe", timeout=30000)
        mock_page.fill.assert_any_call("#email", "jane@test.com", timeout=30000)

    @pytest.mark.asyncio
    async def test_click_step(self):
        mock_page = AsyncMock()
        step = {"click": "#submit-btn"}
        from openeraseme.adapters.web.playwright_runner import _execute_step

        await _execute_step(
            mock_page,
            step,
            None,
            timeout=30.0,
            identity_fields={},
            screenshot_dir=None,
            step_index=0,
        )
        mock_page.click.assert_called_once_with("#submit-btn", timeout=30000)

    @pytest.mark.asyncio
    async def test_select_step(self):
        mock_page = AsyncMock()
        step = {"select": {"#reason": "gdpr"}}
        from openeraseme.adapters.web.playwright_runner import _execute_step

        await _execute_step(
            mock_page,
            step,
            None,
            timeout=30.0,
            identity_fields={},
            screenshot_dir=None,
            step_index=0,
        )
        mock_page.select_option.assert_called_once_with("#reason", "gdpr", timeout=30000)

    @pytest.mark.asyncio
    async def test_wait_for_step(self):
        mock_page = AsyncMock()
        step = {"wait_for": "#success-message"}
        from openeraseme.adapters.web.playwright_runner import _execute_step

        await _execute_step(
            mock_page,
            step,
            None,
            timeout=30.0,
            identity_fields={},
            screenshot_dir=None,
            step_index=0,
        )
        mock_page.wait_for_selector.assert_called_once_with("#success-message", timeout=30000)

    @pytest.mark.asyncio
    async def test_assert_text_passes(self):
        mock_page = AsyncMock()
        mock_page.text_content.return_value = "Your request has been submitted"
        step = {"assert_text": "submitted"}
        from openeraseme.adapters.web.playwright_runner import _execute_step

        await _execute_step(
            mock_page,
            step,
            None,
            timeout=30.0,
            identity_fields={},
            screenshot_dir=None,
            step_index=0,
        )
        mock_page.text_content.assert_called_once_with("body")

    @pytest.mark.asyncio
    async def test_assert_text_fails(self):
        mock_page = AsyncMock()
        mock_page.text_content.return_value = "Something else"
        step = {"assert_text": "success"}
        from openeraseme.adapters.web.playwright_runner import _execute_step

        with pytest.raises(PlaywrightRunnerError, match="Assertion failed"):
            await _execute_step(
                mock_page,
                step,
                None,
                timeout=30.0,
                identity_fields={},
                screenshot_dir=None,
                step_index=0,
            )

    @pytest.mark.asyncio
    async def test_assert_text_none_body_fails(self):
        mock_page = AsyncMock()
        mock_page.text_content.return_value = None
        step = {"assert_text": "anything"}
        from openeraseme.adapters.web.playwright_runner import _execute_step

        with pytest.raises(PlaywrightRunnerError, match="Assertion failed"):
            await _execute_step(
                mock_page,
                step,
                None,
                timeout=30.0,
                identity_fields={},
                screenshot_dir=None,
                step_index=0,
            )

    @pytest.mark.asyncio
    async def test_screenshot_step(self, tmp_path):
        mock_page = AsyncMock()
        mock_page.screenshot.return_value = b"image_data"
        step = {"screenshot": "after-submit"}
        from openeraseme.adapters.web.playwright_runner import _execute_step

        await _execute_step(
            mock_page,
            step,
            None,
            timeout=30.0,
            identity_fields={},
            screenshot_dir=tmp_path,
            step_index=0,
        )
        mock_page.screenshot.assert_called_once()

    @pytest.mark.asyncio
    async def test_complete_form_flow(self, tmp_path):
        mock_page = AsyncMock()
        mock_page.text_content.return_value = "Your request has been submitted successfully."

        steps = [
            {"goto": "https://example.com/form"},
            {"fill": {"#name": "John Doe", "#email": "john@test.com"}},
            {"select": {"#reason": "gdpr"}},
            {"click": "#submit-btn"},
            {"wait_for": "#success-message"},
            {"assert_text": "submitted successfully"},
            {"screenshot": "success"},
        ]

        from openeraseme.adapters.web.playwright_runner import _execute_step

        for i, step in enumerate(steps):
            await _execute_step(
                mock_page,
                step,
                "https://example.com/form" if i == 0 else None,
                timeout=30.0,
                identity_fields={},
                screenshot_dir=tmp_path,
                step_index=i,
            )

        mock_page.goto.assert_called_once()
        assert mock_page.fill.call_count == 2
        mock_page.click.assert_called_once()
        mock_page.wait_for_selector.assert_called_once()
        mock_page.text_content.assert_called_once()
        mock_page.screenshot.assert_called_once()


class TestFormSchema:
    def test_form_spec_roundtrip(self):
        from openeraseme.registry.schema import FormSpec, FormStep

        spec = FormSpec(
            steps=[
                FormStep(goto="https://example.com/form"),
                FormStep(fill={"#name": "${full_name}", "#email": "${email}"}),
                FormStep(select={"#reason": "gdpr"}),
                FormStep(click="#submit-btn"),
                FormStep(wait_for="#success-message"),
                FormStep(assert_text="success"),
                FormStep(screenshot="result"),
            ],
            timeout_seconds=45.0,
            rate_limit_delay=2.0,
        )

        data = spec.model_dump()
        assert len(data["steps"]) == 7
        assert data["timeout_seconds"] == 45.0
        assert data["rate_limit_delay"] == 2.0
        assert data["steps"][0]["goto"] == "https://example.com/form"
        assert data["steps"][1]["fill"]["#name"] == "${full_name}"
        assert data["steps"][3]["click"] == "#submit-btn"
        assert data["steps"][4]["wait_for"] == "#success-message"
        assert data["steps"][5]["assert_text"] == "success"
        assert data["steps"][6]["screenshot"] == "result"

    def test_web_form_opt_out_roundtrip(self):
        from openeraseme.registry.schema import FormSpec, FormStep, WebFormOptOut

        form = WebFormOptOut(
            url="https://broker.example.com/optout",
            form_spec=FormSpec(
                steps=[
                    FormStep(fill={"#name": "Test"}),
                    FormStep(click="#submit"),
                ]
            ),
        )
        data = form.model_dump()
        assert data["type"] == "web_form"
        assert data["url"] == "https://broker.example.com/optout"
        assert len(data["form_spec"]["steps"]) == 2


class TestLocalFixtureForm:
    def test_fixture_html_exists(self):
        path = Path(__file__).parent.parent / "fixtures" / "web_forms" / "simple_form.html"
        assert path.exists()
        content = path.read_text()
        assert "optout-form" in content
        assert "submit-btn" in content
        assert "success-message" in content

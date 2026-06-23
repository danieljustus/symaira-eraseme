"""Tests for the web form service handlers in ``services/web_form.py``."""

from __future__ import annotations

from unittest.mock import AsyncMock, patch

import pytest

from symeraseme.adapters.web.playwright_runner import (
    PlaywrightRunnerError,
    WebFormResult,
)
from symeraseme.core.exceptions import RegistryError
from symeraseme.registry.schema import Address, FormSpec, FormStep, IdentityProfile

WF = "symeraseme.services.web_form"


class TestBuildIdentityFields:
    """Coverage for ``_build_identity_fields`` — all profile-existence branches."""

    def test_no_profile_returns_empty_dict(self):
        with patch(f"{WF}.profile_exists", return_value=False):
            from symeraseme.services.web_form import _build_identity_fields

            result = _build_identity_fields()
            assert result == {}

    def test_with_full_profile_returns_all_fields(self):
        profile = IdentityProfile(
            full_name="John Michael Doe",
            email_addresses=["john@example.com"],
            phone_numbers=["+1-555-0100"],
            addresses=[
                Address(
                    street="123 Main St",
                    city="Springfield",
                    postal_code="12345",
                    state="IL",
                    country="US",
                ),
            ],
        )
        with (
            patch(f"{WF}.profile_exists", return_value=True),
            patch(f"{WF}.load_profile", return_value=profile),
        ):
            from symeraseme.services.web_form import _build_identity_fields

            result = _build_identity_fields()
            assert result["full_name"] == "John Michael Doe"
            assert result["first_name"] == "John"
            assert result["last_name"] == "Michael Doe"
            assert result["email"] == "john@example.com"
            assert result["phone_number"] == "+1-555-0100"
            assert result["address_street_0"] == "123 Main St"
            assert result["address_city_0"] == "Springfield"
            assert result["address_zip_0"] == "12345"
            assert result["address_state_0"] == "IL"
            assert result["address_country_0"] == "US"

    def test_single_name_profile(self):
        profile = IdentityProfile(full_name="Madonna")
        with (
            patch(f"{WF}.profile_exists", return_value=True),
            patch(f"{WF}.load_profile", return_value=profile),
        ):
            from symeraseme.services.web_form import _build_identity_fields

            result = _build_identity_fields()
            assert result["full_name"] == "Madonna"
            assert result["first_name"] == "Madonna"
            assert result["last_name"] == ""

    def test_missing_email_and_phone_default_to_empty(self):
        profile = IdentityProfile(
            full_name="Jane Doe",
            email_addresses=[],
            phone_numbers=[],
        )
        with (
            patch(f"{WF}.profile_exists", return_value=True),
            patch(f"{WF}.load_profile", return_value=profile),
        ):
            from symeraseme.services.web_form import _build_identity_fields

            result = _build_identity_fields()
            assert result["email"] == ""
            assert result["phone_number"] == ""


class TestWarnMissingStateForCCPA:
    """Coverage for ``_warn_missing_state_for_ccpa``."""

    def test_non_ccpa_jurisdiction_skips_warning(self, caplog):
        caplog.set_level("WARNING")
        from symeraseme.services.web_form import _warn_missing_state_for_ccpa

        _warn_missing_state_for_ccpa(
            "test", "Test Broker", ["GDPR"], {"address_state_0": ""}
        )
        assert len(caplog.records) == 0

    def test_ccpa_with_empty_state_logs_warning(self, caplog):
        caplog.set_level("WARNING")
        from symeraseme.services.web_form import _warn_missing_state_for_ccpa

        _warn_missing_state_for_ccpa(
            "test", "Test Broker", ["CCPA"], {"address_state_0": ""}
        )
        assert len(caplog.records) == 1
        assert "requires CCPA form" in caplog.records[0].message
        assert "Test Broker" in caplog.records[0].message

    def test_ccpa_with_populated_state_no_warning(self, caplog):
        caplog.set_level("WARNING")
        from symeraseme.services.web_form import _warn_missing_state_for_ccpa

        _warn_missing_state_for_ccpa(
            "test", "Test Broker", ["CCPA"], {"address_state_0": "CA"}
        )
        assert len(caplog.records) == 0

    def test_ccpa_first_address_empty_second_populated_warns(self, caplog):
        caplog.set_level("WARNING")
        from symeraseme.services.web_form import _warn_missing_state_for_ccpa

        _warn_missing_state_for_ccpa(
            "test",
            "Test Broker",
            ["CCPA"],
            {"address_state_0": "", "address_state_1": "TX"},
        )
        # Warns on the first empty state key and returns early
        assert len(caplog.records) == 1

    def test_ccpa_no_address_state_keys_no_warning(self, caplog):
        caplog.set_level("WARNING")
        from symeraseme.services.web_form import _warn_missing_state_for_ccpa

        _warn_missing_state_for_ccpa(
            "test", "Test Broker", ["CCPA"], {"full_name": "Jane Doe"}
        )
        assert len(caplog.records) == 0


class TestRunWebFormForBroker:
    """Coverage for ``run_web_form_for_broker`` — dry-run, no-form, success, failure."""

    @pytest.mark.asyncio
    async def test_no_web_forms_returns_error(self):
        broker = _stub_broker(opt_out=[])
        with patch(f"{WF}.load_broker", return_value=broker):
            from symeraseme.services.web_form import run_web_form_for_broker

            result = await run_web_form_for_broker("test-broker")
            assert result["success"] is False
            assert "no web form" in result["error"].lower()

    @pytest.mark.asyncio
    async def test_dry_run_returns_structured_data(self):
        form = _stub_web_form(
            url="https://example.com/optout",
            steps=[FormStep(fill={"#name": "${full_name}"}), FormStep(click="#submit")],
        )
        broker = _stub_broker(opt_out=[form], jurisdictions=["GDPR"])
        with (
            patch(f"{WF}.load_broker", return_value=broker),
            patch(f"{WF}.profile_exists", return_value=False),
        ):
            from symeraseme.services.web_form import run_web_form_for_broker

            result = await run_web_form_for_broker("test-broker", dry_run=True)
            assert result["success"] is True
            assert result["dry_run"] is True
            assert result["broker_id"] == "test-broker"
            assert result["broker_name"] == "Test Broker"
            assert result["url"] == "https://example.com/optout"
            assert len(result["steps"]) == 2
            assert result["identity_fields"] == {}
            # body is a JSON string
            assert '"url": "https://example.com/optout"' in result["body"]

    @pytest.mark.asyncio
    async def test_successful_run(self):
        form = _stub_web_form(
            url="https://example.com/optout",
            steps=[FormStep(goto="https://example.com/optout")],
        )
        broker = _stub_broker(opt_out=[form], jurisdictions=["GDPR"])
        mock_result = WebFormResult(
            success=True,
            step_index=1,
            total_steps=1,
            error="",
            screenshot_path="",
        )
        with (
            patch(f"{WF}.load_broker", return_value=broker),
            patch(f"{WF}.profile_exists", return_value=True),
            patch(f"{WF}.load_profile") as mock_load_profile,
            patch(f"{WF}._run_form", new_callable=AsyncMock, return_value=mock_result),
        ):
            mock_load_profile.return_value = IdentityProfile(
                full_name="Jane Doe",
                email_addresses=["jane@example.com"],
                phone_numbers=[],
            )
            from symeraseme.services.web_form import run_web_form_for_broker

            result = await run_web_form_for_broker("test-broker")
            assert result["success"] is True
            assert result["broker_id"] == "test-broker"
            assert result["step_index"] == 1
            assert result["total_steps"] == 1
            assert result["error"] == ""
            assert result["task_id"] is None

    @pytest.mark.asyncio
    async def test_failure_creates_manual_task(self):
        form = _stub_web_form(
            url="https://example.com/optout",
            steps=[FormStep(goto="https://example.com/optout")],
        )
        broker = _stub_broker(opt_out=[form], jurisdictions=["GDPR"])
        mock_result = WebFormResult(
            success=False,
            step_index=0,
            total_steps=3,
            error="Timeout waiting for selector",
            screenshot_path="/tmp/screen.png",
        )
        mock_task = type("ManualTask", (), {"id": 42})()
        with (
            patch(f"{WF}.load_broker", return_value=broker),
            patch(f"{WF}.profile_exists", return_value=False),
            patch(f"{WF}._run_form", new_callable=AsyncMock, return_value=mock_result),
            patch(f"{WF}.create_manual_task", return_value=mock_task),
        ):
            from symeraseme.services.web_form import run_web_form_for_broker

            result = await run_web_form_for_broker("test-broker")
            assert result["success"] is False
            assert result["error"] == "Timeout waiting for selector"
            assert result["broker_id"] == "test-broker"
            assert result["task_id"] == 42


class TestHandleRunWebForm:
    """Coverage for ``handle_run_web_form`` — error paths, dry-run, success."""

    @pytest.mark.asyncio
    async def test_broker_not_found_registry_error(self):
        with patch(f"{WF}.load_broker", side_effect=RegistryError("missing")):
            from symeraseme.services.web_form import handle_run_web_form

            result = await handle_run_web_form("unknown")
            assert result.success is False
            assert "not found" in (result.error or "").lower()
            assert "unknown" in (result.error or "")

    @pytest.mark.asyncio
    async def test_broker_not_found_file_not_found(self):
        with patch(
            f"{WF}.load_broker", side_effect=FileNotFoundError("no such broker")
        ):
            from symeraseme.services.web_form import handle_run_web_form

            result = await handle_run_web_form("unknown")
            assert result.success is False
            assert "not found" in (result.error or "").lower()

    @pytest.mark.asyncio
    async def test_no_web_form_channel(self):
        broker = _stub_broker(opt_out=[])
        with patch(f"{WF}.load_broker", return_value=broker):
            from symeraseme.services.web_form import handle_run_web_form

            result = await handle_run_web_form("test-broker")
            assert result.success is False
            assert "no web form" in (result.error or "").lower()

    @pytest.mark.asyncio
    async def test_playwright_error(self):
        form = _stub_web_form(
            url="https://example.com/optout",
            steps=[FormStep(goto="https://example.com/optout")],
        )
        broker = _stub_broker(opt_out=[form], jurisdictions=["GDPR"])
        with (
            patch(f"{WF}.load_broker", return_value=broker),
            patch(f"{WF}.profile_exists", return_value=False),
            patch(
                f"{WF}._run_form",
                new_callable=AsyncMock,
                side_effect=PlaywrightRunnerError("Browser crashed"),
            ),
        ):
            from symeraseme.services.web_form import handle_run_web_form

            result = await handle_run_web_form("test-broker")
            assert result.success is False
            assert result.error is not None
            assert "playwright error" in result.error.lower()
            assert "Browser crashed" in result.error

    @pytest.mark.asyncio
    async def test_dry_run(self):
        form = _stub_web_form(
            url="https://example.com/optout",
            steps=[FormStep(fill={"#name": "${full_name}"})],
        )
        broker = _stub_broker(opt_out=[form], jurisdictions=["GDPR"])
        with (
            patch(f"{WF}.load_broker", return_value=broker),
            patch(f"{WF}.profile_exists", return_value=True),
            patch(f"{WF}.load_profile") as mock_load_profile,
        ):
            mock_load_profile.return_value = IdentityProfile(full_name="Jane Doe")
            from symeraseme.services.web_form import handle_run_web_form

            result = await handle_run_web_form("test-broker", dry_run=True)
            assert result.success is True
            assert result.data["dry_run"] is True
            assert "DRY RUN" in result.data["message"]

    @pytest.mark.asyncio
    async def test_successful_run(self):
        form = _stub_web_form(
            url="https://example.com/optout",
            steps=[FormStep(goto="https://example.com/optout")],
        )
        broker = _stub_broker(opt_out=[form], jurisdictions=["GDPR"])
        mock_result = WebFormResult(
            success=True,
            step_index=1,
            total_steps=1,
            error="",
            screenshot_path="",
        )
        with (
            patch(f"{WF}.load_broker", return_value=broker),
            patch(f"{WF}.profile_exists", return_value=False),
            patch(f"{WF}._run_form", new_callable=AsyncMock, return_value=mock_result),
        ):
            from symeraseme.services.web_form import handle_run_web_form

            result = await handle_run_web_form("test-broker")
            assert result.success is True
            assert result.data["success"] is True
            assert "completed" in result.data.get("message", "")

    @pytest.mark.asyncio
    async def test_failure_returns_error_message(self):
        form = _stub_web_form(
            url="https://example.com/optout",
            steps=[FormStep(goto="https://example.com/optout")],
        )
        broker = _stub_broker(opt_out=[form], jurisdictions=["GDPR"])
        mock_result = WebFormResult(
            success=False,
            step_index=1,
            total_steps=3,
            error="Field validation failed",
            screenshot_path="",
        )
        mock_task = type("ManualTask", (), {"id": 99})()
        with (
            patch(f"{WF}.load_broker", return_value=broker),
            patch(f"{WF}.profile_exists", return_value=False),
            patch(f"{WF}._run_form", new_callable=AsyncMock, return_value=mock_result),
            patch(f"{WF}.create_manual_task", return_value=mock_task),
        ):
            from symeraseme.services.web_form import handle_run_web_form

            result = await handle_run_web_form("test-broker")
            assert result.success is False
            assert result.data["success"] is False
            assert result.data["task_id"] == 99
            assert "Field validation failed" in (result.error or "")


# ---------------------------------------------------------------------------
# Helpers
# ---------------------------------------------------------------------------


def _stub_broker(
    *,
    opt_out: list | None = None,
    jurisdictions: list[str] | None = None,
) -> object:
    """Return a minimal broker-like object suitable for patching ``load_broker``."""
    return type(
        "Broker",
        (),
        {
            "id": "test-broker",
            "name": "Test Broker",
            "opt_out": opt_out or [],
            "jurisdictions": jurisdictions or [],
        },
    )()


def _stub_web_form(url: str, steps: list[FormStep]) -> object:
    """Return a minimal ``WebFormOptOut``-like object."""
    from symeraseme.registry.schema import WebFormOptOut

    return WebFormOptOut(
        url=url,
        form_spec=FormSpec(steps=steps),
    )

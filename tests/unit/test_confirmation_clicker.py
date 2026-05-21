from __future__ import annotations

from unittest.mock import patch

import pytest

from openeraseme.adapters.web.confirmation_clicker import (
    KNOWN_BROKER_DOMAINS,
    ConfirmationClickerError,
    ConfirmationResult,
    _capture_clicker_error,
    extract_confirmation_links,
)


class TestExtractConfirmationLinks:
    def test_empty_text(self):
        assert extract_confirmation_links("") == []

    def test_no_urls(self):
        assert extract_confirmation_links("Hello world") == []

    def test_extracts_simple_url(self):
        text = "Click here: https://acxiom.com/confirm"
        links = extract_confirmation_links(text)
        assert links == ["https://acxiom.com/confirm"]

    def test_filters_by_domain(self):
        text = "Confirm at https://evil.com/confirm and https://acxiom.com/optout"
        allowed = frozenset({"acxiom.com"})
        links = extract_confirmation_links(text, allowed_domains=allowed)
        assert len(links) == 1
        assert "acxiom.com" in links[0]

    def test_filters_unknown_domains(self):
        text = "Go to https://unknown-spam-site.com/click"
        links = extract_confirmation_links(text)
        assert links == []

    def test_known_broker_domains_work(self):
        for domain in list(KNOWN_BROKER_DOMAINS)[:3]:
            text = f"Confirm: https://{domain}/confirm?id=123"
            links = extract_confirmation_links(text)
            assert len(links) == 1
            assert domain in links[0]

    def test_deduplicates(self):
        text = "Link: https://acxiom.com/confirm Link: https://acxiom.com/confirm"
        links = extract_confirmation_links(text)
        assert len(links) == 1

    def test_sorts_by_path_length(self):
        text = (
            "https://acxiom.com/tracking?abc=123 "
            "https://acxiom.com/verbose-confirm-path "
            "https://acxiom.com/confirm"
        )
        links = extract_confirmation_links(text)
        assert links[0] == "https://acxiom.com/confirm"

    def test_handles_www_prefix(self):
        text = "https://www.acxiom.com/confirm"
        links = extract_confirmation_links(text)
        assert len(links) == 1

    def test_cleans_trailing_punctuation(self):
        text = "Visit https://acxiom.com/confirm."
        links = extract_confirmation_links(text)
        assert links[0] == "https://acxiom.com/confirm"

    def test_custom_allowed_domains(self):
        text = "https://custom-broker.com/yes"
        allowed = frozenset({"custom-broker.com"})
        links = extract_confirmation_links(text, allowed_domains=allowed)
        assert len(links) == 1


class TestCaptureClickerError:
    def test_timeout(self):
        exc = TimeoutError("Navigation timed out after 30s")
        msg = _capture_clicker_error(exc, "https://example.com")
        assert "Timeout" in msg

    def test_network(self):
        exc = Exception("net::ERR_CONNECTION_REFUSED")
        msg = _capture_clicker_error(exc, "https://example.com")
        assert "Network" in msg

    def test_generic(self):
        exc = ValueError("Something broke")
        msg = _capture_clicker_error(exc, "https://example.com")
        assert "Something broke" in msg


class TestAutoConfirmValidation:
    def test_no_links_returns_error(self):
        result = ConfirmationResult(success=False, step="no_links", error="No links")
        assert not result.success
        assert result.step == "no_links"

    @pytest.mark.asyncio
    async def test_missing_playwright_raises(self):
        from openeraseme.adapters.web.confirmation_clicker import auto_confirm

        with (
            patch.dict("sys.modules", {"playwright.async_api": None}),
            pytest.raises(ConfirmationClickerError, match="Playwright is not installed"),
        ):
            await auto_confirm(1, "https://acxiom.com/confirm")

    @pytest.mark.asyncio
    async def test_dry_run_returns_immediately(self):
        from openeraseme.adapters.web.confirmation_clicker import auto_confirm

        result = await auto_confirm(
            1,
            "Click here: https://acxiom.com/confirm",
            dry_run=True,
        )
        assert result.dry_run
        assert result.success
        assert "acxiom.com" in result.clicked_url


class TestConfirmationResult:
    def test_defaults(self):
        r = ConfirmationResult()
        assert not r.success
        assert r.clicked_url == ""
        assert r.error == ""
        assert not r.dry_run

    def test_success_state(self):
        r = ConfirmationResult(success=True, clicked_url="https://example.com/c", step="clicked")
        assert r.success
        assert r.clicked_url == "https://example.com/c"
        assert r.step == "clicked"

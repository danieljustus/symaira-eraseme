"""Tests for symfetch-aware URL fetching."""

from __future__ import annotations

import subprocess
from unittest.mock import patch

import pytest

from symeraseme.core.webfetch import (
    FetchResult,
    _fetch_via_symfetch,
    fetch_url,
    symfetch_available,
)


class TestSymfetchAvailable:
    def test_returns_true_when_installed(self):
        with patch("shutil.which", return_value="/usr/local/bin/symfetch"):
            assert symfetch_available() is True

    def test_returns_false_when_missing(self):
        with patch("shutil.which", return_value=None):
            assert symfetch_available() is False


class TestFetchViaSymvault:
    def test_success(self):
        fake_result = subprocess.CompletedProcess(
            args=["symfetch", "get", "https://example.com", "--format", "md"],
            returncode=0,
            stdout=b"# Example\n\nHello world",
            stderr=b"",
        )
        with (
            patch("symeraseme.core.webfetch.symfetch_available", return_value=True),
            patch("subprocess.run", return_value=fake_result),
        ):
            result = _fetch_via_symfetch("https://example.com")

        assert result is not None
        assert result.body == "# Example\n\nHello world"
        assert result.via == "symfetch"
        assert result.status_code == 200

    def test_not_available_returns_none(self):
        with patch("symeraseme.core.webfetch.symfetch_available", return_value=False):
            result = _fetch_via_symfetch("https://example.com")
        assert result is None

    def test_nonzero_exit_returns_none(self):
        fake_result = subprocess.CompletedProcess(
            args=["symfetch", "get", "https://example.com", "--format", "md"],
            returncode=1,
            stdout=b"",
            stderr=b"error: connection refused",
        )
        with (
            patch("symeraseme.core.webfetch.symfetch_available", return_value=True),
            patch("subprocess.run", return_value=fake_result),
        ):
            result = _fetch_via_symfetch("https://example.com")
        assert result is None

    def test_timeout_returns_none(self):
        with (
            patch("symeraseme.core.webfetch.symfetch_available", return_value=True),
            patch(
                "subprocess.run",
                side_effect=subprocess.TimeoutExpired(cmd="symfetch", timeout=60),
            ),
        ):
            result = _fetch_via_symfetch("https://slow.example.com")
        assert result is None


class TestFetchUrl:
    def test_auto_uses_symfetch_when_available(self):
        symfetch_result = FetchResult(
            url="https://example.com",
            status_code=200,
            body="via symfetch",
            via="symfetch",
        )
        with (
            patch("symeraseme.core.webfetch._fetch_via_symfetch", return_value=symfetch_result),
        ):
            result = fetch_url("https://example.com")

        assert result.via == "symfetch"
        assert result.body == "via symfetch"

    def test_falls_back_to_urllib_when_symfetch_fails(self):
        urllib_result = FetchResult(
            url="https://example.com",
            status_code=200,
            body="via urllib",
            via="urllib",
        )
        with (
            patch("symeraseme.core.webfetch._fetch_via_symfetch", return_value=None),
            patch("symeraseme.core.webfetch._fetch_via_urllib", return_value=urllib_result),
        ):
            result = fetch_url("https://example.com")

        assert result.via == "urllib"

    def test_force_symfetch_raises_when_not_installed(self):
        with (
            patch("symeraseme.core.webfetch.symfetch_available", return_value=False),
            pytest.raises(RuntimeError, match="symfetch requested but not found"),
        ):
            fetch_url("https://example.com", use_symfetch=True)

    def test_force_urllib_skips_symfetch(self):
        urllib_result = FetchResult(
            url="https://example.com",
            status_code=200,
            body="forced urllib",
            via="urllib",
        )
        with (
            patch("symeraseme.core.webfetch._fetch_via_symfetch") as mock_sf,
            patch("symeraseme.core.webfetch._fetch_via_urllib", return_value=urllib_result),
        ):
            result = fetch_url("https://example.com", use_symfetch=False)

        mock_sf.assert_not_called()
        assert result.via == "urllib"

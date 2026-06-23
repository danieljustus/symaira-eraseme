"""Tests for the captcha solving service layer."""

from __future__ import annotations

from unittest.mock import MagicMock, patch

from symeraseme.adapters.web.captcha_solver import CaptchaError
from symeraseme.core.result_types import CliResult


class TestHandleSolveCaptcha:
    """Tests for handle_solve_captcha()."""

    # -- dry_run path -------------------------------------------------------

    def test_dry_run_returns_success(self):
        from symeraseme.services.captcha import handle_solve_captcha

        result = handle_solve_captcha(dry_run=True)

        assert result.success is True
        assert result.error is None

    def test_dry_run_includes_expected_fields(self):
        from symeraseme.services.captcha import handle_solve_captcha

        result = handle_solve_captcha(
            provider="capsolver",
            site_key="6Lc123",
            page_url="https://example.com",
            dry_run=True,
        )

        assert result.data["dry_run"] is True
        assert result.data["provider"] == "capsolver"
        assert result.data["site_key"] == "6Lc123"
        assert result.data["page_url"] == "https://example.com"
        assert "[DRY RUN]" in result.data["message"]

    def test_dry_run_provides_none_fields_when_omitted(self):
        from symeraseme.services.captcha import handle_solve_captcha

        result = handle_solve_captcha(dry_run=True)

        assert result.data["site_key"] is None
        assert result.data["page_url"] is None

    # -- validation: missing site_key / page_url ----------------------------

    def test_missing_site_key_returns_error(self):
        from symeraseme.services.captcha import handle_solve_captcha

        result = handle_solve_captcha(
            site_key=None,
            page_url="https://example.com",
        )

        assert result.success is False
        assert "site_key and page_url are required" in (result.error or "")

    def test_missing_page_url_returns_error(self):
        from symeraseme.services.captcha import handle_solve_captcha

        result = handle_solve_captcha(
            site_key="6Lc123",
            page_url=None,
        )

        assert result.success is False
        assert "site_key and page_url are required" in (result.error or "")

    def test_both_missing_returns_error(self):
        from symeraseme.services.captcha import handle_solve_captcha

        result = handle_solve_captcha(
            site_key=None,
            page_url=None,
        )

        assert result.success is False
        assert "site_key and page_url are required" in (result.error or "")

    # -- successful solve ---------------------------------------------------

    @patch("symeraseme.services.captcha.typer.echo")
    @patch("symeraseme.services.captcha.create_solver")
    def test_successful_solve_returns_token_and_task_id(
        self, mock_create_solver, mock_echo
    ):
        from symeraseme.services.captcha import handle_solve_captcha

        # Arrange
        mock_solver = MagicMock()
        mock_solver.solve_recaptcha_v2.return_value = MagicMock(
            token="captcha-token-abc",
            task_id="task-456",
        )
        mock_create_solver.return_value = mock_solver

        # Act
        result = handle_solve_captcha(
            provider="capsolver",
            api_key="test-key",
            site_key="6Lc123",
            page_url="https://example.com/form",
        )

        # Assert
        assert result.success is True
        assert result.data["token"] == "captcha-token-abc"
        assert result.data["task_id"] == "task-456"
        assert result.data["provider"] == "capsolver"

    @patch("symeraseme.services.captcha.typer.echo")
    @patch("symeraseme.services.captcha.create_solver")
    def test_successful_solve_creates_solver_with_correct_args(
        self, mock_create_solver, mock_echo
    ):
        from symeraseme.services.captcha import handle_solve_captcha

        mock_solver = MagicMock()
        mock_solver.solve_recaptcha_v2.return_value = MagicMock(
            token="tok", task_id="tid"
        )
        mock_create_solver.return_value = mock_solver

        handle_solve_captcha(
            provider="capsolver",
            api_key="test-key",
            site_key="6Lc123",
            page_url="https://example.com/form",
        )

        mock_create_solver.assert_called_once_with("capsolver", api_key="test-key")
        mock_solver.solve_recaptcha_v2.assert_called_once_with(
            site_key="6Lc123",
            page_url="https://example.com/form",
        )

    @patch("symeraseme.services.captcha.typer.echo")
    @patch("symeraseme.services.captcha.create_solver")
    def test_successful_solve_echoes_provider(
        self, mock_create_solver, mock_echo
    ):
        from symeraseme.services.captcha import handle_solve_captcha

        mock_solver = MagicMock()
        mock_solver.solve_recaptcha_v2.return_value = MagicMock(
            token="tok", task_id="tid"
        )
        mock_create_solver.return_value = mock_solver

        handle_solve_captcha(
            provider="twocaptcha",
            api_key="key",
            site_key="k",
            page_url="https://example.com",
        )

        mock_echo.assert_called_once_with("Solving captcha via twocaptcha...")

    # -- CaptchaError path --------------------------------------------------

    @patch("symeraseme.services.captcha.typer.echo")
    @patch("symeraseme.services.captcha.create_solver")
    def test_captcha_error_returns_error_result(
        self, mock_create_solver, mock_echo
    ):
        from symeraseme.services.captcha import handle_solve_captcha

        mock_solver = MagicMock()
        mock_solver.solve_recaptcha_v2.side_effect = CaptchaError("Invalid API key")
        mock_create_solver.return_value = mock_solver

        result = handle_solve_captcha(
            provider="capsolver",
            api_key="bad-key",
            site_key="6Lc123",
            page_url="https://example.com",
        )

        assert result.success is False
        assert "Invalid API key" in (result.error or "")
        assert "Captcha solving failed" in (result.error or "")

    @patch("symeraseme.services.captcha.typer.echo")
    @patch("symeraseme.services.captcha.create_solver")
    def test_captcha_error_includes_guidance(
        self, mock_create_solver, mock_echo
    ):
        from symeraseme.services.captcha import handle_solve_captcha

        mock_solver = MagicMock()
        mock_solver.solve_recaptcha_v2.side_effect = CaptchaError("Timeout")
        mock_create_solver.return_value = mock_solver

        result = handle_solve_captcha(
            provider="capsolver",
            api_key="key",
            site_key="k",
            page_url="https://example.com",
        )

        assert "CAPSOLVER_API_KEY" in (result.error or "")
        assert "TWOCAPTCHA_API_KEY" in (result.error or "")

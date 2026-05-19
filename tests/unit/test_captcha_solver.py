from __future__ import annotations

from unittest.mock import patch

import pytest

from openeraseme.adapters.web.captcha_solver import (
    CapSolverSolver,
    CaptchaError,
    CaptchaSolution,
    TwoCaptchaSolver,
    create_solver,
)


class TestCreateSolver:
    def test_capsolver(self):
        solver = create_solver("capsolver", api_key="test-key")
        assert isinstance(solver, CapSolverSolver)

    def test_twocaptcha(self):
        solver = create_solver("twocaptcha", api_key="test-key")
        assert isinstance(solver, TwoCaptchaSolver)

    def test_unknown_provider(self):
        with pytest.raises(CaptchaError, match="Unknown"):
            create_solver("unknown", api_key="key")

    def test_missing_key(self):
        env_patcher = patch.dict("os.environ", {}, clear=True)
        with env_patcher, pytest.raises(CaptchaError, match="API key not configured"):
            create_solver("capsolver")

    def test_reads_env_var(self):
        with patch.dict("os.environ", {"CAPSOLVER_API_KEY": "env-key"}):
            solver = create_solver("capsolver")
            assert solver._api_key == "env-key"


class TestCapSolverSolver:
    def test_init(self):
        solver = CapSolverSolver("test-key")
        assert solver._api_key == "test-key"

    @patch("openeraseme.adapters.web.captcha_solver.urlopen")
    def test_solve_recaptcha_v2_creates_task(self, mock_urlopen):
        from unittest.mock import Mock

        # Mock createTask response
        create_resp = Mock()
        create_resp.read.return_value = b'{"errorId": 0, "taskId": "abc-123"}'
        # Mock getTaskResult response
        result_resp = Mock()
        result_resp.read.return_value = (
            b'{"errorId": 0, "status": "ready", "solution":'
            + b' {"gRecaptchaResponse": "token-xyz"}}'
        )
        mock_urlopen.side_effect = [create_resp, result_resp]

        solver = CapSolverSolver("test-key")
        result = solver.solve_recaptcha_v2(
            site_key="6Lc...",
            page_url="https://example.com/form",
        )

        assert result.token == "token-xyz"
        assert result.task_id == "abc-123"

    @patch("openeraseme.adapters.web.captcha_solver.urlopen")
    def test_capsolver_api_error(self, mock_urlopen):
        from unittest.mock import Mock

        resp = Mock()
        resp.read.return_value = b'{"errorId": 1, "errorDescription": "Invalid API key"}'
        mock_urlopen.return_value = resp

        solver = CapSolverSolver("bad-key")
        with pytest.raises(CaptchaError, match="Invalid API key"):
            solver.solve_recaptcha_v2(
                site_key="6Lc...",
                page_url="https://example.com",
            )

    @patch("openeraseme.adapters.web.captcha_solver.urlopen")
    @patch("openeraseme.adapters.web.captcha_solver.time.sleep")
    def test_capsolver_timeout(self, mock_sleep, mock_urlopen):
        from unittest.mock import Mock

        create_resp = Mock()
        create_resp.read.return_value = b'{"errorId": 0, "taskId": "abc-123"}'
        result_resp = Mock()
        result_resp.read.return_value = b'{"errorId": 0, "status": "processing"}'
        mock_urlopen.side_effect = [create_resp] + [result_resp] * 30

        solver = CapSolverSolver("test-key")
        with pytest.raises(CaptchaError, match="timed out"):
            solver.solve_recaptcha_v2(
                site_key="6Lc...",
                page_url="https://example.com",
            )


class TestTwoCaptchaSolver:
    def test_init(self):
        solver = TwoCaptchaSolver("test-key")
        assert solver._api_key == "test-key"

    @patch("openeraseme.adapters.web.captcha_solver.urlopen")
    def test_solve_recaptcha_v2(self, mock_urlopen):
        from unittest.mock import Mock

        # Mock in.php upload response
        upload_resp = Mock()
        upload_resp.read.return_value = b'{"status": 1, "request": "captcha-456"}'
        # Mock res.php result response
        result_resp = Mock()
        result_resp.read.return_value = b'{"status": 1, "request": "token-abc"}'
        mock_urlopen.side_effect = [upload_resp, result_resp]

        solver = TwoCaptchaSolver("test-key")
        result = solver.solve_recaptcha_v2(
            site_key="6Lc...",
            page_url="https://example.com/form",
        )

        assert result.token == "token-abc"
        assert result.task_id == "captcha-456"

    @patch("openeraseme.adapters.web.captcha_solver.urlopen")
    def test_twocaptcha_upload_error(self, mock_urlopen):
        from unittest.mock import Mock

        resp = Mock()
        resp.read.return_value = b'{"status": 0, "request": "ERROR_WRONG_USER_KEY"}'
        mock_urlopen.return_value = resp

        solver = TwoCaptchaSolver("bad-key")
        with pytest.raises(CaptchaError, match="ERROR_WRONG_USER_KEY"):
            solver.solve_recaptcha_v2(
                site_key="6Lc...",
                page_url="https://example.com",
            )

    @patch("openeraseme.adapters.web.captcha_solver.urlopen")
    @patch("openeraseme.adapters.web.captcha_solver.time.sleep")
    def test_twocaptcha_timeout(self, mock_sleep, mock_urlopen):
        from unittest.mock import Mock

        upload_resp = Mock()
        upload_resp.read.return_value = b'{"status": 1, "request": "captcha-456"}'
        result_resp = Mock()
        result_resp.read.return_value = b'{"status": 0, "request": "CAPCHA_NOT_READY"}'
        mock_urlopen.side_effect = [upload_resp] + [result_resp] * 30

        solver = TwoCaptchaSolver("test-key")
        with pytest.raises(CaptchaError, match="timed out"):
            solver.solve_recaptcha_v2(
                site_key="6Lc...",
                page_url="https://example.com",
            )


class TestCaptchaSolution:
    def test_defaults(self):
        s = CaptchaSolution()
        assert s.token == ""
        assert s.task_id == ""

    def test_full(self):
        s = CaptchaSolution(token="abc", task_id="123")
        assert s.token == "abc"
        assert s.task_id == "123"


class TestPlaywrightIntegration:
    def test_form_step_has_solve_captcha(self):
        """Verify the schema roundtrips solve_captcha correctly."""
        from openeraseme.registry.schema import FormStep

        step = FormStep(
            solve_captcha={
                "provider": "capsolver",
                "site_key": "6Lc123",
                "action": "verify",
                "min_score": "0.3",
            }
        )
        data = step.model_dump(exclude_none=True)
        assert "solve_captcha" in data
        assert data["solve_captcha"]["provider"] == "capsolver"
        assert data["solve_captcha"]["site_key"] == "6Lc123"

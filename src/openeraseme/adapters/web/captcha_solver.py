from __future__ import annotations

import json
import logging
import time
from abc import ABC, abstractmethod
from pathlib import Path
from typing import Any
from urllib.parse import urlencode
from urllib.request import Request, urlopen

logger = logging.getLogger(__name__)

CAPSOLVER_BASE = "https://api.capsolver.com"
TWOCAPTCHA_BASE = "https://2captcha.com"
POLL_INTERVAL = 5


class CaptchaError(Exception):
    pass


class CaptchaSolution:
    def __init__(self, token: str = "", task_id: str = "") -> None:
        self.token = token
        self.task_id = task_id


class CaptchaSolver(ABC):
    """Abstract base for captcha solving services."""

    def __init__(self, api_key: str) -> None:
        self._api_key = api_key

    @abstractmethod
    def solve_image(
        self, image_path: str | Path, **kwargs: Any
    ) -> CaptchaSolution: ...

    @abstractmethod
    def solve_recaptcha_v2(
        self, site_key: str, page_url: str, **kwargs: Any
    ) -> CaptchaSolution: ...

    @abstractmethod
    def solve_recaptcha_v3(
        self,
        site_key: str,
        page_url: str,
        action: str = "verify",
        min_score: float = 0.3,
        **kwargs: Any,
    ) -> CaptchaSolution: ...


class CapSolverSolver(CaptchaSolver):
    """CapSolver.com REST API implementation."""

    def solve_image(self, image_path: str | Path, **kwargs: Any) -> CaptchaSolution:
        with open(image_path, "rb") as f:
            import base64
            image_b64 = base64.b64encode(f.read()).decode()

        task = {
            "type": "ImageToTextTask",
            "body": image_b64,
        }
        if "module" in kwargs:
            task["module"] = kwargs["module"]
        return self._solve_task(task)

    def solve_recaptcha_v2(
        self, site_key: str, page_url: str, **kwargs: Any
    ) -> CaptchaSolution:
        task: dict[str, Any] = {
            "type": "ReCaptchaV2Task",
            "websiteURL": page_url,
            "websiteKey": site_key,
        }
        if "is_invisible" in kwargs:
            task["isInvisible"] = kwargs["is_invisible"]
        if "page_action" in kwargs:
            task["pageAction"] = kwargs["page_action"]
        return self._solve_task(task)

    def solve_recaptcha_v3(
        self,
        site_key: str,
        page_url: str,
        action: str = "verify",
        min_score: float = 0.3,
        **kwargs: Any,
    ) -> CaptchaSolution:
        task: dict[str, Any] = {
            "type": "ReCaptchaV3Task",
            "websiteURL": page_url,
            "websiteKey": site_key,
            "pageAction": action,
            "minScore": min_score,
        }
        return self._solve_task(task)

    def _solve_task(self, task: dict[str, Any]) -> CaptchaSolution:
        task_id = self._create_task(task)
        solution = self._await_solution(task_id)
        token = solution.get("gRecaptchaResponse", solution.get("text", ""))
        return CaptchaSolution(token=token, task_id=task_id)

    def _create_task(self, task: dict[str, Any]) -> str:
        payload = json.dumps({
            "clientKey": self._api_key,
            "task": task,
        }).encode()
        req = Request(
            f"{CAPSOLVER_BASE}/createTask",
            data=payload,
            headers={"Content-Type": "application/json"},
        )
        try:
            resp = urlopen(req, timeout=30)
            data = json.loads(resp.read().decode())
        except Exception as e:
            raise CaptchaError(f"CapSolver createTask failed: {e}") from e
        if data.get("errorId", 0) != 0:
            raise CaptchaError(f"CapSolver error: {data.get('errorDescription', 'unknown')}")
        return str(data.get("taskId", ""))

    def _await_solution(self, task_id: str, max_retries: int = 30) -> dict[str, Any]:
        payload = json.dumps({
            "clientKey": self._api_key,
            "taskId": task_id,
        }).encode()
        for _ in range(max_retries):
            time.sleep(POLL_INTERVAL)
            try:
                req = Request(
                    f"{CAPSOLVER_BASE}/getTaskResult",
                    data=payload,
                    headers={"Content-Type": "application/json"},
                )
                resp = urlopen(req, timeout=30)
                data = json.loads(resp.read().decode())
            except Exception as e:
                raise CaptchaError(f"CapSolver getTaskResult failed: {e}") from e
            if data.get("errorId", 0) != 0:
                raise CaptchaError(f"CapSolver error: {data.get('errorDescription', 'unknown')}")
            if data.get("status") == "ready":
                return data.get("solution", {})
        raise CaptchaError(f"CapSolver task {task_id} timed out")


class TwoCaptchaSolver(CaptchaSolver):
    """2captcha.com REST API implementation."""

    def solve_image(self, image_path: str | Path, **kwargs: Any) -> CaptchaSolution:
        with open(image_path, "rb") as f:
            import base64
            image_b64 = base64.b64encode(f.read()).decode()

        params: dict[str, Any] = {
            "key": self._api_key,
            "method": "base64",
            "body": image_b64,
            "json": 1,
        }
        if "phrase" in kwargs:
            params["phrase"] = int(kwargs["phrase"])
        if "regsense" in kwargs:
            params["regsense"] = int(kwargs["regsense"])
        if "numeric" in kwargs:
            params["numeric"] = int(kwargs["numeric"])
        return self._solve(params)

    def solve_recaptcha_v2(
        self, site_key: str, page_url: str, **kwargs: Any
    ) -> CaptchaSolution:
        params: dict[str, Any] = {
            "key": self._api_key,
            "method": "userrecaptcha",
            "googlekey": site_key,
            "pageurl": page_url,
            "json": 1,
        }
        if "invisible" in kwargs:
            params["invisible"] = int(kwargs["invisible"])
        return self._solve(params)

    def solve_recaptcha_v3(
        self,
        site_key: str,
        page_url: str,
        action: str = "verify",
        min_score: float = 0.3,
        **kwargs: Any,
    ) -> CaptchaSolution:
        params: dict[str, Any] = {
            "key": self._api_key,
            "method": "userrecaptcha",
            "googlekey": site_key,
            "pageurl": page_url,
            "action": action,
            "min_score": min_score,
            "json": 1,
            "version": "v3",
        }
        return self._solve(params)

    def _solve(self, params: dict[str, Any]) -> CaptchaSolution:
        captcha_id = self._upload(params)
        solution = self._await_result(captcha_id)
        return CaptchaSolution(token=solution, task_id=captcha_id)

    def _upload(self, params: dict[str, Any]) -> str:
        try:
            url = f"{TWOCAPTCHA_BASE}/in.php?{urlencode(params)}"
            req = Request(url)
            resp = urlopen(req, timeout=30)
            data = json.loads(resp.read().decode())
        except Exception as e:
            raise CaptchaError(f"2captcha in.php failed: {e}") from e
        if data.get("status") != 1:
            error_text = data.get("request", "unknown")
            raise CaptchaError(f"2captcha upload error: {error_text}")
        return str(data.get("request", ""))

    def _await_result(self, captcha_id: str, max_retries: int = 30) -> str:
        params = urlencode({
            "key": self._api_key,
            "action": "get",
            "id": captcha_id,
            "json": 1,
        })
        for _ in range(max_retries):
            time.sleep(POLL_INTERVAL)
            try:
                req = Request(f"{TWOCAPTCHA_BASE}/res.php?{params}")
                resp = urlopen(req, timeout=30)
                data = json.loads(resp.read().decode())
            except Exception as e:
                raise CaptchaError(f"2captcha res.php failed: {e}") from e
            if data.get("status") == 1:
                return str(data.get("request", ""))
            if data.get("request") != "CAPCHA_NOT_READY":
                raise CaptchaError(f"2captcha error: {data.get('request', 'unknown')}")
        raise CaptchaError(f"2captcha captcha {captcha_id} timed out")


def create_solver(
    provider: str,
    api_key: str | None = None,
) -> CaptchaSolver:
    """Create a captcha solver by provider name.

    Reads API key from environment if not provided:
    - capsolver -> CAPSOLVER_API_KEY
    - twocaptcha -> TWOCAPTCHA_API_KEY
    """
    import os

    provider = provider.lower().strip()
    if provider == "capsolver":
        key = api_key or os.environ.get("CAPSOLVER_API_KEY", "")
        if not key:
            raise CaptchaError(
                "CapSolver API key not configured. "
                "Set CAPSOLVER_API_KEY env var or pass --api-key"
            )
        return CapSolverSolver(key)
    elif provider == "twocaptcha":
        key = api_key or os.environ.get("TWOCAPTCHA_API_KEY", "")
        if not key:
            raise CaptchaError(
                "2Captcha API key not configured. "
                "Set TWOCAPTCHA_API_KEY env var or pass --api-key"
            )
        return TwoCaptchaSolver(key)
    else:
        msg = f"Unknown captcha provider: {provider}. Use 'capsolver' or 'twocaptcha'."
        raise CaptchaError(msg)

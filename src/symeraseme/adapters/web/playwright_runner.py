"""Playwright-based web form runner with declarative DSL."""

from __future__ import annotations

import logging
import re
import time
from pathlib import Path
from typing import Any

logger = logging.getLogger(__name__)


class PlaywrightRunnerError(Exception):
    """PlaywrightRunner error."""

    pass


class WebFormResult:
    def __init__(
        self,
        *,
        success: bool = False,
        step_index: int = 0,
        total_steps: int = 0,
        error: str = "",
        screenshot_path: str = "",
        dry_run: bool = False,
    ) -> None:
        self.success = success
        self.step_index = step_index
        self.total_steps = total_steps
        self.error = error
        self.screenshot_path = screenshot_path
        self.dry_run = dry_run


async def run_web_form(
    url: str,
    steps: list[dict[str, Any]],
    *,
    headless: bool = True,
    timeout_seconds: float = 30.0,
    rate_limit_delay: float = 1.0,
    screenshot_dir: str | Path | None = None,
    identity_fields: dict[str, str] | None = None,
) -> WebFormResult:
    """Execute a web form using Playwright.

    Args:
        url: Starting URL of the web form.
        steps: List of step dicts with action keys.
        headless: Run browser in headless mode.
        timeout_seconds: Per-action timeout.
        rate_limit_delay: Polite delay between actions (seconds).
        screenshot_dir: Directory to save screenshots.
        identity_fields: Identity profile fields for form filling.
    """
    try:
        from playwright.async_api import async_playwright
    except ImportError:
        msg = (
            "Playwright is not installed. "
            "Install it via: uv pip install playwright && playwright install chromium"
        )
        raise PlaywrightRunnerError(msg) from None

    screenshot_dir_path = Path(screenshot_dir) if screenshot_dir else None
    if screenshot_dir_path:
        screenshot_dir_path.mkdir(parents=True, exist_ok=True, mode=0o700)

    async with async_playwright() as pw:
        browser = await pw.chromium.launch(headless=headless)
        context = await browser.new_context(
            viewport={"width": 1280, "height": 800},
            user_agent=(
                "Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) "
                "AppleWebKit/537.36 (KHTML, like Gecko) "
                "Chrome/125.0.0.0 Safari/537.36"
            ),
        )
        page = await context.new_page()

        result = WebFormResult(
            success=False,
            total_steps=len(steps),
        )

        try:
            for idx, step in enumerate(steps):
                result.step_index = idx
                await _execute_step(
                    page,
                    step,
                    url if idx == 0 else None,
                    timeout=timeout_seconds,
                    identity_fields=identity_fields or {},
                    screenshot_dir=screenshot_dir_path,
                    step_index=idx,
                )

                if rate_limit_delay > 0:
                    await _async_sleep(rate_limit_delay)

            result.success = True
        except PlaywrightRunnerError:
            raise
        except (RuntimeError, ValueError, OSError) as e:
            result.error = _capture_error(e, page.url if page else url)
            if screenshot_dir_path:
                result.screenshot_path = str(
                    await _save_screenshot(page, screenshot_dir_path, "failure")
                )
            logger.warning("Web form failed at step %d: %s", idx + 1, result.error)
        finally:
            await browser.close()

        return result


async def _execute_step(
    page: Any,
    step: dict[str, Any],
    initial_url: str | None,
    *,
    timeout: float,
    identity_fields: dict[str, str],
    screenshot_dir: Path | None,
    step_index: int,
) -> None:
    """Execute a single form step."""
    step_timeout = timeout * 1000  # ms

    if "goto" in step:
        target_url = step["goto"]
        if initial_url and target_url == ".":
            target_url = initial_url
        logger.debug("Navigating to %s", target_url)
        await page.goto(target_url, timeout=step_timeout, wait_until="domcontentloaded")

    for selector, value in (step.get("fill") or {}).items():
        filled = _resolve_value(value, identity_fields)
        logger.debug("Filling %s", selector)
        await page.fill(selector, filled, timeout=step_timeout)

    for selector, option in (step.get("select") or {}).items():
        logger.debug("Selecting %s = %s", selector, option)
        resolved = _resolve_value(option, identity_fields)
        await page.select_option(selector, resolved, timeout=step_timeout)

    click_target = step.get("click")
    if click_target:
        logger.debug("Clicking %s", click_target)
        await page.click(click_target, timeout=step_timeout)

    wait_for = step.get("wait_for")
    if wait_for:
        logger.debug("Waiting for %s", wait_for)
        await page.wait_for_selector(wait_for, timeout=step_timeout)

    wait_sec = step.get("wait_seconds")
    if wait_sec:
        logger.debug("Waiting %.1f seconds", wait_sec)
        await _async_sleep(wait_sec)

    solve_captcha = step.get("solve_captcha")
    if solve_captcha:
        provider = solve_captcha.get("provider", "capsolver")
        site_key = solve_captcha.get("site_key", "")

        logger.debug("Solving captcha via %s (site_key=%s)", provider, site_key)
        from symeraseme.adapters.web.captcha_solver import CaptchaError, create_solver

        try:
            solver = create_solver(provider)
            current_url = page.url
            result = solver.solve_recaptcha_v2(
                site_key=site_key,
                page_url=current_url,
            )
        except CaptchaError as e:
            msg = f"Captcha solving failed: {e}"
            raise PlaywrightRunnerError(msg) from None

        if result.token:
            logger.debug("Injecting captcha token")
            await page.evaluate(
                'document.getElementById("g-recaptcha-response")?.style.display = "block";'
            )
            await page.fill(
                "textarea#g-recaptcha-response, [name='g-recaptcha-response']",
                result.token,
            )
            await _async_sleep(1)

    assert_text = step.get("assert_text")
    if assert_text:
        logger.debug("Asserting text: %s", assert_text)
        content = await page.text_content("body")
        if content is None or assert_text not in content:
            msg = f"Assertion failed: text '{assert_text}' not found on page"
            raise PlaywrightRunnerError(msg)

    screenshot_name = step.get("screenshot")
    if screenshot_name and screenshot_dir:
        path = await _save_screenshot(page, screenshot_dir, screenshot_name)
        logger.debug("Screenshot saved to %s", path)


def _resolve_value(value: str, identity_fields: dict[str, str]) -> str:
    """Resolve template variables in a value string."""
    if not value:
        return value
    result = value
    for key, val in (identity_fields or {}).items():
        placeholder = f"${{{key}}}"
        if placeholder in result:
            result = result.replace(placeholder, val)
    unresolved = re.findall(r"\$\{[^}]+\}", result)
    if unresolved:
        keys = ", ".join(sorted(set(unresolved)))
        msg = f"Unresolved identity placeholder(s): {keys}"
        raise PlaywrightRunnerError(msg)
    return result


def _capture_error(exc: Exception, current_url: str) -> str:
    """Capture a user-friendly error message."""
    msg = str(exc) or type(exc).__name__
    logger.debug("Playwright error at %s: %s", current_url, msg)
    if "Timeout" in msg or "timed out" in msg.lower():
        return f"Timeout error: {msg[:200]}"
    if "net::" in msg:
        return f"Network error: {msg[:200]}"
    return f"Browser automation error: {msg[:200]}"


async def _save_screenshot(page: Any, directory: Path, name: str) -> Path:
    """Save a page screenshot with restrictive permissions."""
    import os

    timestamp = time.strftime("%Y%m%d-%H%M%S")
    safe_name = "".join(c if c.isalnum() or c in "-_" else "_" for c in name)
    filename = f"{timestamp}_{safe_name}.png"
    path = directory / filename
    await page.screenshot(path=str(path), full_page=True)
    if path.exists():
        os.chmod(path, 0o600)
    return path


async def _async_sleep(seconds: float) -> None:
    """Async sleep helper."""
    import asyncio

    await asyncio.sleep(seconds)

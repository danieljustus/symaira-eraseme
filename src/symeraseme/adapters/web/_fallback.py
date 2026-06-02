from __future__ import annotations

import asyncio
import json
import logging
import threading
import time
from pathlib import Path
from typing import Any

from symeraseme.adapters.web._compat import PlaywrightError

logger = logging.getLogger(__name__)


async def _async_get_content(page: Any) -> str:
    return await page.content()


async def _async_extract_form_fields(page: Any) -> tuple[dict[str, str], list[str]]:
    field_info = await page.evaluate(
        """() => {
            const fields = {};
            const selectors = [];
            document.querySelectorAll('input, select, textarea').forEach(el => {
                if (el.name || el.id) {
                    const key = el.name || el.id;
                    fields[key] = el.value || '';
                    const selector = el.id ? '#' + el.id :
                        'input[name="' + el.name + '"]'
                    selectors.push(selector);
                }
            });
            return { fields: JSON.stringify(fields), selectors: JSON.stringify(selectors) };
        }"""
    )
    fields = json.loads(field_info.get("fields", "{}"))
    selectors = json.loads(field_info.get("selectors", "[]"))
    return fields, selectors


async def _async_save_screenshot(page: Any, directory: Path, name: str) -> str:
    timestamp = time.strftime("%Y%m%d-%H%M%S")
    safe_name = "".join(c if c.isalnum() or c in "-_" else "_" for c in name)
    filename = f"{timestamp}_{safe_name}.png"
    path = directory / filename
    await page.screenshot(path=str(path), full_page=True)
    return str(path)


def capture_form_state(
    page: Any,
    *,
    url: str = "",
    reason: str = "generic_error",
    error_message: str = "",
    step_index: int = 0,
    total_steps: int = 0,
    broker_name: str = "",
    broker_id: str = "",
    screenshot_dir: str | Path | None = None,
) -> Any:
    from symeraseme.core.manual_fallback import FALLBACK_REASONS, FormState

    captured_url = url or getattr(page, "url", "")
    html_snapshot = ""
    form_fields: dict[str, str] = {}
    field_selectors: list[str] = []
    screenshot_path: str | None = None

    async def _capture() -> None:
        nonlocal html_snapshot, form_fields, field_selectors, screenshot_path
        try:
            html_snapshot = await _async_get_content(page)
        except PlaywrightError as e:
            logger.warning("Failed to capture HTML snapshot: %s", e)

        try:
            form_fields, field_selectors = await _async_extract_form_fields(page)
        except PlaywrightError as e:
            logger.warning("Failed to extract form fields: %s", e)

        if screenshot_dir:
            try:
                screenshot_path = await _async_save_screenshot(
                    page, Path(screenshot_dir), "manual_fallback"
                )
            except PlaywrightError as e:
                logger.warning("Failed to save screenshot: %s", e)

    try:
        asyncio.get_running_loop()
    except RuntimeError:
        asyncio.run(_capture())
    else:
        exception: BaseException | None = None

        def _run_in_thread() -> None:
            nonlocal exception
            new_loop = asyncio.new_event_loop()
            asyncio.set_event_loop(new_loop)
            try:
                new_loop.run_until_complete(_capture())
            except BaseException as exc:
                exception = exc
            finally:
                new_loop.close()

        t = threading.Thread(target=_run_in_thread)
        t.start()
        t.join()
        if exception is not None:
            raise exception

    return FormState(
        url=captured_url,
        screenshot_path=screenshot_path,
        html_snapshot=html_snapshot[:5000],
        form_fields=form_fields,
        field_selectors=field_selectors,
        error_message=error_message,
        reason=reason if reason in FALLBACK_REASONS else "generic_error",
        step_index=step_index,
        total_steps=total_steps,
        broker_name=broker_name,
        broker_id=broker_id,
    )

from __future__ import annotations

import logging
import re
import time
from pathlib import Path
from typing import Any
from urllib.parse import urlparse

logger = logging.getLogger(__name__)

RE_URL = re.compile(r"https?://[^\s<>\"']+")

KNOWN_BROKER_DOMAINS: frozenset[str] = frozenset(
    {
        "acxiom.com",
        "oracle.com",
        "schufa.de",
        "beenverified.com",
        "spokeo.com",
        "intelius.com",
        "whitepages.com",
        "mylife.com",
        "peekyou.com",
        "pipl.com",
        "radaris.com",
        "truepeoplesearch.com",
        "ussearch.com",
        "peoplefinders.com",
        "instantcheckmate.com",
        "truthfinder.com",
        "addresses.com",
        "anywho.com",
        "dexknows.com",
        "meridiandata.us",
        "experian.com",
        "transunion.com",
        "equifax.com",
    }
)


class ConfirmationClickerError(Exception):
    pass


class ConfirmationResult:
    def __init__(
        self,
        *,
        success: bool = False,
        clicked_url: str = "",
        step: str = "",
        error: str = "",
        screenshot_before: str = "",
        screenshot_after: str = "",
        dry_run: bool = False,
    ) -> None:
        self.success = success
        self.clicked_url = clicked_url
        self.step = step
        self.error = error
        self.screenshot_before = screenshot_before
        self.screenshot_after = screenshot_after
        self.dry_run = dry_run


def extract_confirmation_links(
    text: str,
    allowed_domains: frozenset[str] | None = None,
) -> list[str]:
    """Extract URLs from email text, filtered by allowed domains."""
    if allowed_domains is None:
        allowed_domains = KNOWN_BROKER_DOMAINS

    urls = RE_URL.findall(text)
    cleaned: list[str] = []
    for url in urls:
        url = url.rstrip(".,;:!?)]>")
        cleaned.append(url)

    seen: set[str] = set()
    filtered: list[str] = []
    for url in cleaned:
        if url in seen:
            continue
        seen.add(url)
        try:
            parsed = urlparse(url)
        except ValueError:
            logger.debug("Failed to parse URL: %s", url)
            continue
        domain = parsed.netloc.lower()
        if domain.startswith("www."):
            domain = domain[4:]
        if allowed_domains and domain not in allowed_domains:
            continue
        filtered.append(url)

    filtered.sort(key=lambda u: (len(urlparse(u).path), len(u)))
    return filtered


async def auto_confirm(
    request_id: int,
    reply_body: str,
    *,
    from_addr: str = "",
    headless: bool = True,
    screenshot_dir: str | Path | None = None,
    rate_limit_delay: float = 2.0,
    dry_run: bool = False,
) -> ConfirmationResult:
    """Detect confirmation links in a broker reply and click them via Playwright."""
    screenshot_dir_path = Path(screenshot_dir) if screenshot_dir else None
    if screenshot_dir_path:
        screenshot_dir_path.mkdir(parents=True, exist_ok=True, mode=0o700)

    allowed_domains: frozenset[str] | None = None
    if from_addr:
        try:
            broker_domain = from_addr.split("@")[-1]
            allowed_domains = KNOWN_BROKER_DOMAINS | {broker_domain}
        except (ValueError, IndexError):
            logger.debug("Failed to extract domain from %s", from_addr)

    links = extract_confirmation_links(reply_body, allowed_domains=allowed_domains)
    if not links:
        return ConfirmationResult(
            success=False,
            step="no_links",
            error="No confirmation links found in reply body",
        )

    target_url = links[0]
    logger.info("Found confirmation link: %s", target_url)

    if dry_run:
        return ConfirmationResult(
            success=True,
            clicked_url=target_url,
            step="dry_run",
            dry_run=True,
        )

    try:
        from playwright.async_api import async_playwright
    except ImportError:
        msg = (
            "Playwright is not installed. "
            "Install via: uv pip install playwright && playwright install chromium"
        )
        raise ConfirmationClickerError(msg) from None

    screenshot_before_path = ""
    screenshot_after_path = ""

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

        result = ConfirmationResult(success=False, clicked_url=target_url)

        try:
            await page.goto(target_url, timeout=30000, wait_until="domcontentloaded")

            if rate_limit_delay > 0:
                await _async_sleep(rate_limit_delay)

            if screenshot_dir_path:
                screenshot_before_path = str(
                    await _save_screenshot(page, screenshot_dir_path, f"req{request_id}_before")
                )
                result.screenshot_before = screenshot_before_path

            click_selectors = [
                "a[href*='confirm']",
                "a[href*='verify']",
                "a[href*='unsubscribe']",
                "button:has-text('Confirm')",
                "button:has-text('Yes')",
                "button:has-text('Verify')",
                "button:has-text('Unsubscribe')",
                "a:has-text('Confirm')",
                "a:has-text('Yes')",
                "a:has-text('Verify')",
                "a:has-text('Click here')",
                "input[type='submit']",
                "button[type='submit']",
            ]

            clicked = False
            for selector in click_selectors:
                try:
                    el = await page.wait_for_selector(selector, timeout=3000)
                    if el:
                        logger.debug("Clicking %s", selector)
                        await el.click()
                        clicked = True
                        result.step = f"clicked_{selector}"
                        break
                except Exception:
                    logger.debug("Selector %s not found or not clickable", selector)
                    continue

            if not clicked:
                try:
                    first_link = await page.wait_for_selector("a", timeout=2000)
                    if first_link:
                        await first_link.click()
                        clicked = True
                        result.step = "clicked_fallback_link"
                except Exception:
                    logger.debug("Fallback link click failed")

            if not clicked:
                result.error = "No clickable confirmation element found"
                result.step = "no_element"
            else:
                result.success = True

            if rate_limit_delay > 0:
                await _async_sleep(rate_limit_delay)

            if screenshot_dir_path:
                suffix = "success" if result.success else "failed"
                screenshot_after_path = str(
                    await _save_screenshot(
                        page,
                        screenshot_dir_path,
                        f"req{request_id}_after_{suffix}",
                    )
                )
                result.screenshot_after = screenshot_after_path

        except Exception as e:
            result.error = _capture_clicker_error(e, page.url if page else target_url)
            if screenshot_dir_path:
                screenshot_after_path = str(
                    await _save_screenshot(page, screenshot_dir_path, f"req{request_id}_error")
                )
                result.screenshot_after = screenshot_after_path
            logger.warning("Auto-confirm failed: %s", result.error)
        finally:
            await browser.close()

    return result


def _capture_clicker_error(exc: Exception, current_url: str) -> str:
    msg = str(exc) or type(exc).__name__
    if "Timeout" in msg or "timed out" in msg.lower():
        return f"Timeout at {current_url}: {msg[:200]}"
    if "net::" in msg:
        return f"Network error at {current_url}: {msg[:200]}"
    return f"Error at {current_url}: {msg[:200]}"


async def _save_screenshot(page: Any, directory: Path, name: str) -> Path:
    """Save a page screenshot with restrictive permissions."""
    import os

    timestamp = time.strftime("%Y%m%d-%H%M%S")
    safe_name = "".join(c if c.isalnum() or c in "-_" else "_" for c in name)
    path = directory / f"{timestamp}_{safe_name}.png"
    await page.screenshot(path=str(path), full_page=True)
    os.chmod(path, 0o600)
    return path


async def _async_sleep(seconds: float) -> None:
    import asyncio

    await asyncio.sleep(seconds)

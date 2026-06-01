"""Shared fixtures for smoke tests."""

from __future__ import annotations

import json
import os
from pathlib import Path
from typing import Any
from unittest.mock import patch

import pytest
from typer.testing import CliRunner, Result

from symeraseme.cli import app
from symeraseme.core.db import close_connection

runner = CliRunner()


@pytest.fixture(autouse=True)
def _clean_env() -> None:
    """Ensure isolated environment state before each test."""
    keys = [
        "SYMERASEME_DATA_DIR",
        "SYMERASEME_DB_DIR",
        "SYMERASEME_IDENTITY_PATH",
        "SYMERASEME_LLM_CONSENT",
    ]
    saved = {k: os.environ.pop(k, None) for k in keys}
    os.environ["SYMERASEME_LLM_CONSENT"] = "1"
    close_connection()
    yield
    close_connection()
    for k, v in saved.items():
        if v is not None:
            os.environ[k] = v
        else:
            os.environ.pop(k, None)


@pytest.fixture(autouse=True)
def _fake_keyring() -> None:
    store: dict[str, str] = {}

    def set_pass(service: str, username: str, password: str) -> None:
        store[f"{service}:{username}"] = password

    def get_pass(service: str, username: str) -> str | None:
        return store.get(f"{service}:{username}")

    def del_pass(service: str, username: str) -> None:
        store.pop(f"{service}:{username}", None)

    with (
        patch("symeraseme.core.identity.keyring.set_password", set_pass),
        patch("symeraseme.core.identity.keyring.get_password", get_pass),
        patch("symeraseme.core.identity.keyring.delete_password", del_pass),
    ):
        yield


@pytest.fixture()
def tmp_home(tmp_path: Path) -> Path:
    """Provide an isolated temp directory as the fake home/data dir."""
    data_dir = tmp_path / "data"
    data_dir.mkdir()
    os.environ["SYMERASEME_DATA_DIR"] = str(data_dir)
    os.environ["SYMERASEME_DB_DIR"] = str(tmp_path)
    os.environ["SYMERASEME_IDENTITY_PATH"] = str(tmp_path / "identity.enc")
    return tmp_path


@pytest.fixture()
def seeded_db(tmp_home: Path) -> None:
    """Initialize DB with a seeded campaign and some removal requests."""
    from symeraseme.core.db import init_db
    from symeraseme.core.events import append_event, create_campaign, create_removal_request
    from symeraseme.core.identity import save_profile
    from symeraseme.core.projection import upsert_state
    from symeraseme.registry.schema import IdentityProfile

    save_profile(
        IdentityProfile(
            full_name="Smoke Test",
            email_addresses=["smoke@test.com"],
        )
    )

    init_db()
    create_campaign("smoke-test", kind="initial", notes="smoke test campaign")
    create_campaign("smoke-test-ccpa", kind="initial", notes="ccpa campaign")

    bids = []
    for broker_id in ["acxiom", "oracle", "spokeo", "verisk", "corelogic"]:
        rid = create_removal_request(
            broker_id=broker_id,
            campaign_id="smoke-test",
            jurisdiction="GDPR-DE",
            template_id="gdpr-art17.de.md.j2",
        )
        bids.append(rid)
        append_event(rid, "PLANNED", payload={"broker_name": broker_id, "channel": "email"})
        upsert_state(rid)

    # Add a few CCPA requests
    for broker_id in ["acxiom", "spokeo"]:
        rid = create_removal_request(
            broker_id=broker_id,
            campaign_id="smoke-test-ccpa",
            jurisdiction="CCPA-US",
            template_id="ccpa-opt-out.en.md.j2",
        )
        bids.append(rid)
        append_event(rid, "PLANNED", payload={"broker_name": broker_id, "channel": "email"})
        upsert_state(rid)


def invoke(*args: str, **kwargs: Any) -> Result:
    """Helper to invoke the CLI app."""
    return runner.invoke(app, list(args), **kwargs)


def assert_ok(result: Result) -> None:
    """Assert the command succeeded."""
    assert result.exit_code == 0, (
        f"CLI command failed:\n"
        f"  exit: {result.exit_code}\n"
        f"  stdout: {result.stdout[:500]}\n"
        f"  stderr: {result.stderr[:500]}"
    )


def assert_json_output(result: Result) -> dict[str, Any]:
    """Assert the command returned valid JSON output."""
    assert_ok(result)
    try:
        return json.loads(result.stdout)
    except json.JSONDecodeError as e:
        pytest.fail(f"Expected JSON output, got: {result.stdout[:500]}\nError: {e}")
        return {}  # unreachable


def assert_in_output_stderr(result: Result, text: str) -> None:
    combined = result.stdout + result.stderr
    assert text in combined, (
        f"Expected {text!r} in output:\n"
        f"  stdout: {result.stdout[:300]}\n"
        f"  stderr: {result.stderr[:300]}"
    )

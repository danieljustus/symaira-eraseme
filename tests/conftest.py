"""Session-wide test fixtures: make the test suite hermetic (issue #598).

Before this conftest existed, identity crypto tests talked to the host OS
keyring: ``symeraseme.core.identity`` reads the AES-256 master key via
``keyring.get_password``, so on developer machines where ``init-profile``
had been run, unit/integration tests failed with ``cryptography.exceptions
.InvalidTag`` while CI stayed green on an empty keyring. The suite was
order- and environment-sensitive (e.g. ``test_llm_factory.py`` failed only
in full-suite runs).

Two mechanisms make every test deterministic and machine-independent:

1. A real ``keyring`` backend class backed by a process-local dict is
   installed via ``keyring.set_keyring()`` at conftest import time, so the
   whole session — unit, integration and smoke tests alike — reads and
   writes an isolated, in-memory keyring and never the host's.
2. ``SYMERASEME_IDENTITY_PATH`` is pinned to a session-scoped temporary
   file, so code paths that call ``load_profile()`` on the *default* path
   (planning, execution, reply rendering) never see — or overwrite — the
   developer's real ``~/.config/symeraseme/identity.enc``.
"""

from __future__ import annotations

import os
from collections.abc import Iterator
from pathlib import Path

import keyring
import pytest
from keyring.backend import KeyringBackend


class InMemoryKeyring(KeyringBackend):
    """Deterministic keyring backend backed by a process-local dict.

    Implements the same protocol production code relies on
    (``get_password`` / ``set_password`` / ``delete_password`` keyed by
    service and username) so it can stand in for the host keyring for the
    whole test session.
    """

    def __init__(self) -> None:
        self._store: dict[tuple[str, str], str] = {}

    @property
    def priority(self) -> float:
        # Higher than any real backend so it always wins if re-discovered.
        return 1_000_000

    def get_password(self, service: str, username: str) -> str | None:
        return self._store.get((service, username))

    def set_password(self, service: str, username: str, password: str) -> None:
        self._store[(service, username)] = password

    def delete_password(self, service: str, username: str) -> None:
        self._store.pop((service, username), None)


# Install the in-memory backend as early as possible — at conftest import
# time, before any test module or fixture can reach the host keyring.
# keyring.set_keyring() pins the backend process-wide.
keyring.set_keyring(InMemoryKeyring())


@pytest.fixture(scope="session", autouse=True)
def _hermetic_identity_path(tmp_path_factory: pytest.TempPathFactory) -> Iterator[Path]:
    """Pin the default identity profile path to an isolated temp file.

    ``symeraseme.core.config`` resolves ``identity_path`` to
    ``~/.config/symeraseme/identity.enc`` when the env var is unset; tests
    exercising plan/execute/reply paths call ``load_profile()`` on that
    default path, so without isolation they read the developer's real
    profile (whose keyring entry does not match, hence InvalidTag) or a
    file encrypted with a key unknown to the fake store.
    """
    identity_path = tmp_path_factory.mktemp("symeraseme-identity") / "identity.enc"
    os.environ["SYMERASEME_IDENTITY_PATH"] = str(identity_path)
    yield identity_path
    os.environ.pop("SYMERASEME_IDENTITY_PATH", None)


# LLM-related env vars are read by ``symeraseme.llm.factory`` at call time
# (provider, model, base URL, Ollama host). A developer shell may export them
# (e.g. ``SYMERASEME_LLM_PROVIDER=ollama`` to use a local model), which makes
# ``tests/unit/test_llm_factory.py`` fail in full-suite runs while CI stays
# green on runners without them. Pin them to unset for the whole session so
# local runs match CI regardless of the developer environment.
_LLM_ENV_VARS = (
    "SYMERASEME_LLM_PROVIDER",
    "SYMERASEME_LLM_MODEL",
    "SYMERASEME_LLM_BASE_URL",
    "OLLAMA_HOST",
)


@pytest.fixture(scope="session", autouse=True)
def _hermetic_llm_env() -> Iterator[None]:
    """Remove LLM-related environment overrides for the whole session."""
    saved = {name: os.environ.pop(name, None) for name in _LLM_ENV_VARS}
    yield
    for name, value in saved.items():
        if value is not None:
            os.environ[name] = value


@pytest.fixture(scope="session")
def fake_keyring_backend() -> InMemoryKeyring:
    """Expose the session's in-memory keyring backend for contract tests."""
    return keyring.get_keyring()

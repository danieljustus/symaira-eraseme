"""Plan, execute, and poll orchestration for removal campaigns.

This module re-exports all public orchestration functions from focused
submodules for backward compatibility. New code should import directly
from the submodules (planning, execution, batch, inbox).
"""

from __future__ import annotations

import warnings

__all__ = [  # noqa: F822
    "execute_campaign",
    "execute_campaign_async",
    "execute_request",
    "get_plan",
    "plan_campaign",
    "submit_inbox_reply",
]

_DEPRECATED_NAMES = frozenset(__all__)


def __getattr__(name: str):
    if name not in _DEPRECATED_NAMES:
        raise AttributeError(f"module {__name__!r} has no attribute {name!r}")
    warnings.warn(
        "symeraseme.core.orchestrator is deprecated; import directly from "
        "symeraseme.core.planning, symeraseme.core.execution, "
        "symeraseme.core.batch, or symeraseme.core.inbox.",
        DeprecationWarning,
        stacklevel=2,
    )
    if name == "execute_campaign":
        from symeraseme.core.batch import execute_campaign  # noqa: PLC0415

        return execute_campaign
    if name == "execute_campaign_async":
        from symeraseme.core.batch import execute_campaign_async  # noqa: PLC0415

        return execute_campaign_async
    if name == "execute_request":
        from symeraseme.core.execution import execute_request  # noqa: PLC0415

        return execute_request
    if name == "submit_inbox_reply":
        from symeraseme.core.inbox import submit_inbox_reply  # noqa: PLC0415

        return submit_inbox_reply
    if name == "get_plan":
        from symeraseme.core.planning import get_plan  # noqa: PLC0415

        return get_plan
    if name == "plan_campaign":
        from symeraseme.core.planning import plan_campaign  # noqa: PLC0415

        return plan_campaign
    return None  # unreachable

"""Plan, execute, and poll orchestration for removal campaigns.

This module re-exports all public orchestration functions from focused
submodules for backward compatibility. New code should import directly
from the submodules (planning, execution, batch, inbox).
"""

from __future__ import annotations

import warnings

from symeraseme.core.batch import execute_campaign, execute_campaign_async
from symeraseme.core.execution import execute_request
from symeraseme.core.inbox import submit_inbox_reply
from symeraseme.core.planning import get_plan, plan_campaign

warnings.warn(
    "symeraseme.core.orchestrator is deprecated; import directly from "
    "symeraseme.core.planning, symeraseme.core.execution, "
    "symeraseme.core.batch, or symeraseme.core.inbox.",
    DeprecationWarning,
    stacklevel=2,
)

__all__ = [
    "execute_campaign",
    "execute_campaign_async",
    "execute_request",
    "get_plan",
    "plan_campaign",
    "submit_inbox_reply",
]

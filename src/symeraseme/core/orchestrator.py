"""Plan, execute, and poll orchestration for removal campaigns.

This module re-exports all public orchestration functions from focused
submodules for backward compatibility. New code should import directly
from the submodules (planning, execution, batch, inbox).
"""

from __future__ import annotations

from symeraseme.core.batch import execute_campaign, execute_campaign_async
from symeraseme.core.execution import execute_request
from symeraseme.core.inbox import submit_inbox_reply
from symeraseme.core.planning import get_plan, plan_campaign

__all__ = [
    "execute_campaign",
    "execute_campaign_async",
    "execute_request",
    "get_plan",
    "plan_campaign",
    "submit_inbox_reply",
]

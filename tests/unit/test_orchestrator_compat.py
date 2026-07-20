"""Tests for the core orchestrator backward-compatibility layer."""

from __future__ import annotations

import pytest
import warnings


def test_orchestrator_compat_exports():
    # Clear warnings registry to ensure DeprecationWarning is captured
    warnings.simplefilter("always", DeprecationWarning)
    
    import symeraseme.core.orchestrator as orchestrator
    from symeraseme.core import batch, execution, inbox, planning

    # List of tuples: (attribute_name, expected_target)
    cases = [
        ("execute_campaign", batch.execute_campaign),
        ("execute_campaign_async", batch.execute_campaign_async),
        ("execute_request", execution.execute_request),
        ("submit_inbox_reply", inbox.submit_inbox_reply),
        ("get_plan", planning.get_plan),
        ("plan_campaign", planning.plan_campaign),
    ]

    for attr, target in cases:
        with pytest.warns(DeprecationWarning) as record:
            val = getattr(orchestrator, attr)
            assert val is target
        
        # Verify deprecation message content
        assert len(record) == 1
        assert "symeraseme.core.orchestrator is deprecated" in str(record[0].message)


def test_orchestrator_compat_unknown_attribute():
    import symeraseme.core.orchestrator as orchestrator

    with warnings.catch_warnings(record=True) as recorded_warnings:
        warnings.simplefilter("always")
        with pytest.raises(AttributeError) as exc_info:
            _ = orchestrator.invalid_attribute
        
        assert "module 'symeraseme.core.orchestrator' has no attribute 'invalid_attribute'" in str(exc_info.value)
    
    # Assert no deprecation warnings were emitted for invalid attributes
    assert not any(issubclass(w.category, DeprecationWarning) for w in recorded_warnings)


def test_orchestrator_compat_unreachable_fallback(monkeypatch):
    import symeraseme.core.orchestrator as orchestrator

    monkeypatch.setattr(orchestrator, "_DEPRECATED_NAMES", frozenset(["dummy"]))
    with pytest.warns(DeprecationWarning):
        val = orchestrator.dummy
    assert val is None

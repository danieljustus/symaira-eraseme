"""Test that events.py facade signatures stay in sync with repositories."""

from __future__ import annotations

import inspect

from symeraseme.core import events
from symeraseme.core.repositories import (
    append_event as repo_append_event,
    create_campaign as repo_create_campaign,
    create_removal_request as repo_create_removal_request,
    get_events as repo_get_events,
    get_events_for_requests as repo_get_events_for_requests,
    get_removal_request as repo_get_removal_request,
    list_campaigns as repo_list_campaigns,
    list_removal_requests as repo_list_removal_requests,
)


def _compare_signatures(facade_fn, repo_fn, name: str) -> None:
    facade_sig = inspect.signature(facade_fn)
    repo_sig = inspect.signature(repo_fn)
    facade_params = list(facade_sig.parameters.keys())
    repo_params = list(repo_sig.parameters.keys())
    assert facade_params == repo_params, (
        f"{name}: parameter mismatch — facade={facade_params}, repo={repo_params}"
    )


def test_create_campaign_signature_sync():
    _compare_signatures(events.create_campaign, repo_create_campaign, "create_campaign")


def test_list_campaigns_signature_sync():
    _compare_signatures(events.list_campaigns, repo_list_campaigns, "list_campaigns")


def test_create_removal_request_signature_sync():
    _compare_signatures(
        events.create_removal_request,
        repo_create_removal_request,
        "create_removal_request",
    )


def test_append_event_signature_sync():
    _compare_signatures(events.append_event, repo_append_event, "append_event")


def test_get_events_for_requests_signature_sync():
    _compare_signatures(
        events.get_events_for_requests,
        repo_get_events_for_requests,
        "get_events_for_requests",
    )


def test_get_events_signature_sync():
    _compare_signatures(events.get_events, repo_get_events, "get_events")


def test_get_removal_request_signature_sync():
    _compare_signatures(
        events.get_removal_request,
        repo_get_removal_request,
        "get_removal_request",
    )


def test_list_removal_requests_signature_sync():
    _compare_signatures(
        events.list_removal_requests,
        repo_list_removal_requests,
        "list_removal_requests",
    )


def test_facade_has_validation_constants():
    assert hasattr(events, "EVENT_TYPES")
    assert hasattr(events, "VALID_SOURCES")
    assert isinstance(events.EVENT_TYPES, frozenset)
    assert isinstance(events.VALID_SOURCES, frozenset)

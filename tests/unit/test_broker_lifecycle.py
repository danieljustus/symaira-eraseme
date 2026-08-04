"""Regression tests for broker lifecycle status handling."""

from __future__ import annotations

from unittest.mock import patch

from symeraseme.registry.loader import clear_registry_cache, load_all_brokers
from symeraseme.registry.schema import (
    Broker,
    BrokerCategory,
    BrokerStatus,
    EmailOptOut,
    Law,
    Priority,
)


def _broker_yaml(broker_id: str, *, status: str | None = None) -> str:
    status_line = f"status: {status}\n" if status is not None else ""
    return (
        f"id: {broker_id}\n"
        f"name: {broker_id}\n"
        "website: https://example.com\n"
        "category: other\n"
        "jurisdictions:\n"
        "- US\n"
        "laws:\n"
        "- CCPA\n"
        "priority: medium\n"
        "opt_out:\n"
        "- type: email\n"
        "  endpoint: test@example.com\n"
        "  template: ccpa-deletion\n"
        "  locale: en\n"
        f"{status_line}"
    )


def test_broker_status_defaults_and_rejects_unknown_values():
    broker = Broker.model_validate(
        {
            "id": "status-default",
            "name": "Status Default",
            "website": "https://example.com",
            "category": "other",
            "jurisdictions": ["US"],
            "laws": ["CCPA"],
            "priority": "medium",
            "opt_out": [
                {
                    "type": "email",
                    "endpoint": "test@example.com",
                    "template": "ccpa-deletion",
                    "locale": "en",
                }
            ],
        }
    )
    assert broker.status is BrokerStatus.active
    assert (
        Broker.model_validate({**broker.model_dump(mode="json"), "status": "merged"}).status
        is BrokerStatus.merged
    )

    try:
        Broker.model_validate({**broker.model_dump(mode="json"), "status": "unknown"})
    except ValueError:
        pass
    else:
        raise AssertionError("unknown lifecycle statuses must be rejected")


def test_loader_filters_lifecycle_statuses_across_cold_warm_and_persistent_cache(
    tmp_path, monkeypatch
):
    (tmp_path / "active.yaml").write_text(_broker_yaml("active"))
    (tmp_path / "inactive.yaml").write_text(_broker_yaml("inactive", status="out-of-business"))
    cache_dir = tmp_path / "cache"
    cache_dir.mkdir()
    monkeypatch.setattr("symeraseme.registry.loader._cache_dir", lambda: cache_dir)
    clear_registry_cache()

    active = load_all_brokers(registry_dir=tmp_path)
    assert [broker.id for broker in active] == ["active"]
    assert active[0].status is BrokerStatus.active

    inactive = load_all_brokers(registry_dir=tmp_path, status=BrokerStatus.out_of_business)
    assert [broker.id for broker in inactive] == ["inactive"]

    all_brokers = load_all_brokers(registry_dir=tmp_path, include_inactive=True)
    assert {broker.id for broker in all_brokers} == {"active", "inactive"}

    clear_registry_cache()
    persistent_inactive = load_all_brokers(
        registry_dir=tmp_path,
        status=BrokerStatus.out_of_business,
    )
    assert [broker.id for broker in persistent_inactive] == ["inactive"]


def _planning_broker(status: BrokerStatus) -> Broker:
    return Broker(
        id=f"{status.value}-broker",
        name=status.value,
        website="https://example.com",
        category=BrokerCategory.other,
        jurisdictions=["US"],
        laws=[Law.ccpa],
        priority=Priority.medium,
        status=status,
        opt_out=[
            EmailOptOut(
                endpoint="test@example.com",
                template="ccpa-deletion",
                locale="en",
            )
        ],
    )


def test_planning_defaults_to_active_and_can_include_inactive():
    from symeraseme.core.planning import plan_campaign

    brokers = [_planning_broker(BrokerStatus.active), _planning_broker(BrokerStatus.merged)]
    with (
        patch("symeraseme.core.planning.create_campaign", return_value=True),
        patch("symeraseme.core.planning.load_all_brokers", return_value=brokers) as load,
        patch("symeraseme.core.planning.create_removal_request", return_value=1),
        patch("symeraseme.core.planning.append_event_and_project"),
    ):
        plan_campaign(campaign_id="active-only")
        load.assert_called_once_with(
            jurisdiction=None,
            law=None,
            priority=None,
            category=None,
            status=BrokerStatus.active,
            include_inactive=False,
        )

    with (
        patch("symeraseme.core.planning.create_campaign", return_value=True),
        patch("symeraseme.core.planning.load_all_brokers", return_value=brokers) as load,
        patch("symeraseme.core.planning.create_removal_request", return_value=1),
        patch("symeraseme.core.planning.append_event_and_project"),
    ):
        plan_campaign(campaign_id="all-statuses", include_inactive=True)
        load.assert_called_once_with(
            jurisdiction=None,
            law=None,
            priority=None,
            category=None,
            status=BrokerStatus.active,
            include_inactive=True,
        )

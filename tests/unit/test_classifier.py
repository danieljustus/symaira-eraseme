"""Tests for the LLM reply classifier."""

from __future__ import annotations

from pathlib import Path
from unittest.mock import MagicMock, patch

from symeraseme.adapters.triage.classifier import (
    CLASSIFICATION_TO_EVENT,
    ReplyClassifier,
    _parse_response,
    build_user_prompt,
)
from symeraseme.llm.protocol import LLMClientError

_EMPTY_FIELDS = '"extracted_fields": {}'
_ACK = '"classification": "ack"'


def load_fixture(name: str) -> str:
    path = Path(__file__).parent.parent / "fixtures" / "broker_replies" / name
    return path.read_text()


def _r(cls_label: str, conf: str, summary: str) -> str:
    return (
        '{"classification": "'
        + cls_label
        + '", "confidence": '
        + conf
        + ', "summary": "'
        + summary
        + '", '
        + _EMPTY_FIELDS
        + "}"
    )


class TestBuildUserPrompt:
    def test_builds_prompt_with_all_fields(self):
        prompt = build_user_prompt(
            broker_name="TestBroker",
            broker_website="https://testbroker.com",
            original_subject="Data Deletion Request",
            original_request_snippet="Please delete my data",
            reply_subject="Re: Data Deletion Request",
            reply_body="We received your request",
        )
        assert "TestBroker" in prompt
        assert "https://testbroker.com" in prompt
        assert "Data Deletion Request" in prompt
        assert "We received your request" in prompt

    def test_handles_missing_original(self):
        prompt = build_user_prompt(
            broker_name="TestBroker",
            broker_website="https://testbroker.com",
            original_subject="",
            original_request_snippet="",
            reply_subject="Reply",
            reply_body="Body",
        )
        assert "TestBroker" in prompt
        assert "Reply" in prompt
        assert "Body" in prompt

    def test_truncates_long_body(self):
        long_body = "x" * 3000
        prompt = build_user_prompt(
            broker_name="B",
            broker_website="https://b.com",
            original_subject="",
            original_request_snippet="",
            reply_subject="S",
            reply_body=long_body,
        )
        assert len(prompt) < 2500


class TestParseResponse:
    def test_parses_valid_json(self):
        result = _parse_response(
            '{"classification": "ack", "confidence": 0.95, "summary": "Ack", ' + _EMPTY_FIELDS + "}"
        )
        assert result.label == "ack"
        assert result.event_type == "ACK"
        assert result.confidence == 0.95
        assert result.needs_human_review is False

    def test_parses_confirmed(self):
        result = _parse_response(_r("confirmed", "0.92", "Del"))
        assert result.label == "confirmed"
        assert result.event_type == "CONFIRMED"

    def test_parses_rejected(self):
        result = _parse_response(_r("rejected", "0.88", "Invalid"))
        assert result.event_type == "REJECTED_FINAL"

    def test_parses_verification(self):
        result = _parse_response(_r("verification", "0.91", "ID"))
        assert result.event_type == "VERIFICATION_REQUESTED"

    def test_parses_autoresponder(self):
        result = _parse_response(_r("autoresponder", "0.98", "OOTO"))
        assert result.event_type == "AUTORESPONDER"

    def test_parses_bounce(self):
        result = _parse_response(_r("bounce", "0.99", "Fail"))
        assert result.event_type == "BOUNCE"

    def test_low_confidence_triggers_review(self):
        result = _parse_response(_r("ack", "0.3", "Low"))
        assert result.needs_human_review is True

    def test_unclear_label_triggers_review(self):
        result = _parse_response(_r("unclear", "0.5", "No"))
        assert result.needs_human_review is True
        assert result.event_type == "HUMAN_ACTION_REQUIRED"

    def test_unknown_label_defaults_to_unclear(self):
        result = _parse_response(_r("unknown_label", "0.9", "?"))
        assert result.label == "unclear"
        assert result.needs_human_review is True

    def test_handles_malformed_json(self):
        result = _parse_response("not json at all")
        assert result.label == "unclear"
        assert result.confidence == 0.0
        assert result.needs_human_review is True

    def test_handles_codeblock_json(self):
        result = _parse_response(
            "```json\n{"
            + _ACK
            + ', "confidence": 0.9, "summary": "ok", '
            + _EMPTY_FIELDS
            + "}\n```"
        )
        assert result.label == "ack"
        assert result.confidence == 0.9

    def test_clamps_confidence(self):
        result = _parse_response(_r("ack", "1.5", "Over"))
        assert result.confidence == 1.0

        result = _parse_response(_r("ack", "-0.5", "Under"))
        assert result.confidence == 0.0

    def test_all_labels_have_event(self):
        from symeraseme.adapters.triage.classifier import CLASSIFICATION_LABELS

        for label in CLASSIFICATION_LABELS:
            assert label in CLASSIFICATION_TO_EVENT, f"Missing event mapping for {label}"


class TestReplyClassifier:
    def test_available_returns_false_without_key(self):
        with patch("symeraseme.llm.factory.create_llm_client") as mock_factory:
            mock_client = MagicMock()
            mock_client.is_available.return_value = False
            mock_factory.return_value = mock_client
            classifier = ReplyClassifier()
            assert classifier.is_available() is False
            mock_factory.assert_called_once()

    def test_classify_fallback_on_api_error(self):
        class MockClient:
            @staticmethod
            def is_available():
                return True

            @staticmethod
            def classify(**kwargs):
                raise LLMClientError("API down")

        classifier = ReplyClassifier(client=MockClient())
        result = classifier.classify(
            broker_name="TestBroker",
            reply_subject="Re: request",
            reply_body="Hello",
        )
        assert result.label == "unclear"
        assert result.needs_human_review is True

    def test_close_sets_client_to_none(self):
        classifier = ReplyClassifier(client=MagicMock(spec=[]))
        classifier.close()
        assert classifier._client is None

    def test_classifier_uses_factory_by_default(self):
        with patch("symeraseme.llm.factory.create_llm_client") as mock_factory:
            mock_client = MagicMock()
            mock_client.is_available.return_value = False
            mock_factory.return_value = mock_client
            classifier = ReplyClassifier()
            assert classifier._client is mock_client
            mock_factory.assert_called_once()

    def test_classifier_accepts_custom_client(self):
        class AvailableClient:
            @staticmethod
            def is_available():
                return True

            @staticmethod
            def classify(system_prompt, user_prompt, **kwargs):
                return (
                    '{"classification": "confirmed", "confidence": 0.99, '
                    '"summary": "Done", "extracted_fields": {}}',
                    None,
                )

        classifier = ReplyClassifier(client=AvailableClient())
        assert classifier.is_available() is True
        result = classifier.classify(
            broker_name="Test",
            reply_subject="Re: request",
            reply_body="Hello",
        )
        assert result.label == "confirmed"
        assert result.confidence == 0.99


class TestClassificationEdgeCases:
    def test_empty_body(self):
        result = _parse_response(_r("unclear", "0.0", "Empty"))
        assert result.needs_human_review is True

    def test_long_summary_truncated(self):
        long_summary = "x" * 500
        result = _parse_response(
            '{"classification": "ack", "confidence": 0.9, '
            + f'"summary": "{long_summary}", {_EMPTY_FIELDS}'
            + "}"
        )
        assert len(result.summary) <= 200


class TestClassifierIntegration:
    FIXTURE_EXPECTATIONS: list[tuple[str, str]] = [
        ("ack_gdpr.txt", "ack"),
        ("confirmed_account_deleted.txt", "confirmed"),
        ("rejected_invalid_id.txt", "rejected"),
        ("verification_needed.txt", "verification"),
        ("autoresponder_ooto.txt", "autoresponder"),
        ("bounce_no_mailbox.txt", "bounce"),
    ]

    def test_all_fixtures_can_be_loaded(self):
        for filename, _expected in self.FIXTURE_EXPECTATIONS:
            content = load_fixture(filename)
            assert len(content) > 50, f"Fixture {filename} is too short"

    def test_build_user_prompt_with_fixtures(self):
        for filename, _expected in self.FIXTURE_EXPECTATIONS:
            body = load_fixture(filename)
            prompt = build_user_prompt(
                broker_name="TestBroker",
                broker_website="https://testbroker.com",
                original_subject="Data Deletion Request",
                original_request_snippet="Please delete my personal data",
                reply_subject="Re: Data Deletion Request",
                reply_body=body,
            )
            assert "TestBroker" in prompt
            assert "Data Deletion Request" in prompt or "data" in prompt.lower()
            assert len(prompt) > 100


class TestAnthropicClientRetry:
    """Verify that SDK exceptions are properly mapped and trigger retries."""

    def test_rate_limit_error_triggers_retry_and_raises_custom_error(self):
        """Simulate anthropic.RateLimitError → retry loop → AnthropicClientRateLimitError."""
        from unittest.mock import MagicMock

        import anthropic
        import pytest

        from symeraseme.llm.anthropic_client import (
            AnthropicClient,
            AnthropicClientRateLimitError,
        )

        client = AnthropicClient(api_key="test-key", max_retries=3)

        # Bypass lazy init so we can inject a mock client
        client._client = MagicMock()

        mock_response = MagicMock()
        mock_response.status_code = 429
        rate_limit_err = anthropic.RateLimitError(
            message="429 Too Many Requests",
            response=mock_response,
            body={"error": {"type": "rate_limit_error", "message": "Rate limited"}},
        )
        client._client.messages.create.side_effect = rate_limit_err

        with pytest.raises(AnthropicClientRateLimitError) as exc_info:
            client.classify(
                system_prompt="You are helpful.",
                user_prompt="Hello",
            )

        assert client._client.messages.create.call_count == 3
        assert "429 Too Many Requests" in str(exc_info.value)


class TestCLassifierCLI:
    def test_classifier_importable(self):
        from symeraseme.adapters.triage import classifier

        assert classifier is not None

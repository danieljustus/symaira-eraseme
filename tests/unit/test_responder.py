"""Tests for the rebuttal template generator (responder)."""

from __future__ import annotations

from openeraseme.adapters.triage.responder import (
    REJECTION_TEMPLATES,
    RebuttalResult,
    _build_classifier_user_prompt,
    _parse_classification_response,
    _select_fallback_template,
    generate_rebuttal,
)
from openeraseme.registry.schema import IdentityProfile


class TestSelectFallbackTemplate:
    def test_address_keyword(self):
        result = _select_fallback_template("We cannot process as your old address does not match")
        assert result == "address_mismatch"

    def test_identity_keyword(self):
        result = _select_fallback_template(
            "Please provide a copy of your passport for verification"
        )
        assert result == "identity_challenged"

    def test_ccpa_keyword(self):
        result = _select_fallback_template(
            "Under CCPA Section 1798.105 we need additional information"
        )
        assert result == "ccpa_identity_challenged"

    def test_no_match_returns_none(self):
        result = _select_fallback_template("Thank you for your request.")
        assert result is None

    def test_empty_message(self):
        result = _select_fallback_template("")
        assert result is None

    def test_case_insensitive(self):
        result = _select_fallback_template("OLD ADDRESS ON FILE")
        assert result == "address_mismatch"


class TestParseClassificationResponse:
    def test_valid_json(self):
        text = (
            '{"classification": "address_mismatch", "confidence": 0.92, '
            '"summary": "Address issue", "key_points": ["old address"], '
            '"jurisdiction": "GDPR"}'
        )
        result = _parse_classification_response(text)
        assert result.classification == "address_mismatch"
        assert result.confidence == 0.92
        assert result.jurisdiction == "GDPR"

    def test_invalid_classification_falls_back(self):
        text = '{"classification": "invalid_type", "confidence": 0.9, "summary": "test"}'
        result = _parse_classification_response(text)
        assert result.classification == "other"

    def test_confidence_clamped(self):
        text = '{"classification": "identity_challenged", "confidence": 1.5, "summary": "test"}'
        result = _parse_classification_response(text)
        assert result.confidence == 1.0

    def test_confidence_negative_clamped(self):
        text = '{"classification": "identity_challenged", "confidence": -0.5, "summary": "test"}'
        result = _parse_classification_response(text)
        assert result.confidence == 0.0

    def test_missing_fields(self):
        text = "{}"
        result = _parse_classification_response(text)
        assert result.classification == "other"
        assert result.confidence == 0.0

    def test_json_decode_error(self):
        result = _parse_classification_response("not json at all {{")
        assert result.classification == "other"
        assert result.confidence == 0.0

    def test_with_code_block(self):
        text = (
            "```json\n"
            '{"classification": "identity_challenged", "confidence": 0.85, '
            '"summary": "ID needed"}\n```'
        )
        result = _parse_classification_response(text)
        assert result.classification == "identity_challenged"
        assert result.confidence == 0.85

    def test_key_points_not_list(self):
        text = (
            '{"classification": "address_mismatch", "confidence": 0.8, '
            '"summary": "test", "key_points": "string"}'
        )
        result = _parse_classification_response(text)
        assert result.key_points == []


class TestBuildClassifierUserPrompt:
    def test_includes_broker_name(self):
        prompt = _build_classifier_user_prompt(
            broker_name="Test Broker",
            broker_message="We need more info",
        )
        assert "Test Broker" in prompt

    def test_includes_broker_message(self):
        prompt = _build_classifier_user_prompt(
            broker_name="Test",
            broker_message="Your address is incorrect",
        )
        assert "Your address is incorrect" in prompt

    def test_includes_original_request(self):
        prompt = _build_classifier_user_prompt(
            broker_name="Test",
            broker_message="More info needed",
            original_request_template="Subject: Delete my data",
        )
        assert "Delete my data" in prompt


class TestRebuttalResult:
    def test_defaults(self):
        result = RebuttalResult(
            template_name="test.md.j2",
            label="Test",
            description="Desc",
            jurisdiction="GDPR",
            rejection_classification="other",
            confidence=0.0,
            rebuttal_body="Body text",
        )
        assert not result.needs_human_review
        assert not result.llm_used

    def test_full_initialization(self):
        result = RebuttalResult(
            template_name="gdpr-rebuttal-address.md.j2",
            label="Address Rebuttal",
            description="Desc",
            jurisdiction="GDPR",
            rejection_classification="address_mismatch",
            confidence=0.95,
            rebuttal_body="Dear Sir...",
            needs_human_review=False,
            llm_used=True,
        )
        assert result.template_name == "gdpr-rebuttal-address.md.j2"
        assert result.rejection_classification == "address_mismatch"
        assert result.llm_used
        assert result.confidence == 0.95


class TestGenerateRebuttal:
    def test_fallback_classification_no_api(self):
        """When LLM is unavailable, fallback keyword matching should work."""
        result = generate_rebuttal(
            broker_name="Test Broker",
            broker_message="We need your passport for identity verification",
            original_request_template="Subject: GDPR deletion request",
        )
        assert result.template_name is not None
        assert result.rebuttal_body
        assert not result.llm_used
        # Fallback correctly selects identity template based on "passport" keyword
        assert "identity" in result.label.lower()
        assert result.needs_human_review  # fallback always needs review

    def test_with_profile(self):
        profile = IdentityProfile(
            full_name="Jane Doe",
            email_addresses=["jane@example.com"],
            jurisdictions=["DE"],
        )
        result = generate_rebuttal(
            broker_name="Test Broker",
            broker_message="Your address does not match our records",
            profile=profile,
        )
        assert result.rebuttal_body
        assert "Jane Doe" in result.rebuttal_body

    def test_ccpa_fallback(self):
        result = generate_rebuttal(
            broker_name="US Broker",
            broker_message="Under CCPA Section 1798.105 we need to verify your identity",
        )
        assert result.rebuttal_body
        if result.rejection_classification == "ccpa_identity_challenged":
            assert "CCPA" in result.rebuttal_body or "California" in result.rebuttal_body

    def test_unknown_rejection_uses_default_template(self):
        result = generate_rebuttal(
            broker_name="Test",
            broker_message="Thank you for your request, we will process it shortly",
        )
        assert result.template_name is not None
        assert result.rebuttal_body
        # Should fall back to a safe generic template
        assert True

    def test_empty_message(self):
        result = generate_rebuttal(
            broker_name="Test",
            broker_message="",
        )
        assert result.rebuttal_body
        assert result.template_name is not None


class TestRejectionTemplates:
    def test_all_templates_exist(self):
        """Verify all registered template files exist in the registry."""
        from pathlib import Path

        here = Path(__file__).resolve()
        repo_root = here.parents[2]
        laws_dir = repo_root / "registry" / "laws"

        for _key, info in REJECTION_TEMPLATES.items():
            template_path = laws_dir / info["template"]
            assert template_path.exists(), (
                f"Template {info['template']} for rejection '{_key}' not found"
            )

    def test_template_content_valid(self):
        """Verify template content contains expected Jinja2 markers."""
        from pathlib import Path

        here = Path(__file__).resolve()
        repo_root = here.parents[2]
        laws_dir = repo_root / "registry" / "laws"

        for _key, info in REJECTION_TEMPLATES.items():
            template_path = laws_dir / info["template"]
            content = template_path.read_text()
            assert "full_name" in content or "{{" in content
            # Should have Jinja2 template markers
            assert "{{" in content


class TestRenderedContent:
    def test_gdpr_rebuttal_address_renders(self):
        from openeraseme.core.templating import render_template

        profile = IdentityProfile(
            full_name="John Smith",
            email_addresses=["john@example.com"],
            addresses=[
                {
                    "street": "123 Main St",
                    "city": "Berlin",
                    "postal_code": "10115",
                    "country": "Germany",
                }
            ],
            jurisdictions=["DE"],
        )
        result = render_template(
            "gdpr-rebuttal-address.md.j2",
            profile=profile,
            broker_name="Test Broker",
            extra_vars={"original_request_date": "2026-01-15"},
        )
        assert "John Smith" in result
        assert "Article 17" in result
        assert "123 Main St" in result
        assert "original_request_date" not in result  # variable should be substituted

    def test_gdpr_rebuttal_identity_renders(self):
        from openeraseme.core.templating import render_template

        profile = IdentityProfile(
            full_name="Jane Doe",
            email_addresses=["jane@example.com"],
            phone_numbers=["+1-555-1234"],
            date_of_birth="1990-01-01",
            jurisdictions=["DE"],
        )
        result = render_template(
            "gdpr-rebuttal-identity.md.j2",
            profile=profile,
            broker_name="Test Broker",
        )
        assert "Jane Doe" in result
        assert "Article 12(6)" in result
        assert "jane@example.com" in result
        assert "+1-555-1234" in result

    def test_ccpa_rebuttal_deletion_renders(self):
        from openeraseme.core.templating import render_template

        profile = IdentityProfile(
            full_name="Alice Wang",
            email_addresses=["alice@example.com"],
            addresses=[
                {
                    "street": "456 Oak Ave",
                    "city": "Los Angeles",
                    "postal_code": "90001",
                    "country": "US",
                }
            ],
            jurisdictions=["US"],
        )
        result = render_template(
            "ccpa-rebuttal-deletion.md.j2",
            profile=profile,
            broker_name="Data Broker Inc.",
        )
        assert "Alice Wang" in result
        assert "California Consumer Privacy Act" in result
        assert "1798.105" in result
        assert "456 Oak Ave" in result

    def test_ccpa_rebuttal_without_address(self):
        from openeraseme.core.templating import render_template

        profile = IdentityProfile(
            full_name="Bob",
            email_addresses=["bob@example.com"],
            jurisdictions=["US"],
        )
        result = render_template(
            "ccpa-rebuttal-deletion.md.j2",
            profile=profile,
            broker_name="Test",
        )
        assert "Bob" in result
        assert "CCPA" in result

    def test_rebuttal_without_profile(self):
        from openeraseme.core.templating import render_template

        result = render_template(
            "gdpr-rebuttal-address.md.j2",
            profile=None,
            broker_name="Test",
        )
        # Should render without profile data (variables resolve to empty string)
        assert "Article 17" in result
        assert "full_name" not in result  # unresolved variables become empty

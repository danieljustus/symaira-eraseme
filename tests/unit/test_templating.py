import pytest

from openeraseme.registry.schema import IdentityProfile


def _fixture_profile() -> IdentityProfile:
    return IdentityProfile(
        full_name="Jane Doe",
        email_addresses=["jane@example.com"],
        phone_numbers=["+1-555-1234"],
        jurisdictions=["DE", "EU"],
        date_of_birth=None,
    )


class TestTemplatingEngine:
    def test_list_templates(self):
        from openeraseme.core.templating import list_templates

        templates = list_templates()
        assert len(templates) >= 1
        assert all(t.endswith(".md.j2") for t in templates)

    def test_render_gdpr_de(self):
        from openeraseme.core.templating import render_template

        result = render_template(
            "gdpr-art17.de.md.j2",
            profile=_fixture_profile(),
            broker_name="Test Broker GmbH",
        )
        assert "Jane Doe" in result
        assert "Löschungsantrag gemäß Art. 17 DSGVO" in result
        assert "jane@example.com" in result

    def test_render_gdpr_en(self):
        from openeraseme.core.templating import render_template

        result = render_template(
            "gdpr-art17.en.md.j2",
            profile=_fixture_profile(),
            broker_name="Test Broker Inc.",
        )
        assert "Jane Doe" in result
        assert "Article 17 GDPR" in result
        assert "jane@example.com" in result

    def test_render_ccpa_deletion(self):
        from openeraseme.core.templating import render_template

        result = render_template(
            "ccpa-deletion.en.md.j2",
            profile=_fixture_profile(),
            broker_name="Data Broker LLC",
        )
        assert "Jane Doe" in result
        assert "California Consumer Privacy Act" in result
        assert "CCPA" in result

    def test_render_ccpa_opt_out(self):
        from openeraseme.core.templating import render_template

        result = render_template(
            "ccpa-opt-out.en.md.j2",
            profile=_fixture_profile(),
            broker_name="Data Broker LLC",
        )
        assert "Jane Doe" in result
        assert "opt out" in result.lower()
        assert "CCPA" in result

    def test_render_without_profile(self):
        from openeraseme.core.templating import render_template

        result = render_template(
            "gdpr-art17.en.md.j2",
            profile=None,
            broker_name="Test",
        )
        assert "full_name" not in result

    def test_missing_template_raises(self):
        from jinja2 import TemplateNotFound

        from openeraseme.core.templating import render_template

        with pytest.raises(TemplateNotFound):
            render_template("nonexistent.md.j2", profile=_fixture_profile())

    def test_render_with_addresses(self):
        from openeraseme.core.templating import render_template

        profile = IdentityProfile(
            full_name="John Smith",
            email_addresses=["john@example.com"],
            addresses=[
                {
                    "street": "456 Oak Ave",
                    "city": "Los Angeles",
                    "postal_code": "90001",
                    "country": "US",
                }
            ],
        )
        result = render_template(
            "ccpa-deletion.en.md.j2",
            profile=profile,
            broker_name="Example Corp",
        )
        assert "John Smith" in result
        assert "456 Oak Ave" in result
        assert "Los Angeles" in result

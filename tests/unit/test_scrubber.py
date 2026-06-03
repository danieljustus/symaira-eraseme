"""Tests for PII scrubber — US + EU coverage."""

from __future__ import annotations

from symeraseme.adapters.triage.scrubber import scrub_pii


class TestUsPiIScrubbing:
    def test_email_partial_mask(self):
        result = scrub_pii("Contact: jane.doe@example.com")
        assert "jane.doe@example.com" not in result
        assert "@" in result
        assert "j****" in result

    def test_phone_partial_mask(self):
        result = scrub_pii("Call 555-123-4567 anytime")
        assert "555-123-4567" not in result
        assert "***-***-4567" in result

    def test_ssn_full_mask(self):
        result = scrub_pii("SSN: 123-45-6789 on file")
        assert "123-45-6789" not in result
        assert "***-**-****" in result


class TestEuPiIScrubbing:
    def test_iban_masked(self):
        result = scrub_pii("IBAN: DE89370400440532013000 for payment")
        assert "DE89370400440532013000" not in result
        assert "DE" in result
        assert "3000" in result

    def test_de_national_id_masked(self):
        result = scrub_pii("Personalausweis: L12345678 erfasst")
        assert "L12345678" not in result
        assert "*******78" in result

    def test_fr_nir_masked(self):
        result = scrub_pii("NIR: 185017510812342 is the SSN")
        assert "185017510812342" not in result
        assert "***342" in result

    def test_es_dni_masked(self):
        result = scrub_pii("DNI: 12345678Z registrado")
        assert "12345678Z" not in result
        assert "****-****-Z" in result

    def test_passport_masked_with_keyword(self):
        result = scrub_pii("Passport #: AB123456 was issued in Berlin")
        assert "AB123456" not in result
        assert "*" in result

    def test_passport_without_keyword_not_scrubbed(self):
        result = scrub_pii("Tracking code: AB123456 for shipment")
        assert "AB123456" in result

    def test_multiple_eu_ids_in_text(self):
        text = "IDs: DE89370400440532013000, DNIs: 12345678Z, Personalausweis L12345678"
        result = scrub_pii(text)
        assert "DE89370400440532013000" not in result
        assert "12345678Z" not in result
        assert "L12345678" not in result


class TestReDoSPrevention:
    def test_long_dot_string_does_not_hang(self):
        import time

        text = "user@" + "." * 5000
        start = time.perf_counter()
        result = scrub_pii(text)
        elapsed = time.perf_counter() - start
        assert elapsed < 1.0
        assert "user@" in result

    def test_long_hyphen_string_does_not_hang(self):
        import time

        text = "user@" + "a-" * 2000
        start = time.perf_counter()
        scrub_pii(text)
        elapsed = time.perf_counter() - start
        assert elapsed < 1.0

    def test_normal_email_still_scrubbed_after_regex_change(self):
        result = scrub_pii("Reach me at alice.smith@company.co.uk")
        assert "alice.smith@company.co.uk" not in result
        assert "a****" in result
        assert "@" in result

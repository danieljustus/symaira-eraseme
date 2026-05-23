from __future__ import annotations

from typer.testing import CliRunner

from openeraseme.cli import app

runner = CliRunner()


class TestClassifyReplyArgs:
    def test_accepts_provider_openai(self):
        result = runner.invoke(app, ["classify-reply", "1", "--provider", "openai"])
        # Request not found is expected (DB empty), but --provider was accepted
        assert "no such option" not in result.output.lower()
        assert "--provider" not in (result.stderr or "")

    def test_accepts_model_gpt4o(self):
        result = runner.invoke(app, ["classify-reply", "1", "--model", "gpt-4o"])
        assert "no such option" not in result.output.lower()
        assert "--model" not in (result.stderr or "")

    def test_accepts_provider_and_model_together(self):
        result = runner.invoke(
            app, ["classify-reply", "1", "--provider", "openai", "--model", "gpt-4o"]
        )
        assert "no such option" not in result.output.lower()

    def test_rejects_unknown_flag(self):
        result = runner.invoke(app, ["classify-reply", "1", "--api-key", "sk-test"])
        assert result.exit_code != 0
        assert "no such option" in result.output.lower() or "Error" in result.output


class TestGenerateRebuttalArgs:
    def test_accepts_provider_openai(self):
        result = runner.invoke(app, ["generate-rebuttal", "1", "--provider", "openai"])
        # --provider was accepted if error is NOT "no such option"
        assert "no such option" not in result.output.lower()
        assert "--provider" not in (result.stderr or "")

    def test_accepts_model_gpt4o(self):
        result = runner.invoke(app, ["generate-rebuttal", "1", "--model", "gpt-4o"])
        assert "no such option" not in result.output.lower()
        assert "--model" not in (result.stderr or "")

    def test_accepts_provider_and_model_together(self):
        result = runner.invoke(
            app, ["generate-rebuttal", "1", "--provider", "openai", "--model", "gpt-4o"]
        )
        assert "no such option" not in result.output.lower()

    def test_rejects_unknown_flag(self):
        result = runner.invoke(app, ["generate-rebuttal", "1", "--api-key", "sk-test"])
        assert result.exit_code != 0
        assert "no such option" in result.output.lower() or "Error" in result.output

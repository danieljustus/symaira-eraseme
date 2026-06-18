from __future__ import annotations

from pathlib import Path
from unittest.mock import patch

from symeraseme.interactive import (
    collect_matches,
    get_context_markup,
    run_interactive_review,
)


def test_collect_matches_basic():
    content = "Hello, my email is john@example.com and my phone is 123-456-7890."
    matches = collect_matches(content)
    # Should detect email and phone
    assert len(matches) == 2
    names = [m.name for m in matches]
    assert "Email" in names
    assert "Phone" in names


def test_collect_matches_overlaps():
    # Email contains characters that shouldn't double-trigger other things inside it
    content = "test-123-4567@example.com"
    matches = collect_matches(content)
    # The whole string is an email; it shouldn't also match as a separate phone number
    assert len(matches) == 1
    assert matches[0].name == "Email"


def test_get_context_markup():
    content = "Line 1\nLine 2 with target@example.com inside\nLine 3"
    matches = collect_matches(content)
    assert len(matches) == 1
    m = matches[0]

    markup = get_context_markup(content, m, context_lines=1)
    assert "Line 1" in markup
    assert "Line 3" in markup
    assert "-> " in markup
    assert "[bold red]" in markup


@patch("symeraseme.cli.console.console.input")
def test_run_interactive_review_yes(mock_input, tmp_path: Path):
    temp_file = tmp_path / "interactive.txt"
    temp_file.write_text("Secret info: test@example.com here.", encoding="utf-8")

    mock_input.return_value = "y"
    result = run_interactive_review(temp_file)
    assert result is True

    new_content = temp_file.read_text(encoding="utf-8")
    assert "test@example.com" not in new_content
    # The default email scrubber masks part of the email
    assert "@" in new_content


@patch("symeraseme.cli.console.console.input")
def test_run_interactive_review_no(mock_input, tmp_path: Path):
    temp_file = tmp_path / "interactive.txt"
    temp_file.write_text("Secret info: test@example.com here.", encoding="utf-8")

    mock_input.return_value = "n"
    result = run_interactive_review(temp_file)
    assert result is False

    new_content = temp_file.read_text(encoding="utf-8")
    assert "test@example.com" in new_content


@patch("symeraseme.cli.console.console.input")
def test_run_interactive_review_quit(mock_input, tmp_path: Path):
    temp_file = tmp_path / "interactive.txt"
    temp_file.write_text("Secret info: test@example.com here.", encoding="utf-8")

    # First prompt: quit, second prompt: save (no)
    mock_input.side_effect = ["q", "n"]
    result = run_interactive_review(temp_file)
    assert result is False

    new_content = temp_file.read_text(encoding="utf-8")
    assert "test@example.com" in new_content

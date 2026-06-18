from __future__ import annotations

import re
from dataclasses import dataclass
from pathlib import Path
from typing import Callable

from symeraseme.adapters.triage import scrubber
from symeraseme.cli.console import console, print_error, print_info, print_success


@dataclass
class PIIMatch:
    pattern: re.Pattern | None
    name: str
    match: re.Match
    replacer: Callable[[re.Match], str]
    start: int
    end: int
    value: str


PATTERN_NAMES = {
    scrubber._IBAN_PATTERN: "IBAN",
    scrubber._DE_ID_PATTERN: "German ID",
    scrubber._FR_ID_PATTERN: "French ID",
    scrubber._ES_ID_PATTERN: "Spanish ID",
    scrubber._PASSPORT_PATTERN: "Passport",
    scrubber._SSN_PATTERN: "SSN",
    scrubber._EMAIL_PATTERN: "Email",
    scrubber._PHONE_PATTERN: "Phone",
}


def collect_matches(content: str) -> list[PIIMatch]:
    """Find all potential PII matches in content, using profile if available and regexes."""
    from symeraseme.core.identity import load_profile, profile_exists

    matches: list[PIIMatch] = []

    # 1. Profile-based matches
    profile = None
    if profile_exists():
        try:
            profile = load_profile()
        except Exception:
            pass

    if profile is not None:
        # Find all email addresses
        for email in profile.email_addresses:
            if not email:
                continue
            for m in re.finditer(re.escape(email), content, re.IGNORECASE):
                matches.append(
                    PIIMatch(
                        pattern=None,
                        name="Profile Email",
                        match=m,
                        replacer=lambda _: "[REDACTED-EMAIL]",
                        start=m.start(),
                        end=m.end(),
                        value=m.group(0),
                    )
                )
        # Find all phone numbers
        for phone in profile.phone_numbers:
            if not phone:
                continue
            for m in re.finditer(re.escape(phone), content, re.IGNORECASE):
                matches.append(
                    PIIMatch(
                        pattern=None,
                        name="Profile Phone",
                        match=m,
                        replacer=lambda _: "[REDACTED-PHONE]",
                        start=m.start(),
                        end=m.end(),
                        value=m.group(0),
                    )
                )
        # Find all names
        if profile.full_name:
            for m in re.finditer(re.escape(profile.full_name), content, re.IGNORECASE):
                matches.append(
                    PIIMatch(
                        pattern=None,
                        name="Profile Name",
                        match=m,
                        replacer=lambda _: "[REDACTED-NAME]",
                        start=m.start(),
                        end=m.end(),
                        value=m.group(0),
                    )
                )
        # Name variants
        for variant in profile.name_variants:
            if not variant:
                continue
            for m in re.finditer(re.escape(variant), content, re.IGNORECASE):
                matches.append(
                    PIIMatch(
                        pattern=None,
                        name="Profile Name",
                        match=m,
                        replacer=lambda _: "[REDACTED-NAME]",
                        start=m.start(),
                        end=m.end(),
                        value=m.group(0),
                    )
                )
        # Addresses
        for addr in profile.addresses:
            if addr.street:
                for m in re.finditer(re.escape(addr.street), content, re.IGNORECASE):
                    matches.append(
                        PIIMatch(
                            pattern=None,
                            name="Profile Street",
                            match=m,
                            replacer=lambda _: "[REDACTED-STREET]",
                            start=m.start(),
                            end=m.end(),
                            value=m.group(0),
                        )
                    )
            if addr.city:
                for m in re.finditer(re.escape(addr.city), content, re.IGNORECASE):
                    matches.append(
                        PIIMatch(
                            pattern=None,
                            name="Profile City",
                            match=m,
                            replacer=lambda _: "[REDACTED-CITY]",
                            start=m.start(),
                            end=m.end(),
                            value=m.group(0),
                        )
                    )
            if addr.postal_code:
                for m in re.finditer(re.escape(addr.postal_code), content, re.IGNORECASE):
                    matches.append(
                        PIIMatch(
                            pattern=None,
                            name="Profile Postal Code",
                            match=m,
                            replacer=lambda _: "[REDACTED-POSTAL]",
                            start=m.start(),
                            end=m.end(),
                            value=m.group(0),
                        )
                    )

    # 2. General regex-based scrubber matches
    for pattern, replacer in scrubber._SCRUBBERS:
        name = PATTERN_NAMES.get(pattern, "PII Pattern")
        for m in pattern.finditer(content):
            # Capture the replacer closure
            def make_repl(rep, match_obj):
                return lambda _: rep(match_obj)

            matches.append(
                PIIMatch(
                    pattern=pattern,
                    name=name,
                    match=m,
                    replacer=make_repl(replacer, m),
                    start=m.start(),
                    end=m.end(),
                    value=m.group(0),
                )
            )

    # 3. Sort matches and resolve overlaps
    matches.sort(key=lambda x: (x.start, -len(x.value)))
    filtered_matches: list[PIIMatch] = []
    last_end = -1
    for m in matches:
        if m.start >= last_end:
            filtered_matches.append(m)
            last_end = m.end

    return filtered_matches


def get_context_markup(content: str, m: PIIMatch, context_lines: int = 2) -> str:
    lines = content.split("\n")
    char_count = 0
    match_line_idx = 0
    for idx, line in enumerate(lines):
        line_len = len(line) + 1  # count the newline character
        if char_count <= m.start < char_count + line_len:
            match_line_idx = idx
            break
        char_count += line_len

    start_idx = max(0, match_line_idx - context_lines)
    end_idx = min(len(lines), match_line_idx + context_lines + 1)

    start_offset = sum(len(lines[i]) + 1 for i in range(start_idx))
    end_offset = sum(len(lines[i]) + 1 for i in range(end_idx))

    sub_content = content[start_offset:end_offset]

    # Local offsets in sub_content
    local_start = m.start - start_offset
    local_end = m.end - start_offset

    # Build the formatted string
    before = sub_content[:local_start]
    match_text = sub_content[local_start:local_end]
    after = sub_content[local_end:]

    from rich.markup import escape
    formatted = f"{escape(before)}[bold red]{escape(match_text)}[/bold red]{escape(after)}"

    result_lines = []
    for offset_idx, line in enumerate(formatted.split("\n")):
        current_line_no = start_idx + offset_idx + 1
        if current_line_no > len(lines):
            break
        is_match_line = (start_idx + offset_idx == match_line_idx)
        prefix = "-> " if is_match_line else "   "
        result_lines.append(f"{prefix}{current_line_no:4d} | {line}")

    return "\n".join(result_lines)


def run_interactive_review(file_path: Path) -> bool:
    """Run interactive PII review on a file. Returns True if saved, False if aborted/no-op."""
    if not file_path.exists():
        print_error(f"File not found: {file_path}")
        return False

    try:
        content = file_path.read_text(encoding="utf-8")
    except Exception as e:
        print_error(f"Cannot read file {file_path}: {e}")
        return False

    matches = collect_matches(content)
    if not matches:
        print_success(f"No PII matches detected in {file_path}.")
        return False

    print_info(f"Found {len(matches)} potential PII matches in {file_path}.")
    console.print("[dim]Use y: redact, n/s: keep/skip, q: quit and save changes made so far[/dim]\n")

    new_content_chunks: list[str] = []
    current_pos = 0
    saved_any = False
    quit_review = False

    for idx, m in enumerate(matches):
        if quit_review:
            break

        # Show match context
        context_markup = get_context_markup(content, m)
        console.print(f"\n[bold yellow]Match {idx + 1} of {len(matches)} - Type: {m.name}[/bold yellow]")
        console.print(context_markup)

        # Prompt
        while True:
            response = console.input(
                f"Redact [bold cyan]'{m.value}'[/bold cyan]? (y/n/s/q) "
            ).strip().lower()
            if response in ("y", "yes"):
                # Append text before match
                new_content_chunks.append(content[current_pos:m.start])
                # Append redacted version
                redacted_value = m.replacer(m.match)
                new_content_chunks.append(redacted_value)
                current_pos = m.end
                saved_any = True
                print_success(f"Redacted to: '{redacted_value}'")
                break
            elif response in ("n", "no", "s", "skip"):
                # Keep original (will be copied with current_pos advance at the end)
                break
            elif response in ("q", "quit"):
                quit_review = True
                break
            else:
                console.print("[red]Invalid input. Choose y (yes), n (no), s (skip), or q (quit).[/red]")

    # Append any remaining content from current_pos to the end
    new_content_chunks.append(content[current_pos:])
    final_content = "".join(new_content_chunks)

    if quit_review:
        if saved_any:
            response = console.input(
                "Save changes made so far? (y/n) "
            ).strip().lower()
            if response not in ("y", "yes"):
                print_info("Review aborted. Changes discarded.")
                return False
        else:
            print_info("Review aborted. No changes made.")
            return False

    if final_content != content:
        try:
            file_path.write_text(final_content, encoding="utf-8")
            print_success(f"Successfully saved redacted content to {file_path}.")
            return True
        except Exception as e:
            print_error(f"Failed to write changes to {file_path}: {e}")
            return False
    else:
        print_info("No changes were made to the file.")
        return False

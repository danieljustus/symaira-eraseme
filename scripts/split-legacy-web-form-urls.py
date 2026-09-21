#!/usr/bin/env python3
"""Split combined legacy `web_form` urls into a clean URL plus an annotation.

Eight registry entries store a real form URL and an annotation in the same
string, separated by ` / `, ` - ` or just whitespace, and they repeat the whole
string as the first `form_spec` navigation step:

    url: https://cuebiq.com/privacy-center/ / privacy@cuebiq.com
    url: https://spydialer.com - Remove My Info link in footer
    url: https://tex warrant roundup

These are genuine `web_form` channels, so unlike the 46 misclassified email
channels (see `normalize-legacy-web-forms.py`) they keep their type. The URL is
reduced to its first absolute http(s) token, the rest is preserved as `notes`,
and `form_spec`'s navigation step is repointed at the same clean URL.

`texaswarrantroundup-us` carries no URL at all — only the annotation "tex warrant
roundup" — so it falls back to the broker's own `website`, which is the only
defensible destination the file names.

An entry is rewritten only when: it is a `web_form` whose `url` is not already a
clean absolute http(s) URI, and whose `form_spec` contains exactly one `goto`
holding that identical string. Anything else is refused and reported.

Usage:
    python3 scripts/split-legacy-web-form-urls.py --check
    python3 scripts/split-legacy-web-form-urls.py --write
"""

from __future__ import annotations

import argparse
import re
import sys
from pathlib import Path

BROKER_ROOT = Path(__file__).resolve().parent.parent / "registry" / "brokers"
CLEAN_URI = re.compile(r"^https?://[^\s/?#]+(?:\S*)?$")


class Refused(Exception):
    """An entry that looks close to the target shape but is not exactly it."""


# A usable destination needs a dotted host, so `https://tex` from a mangled
# value like `https://tex warrant roundup` is not mistaken for one.
HOST = re.compile(r"^https?://[a-z0-9-]+(?:\.[a-z0-9-]+)+", re.I)


def clean_uri(value: str) -> str | None:
    """The first whitespace-delimited token that is itself an absolute http(s) URL."""
    for token in value.split():
        if HOST.match(token):
            return token
    return None


def annotation(value: str, url: str) -> str:
    """Whatever the original string held besides the URL, tidied up."""
    return value.replace(url, " ", 1).strip(" \t/-(–—)").strip()


def top_field(text: str, name: str) -> str | None:
    match = re.search(rf"^{name}:\s*(.+)$", text, re.M)
    return match.group(1).strip().strip("\"'") if match else None


def scalar_lines(lines: list[str], index: int) -> tuple[str, int]:
    """Return (joined value, last line index) for a YAML scalar starting at `index`.

    A wrapped scalar continues on lines indented deeper than its own key; a
    sibling key at the same indent ends it.
    """
    indent = len(lines[index]) - len(lines[index].lstrip())
    parts = [lines[index].split(":", 1)[1].strip()]
    last = index
    for offset, line in enumerate(lines[index + 1 :], start=index + 1):
        if line.strip() == "" or len(line) - len(line.lstrip()) <= indent:
            break
        parts.append(line.strip())
        last = offset
    return " ".join(part for part in parts if part), last


def key_index(lines: list[str], pattern: str) -> int | None:
    return next((i for i, line in enumerate(lines) if re.match(pattern, line)), None)


def process(path: Path) -> tuple[str, str] | None:
    """Return (rewritten text, note) or None when the file needs no change."""
    text = path.read_text(encoding="utf-8")
    if "opt_out:" not in text:
        return None
    head, rest = text.split("opt_out:", 1)
    for marker in ("\nverification:", "\nnotes:", "\nadded_date:"):
        if marker in rest:
            block, tail = rest.split(marker, 1)
            tail = marker + tail
            break
    else:
        raise Refused("opt_out block has no following top-level key")

    # ── locate the single channel and its url / goto values ────────────────
    lines = block.split("\n")
    if len([l for l in lines if re.match(r"^\s*-?\s*type:\s*\S", l)]) != 1:
        return None
    if not any(re.match(r"^\s*-?\s*type:\s*web_form\s*$", l) for l in lines):
        return None

    url_index = key_index(lines, r"^\s+url:\s*\S")
    if url_index is None:
        return None
    raw, url_last = scalar_lines(lines, url_index)
    raw = raw.strip("\"'")
    if re.match(r"^https?://\S+$", raw):
        return None  # already clean

    goto_index = key_index(lines, r"^\s*-\s*goto:\s*\S")
    if goto_index is None:
        raise Refused("web_form without a form_spec goto")
    goto, goto_last = scalar_lines(lines, goto_index)
    goto = goto.strip("\"'")
    if goto != raw:
        raise Refused(f"form_spec navigates to {goto!r}, not exactly {raw!r}")

    # ── decide the destination and the annotation ──────────────────────────
    uri = clean_uri(raw)
    if uri is None:
        # No usable URL in the value at all: fall back to the broker's website,
        # which is the only defensible destination the file names. The original
        # value is then the annotation in full.
        uri = top_field(text, "website") or ""
        if not HOST.match(uri):
            raise Refused(f"no usable URL for {raw!r} and no website to fall back on")
        note = raw.strip()
    else:
        note = annotation(raw, uri)
        if not note:
            raise Refused(f"{raw!r} carries no annotation to preserve")

    # ── rewrite, replacing the whole (possibly wrapped) scalar ─────────────
    out: list[str] = []
    for i, line in enumerate(lines):
        if i == url_index:
            out.append(re.sub(r"(\s+url:\s*).*$", rf"\g<1>{uri}", line))
            continue
        if i == goto_index:
            out.append(re.sub(r"(\s*-\s*goto:\s*).*$", rf"\g<1>{uri}", line))
            continue
        if url_index < i <= url_last or goto_index < i <= goto_last:
            continue  # wrapped continuation lines are folded into the new value
        out.append(line)
    block = "\n".join(out)

    if "\nnotes:" in tail:
        # Append rather than clobber: an existing note was written deliberately.
        existing = top_field(tail, "notes")
        combined = f"{existing}; {note}" if existing else note
        tail = re.sub(
            r"^(\nnotes:\s*).*$",
            lambda m: m.group(1) + combined,
            tail,
            count=1,
            flags=re.M,
        )
    else:
        tail = f"\nnotes: {note}" + tail
    return head + "opt_out:" + block + tail, note


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    group = parser.add_mutually_exclusive_group(required=True)
    group.add_argument("--check", action="store_true")
    group.add_argument("--write", action="store_true")
    args = parser.parse_args()

    files = sorted(f for f in BROKER_ROOT.rglob("*") if f.is_file())
    changed: list[tuple[Path, str]] = []
    refused: list[tuple[Path, str]] = []
    for path in files:
        try:
            result = process(path)
        except Refused as refusal:
            refused.append((path, str(refusal)))
            continue
        if result is None:
            continue
        rewritten, note = result
        changed.append((path, note))
        if args.write:
            path.write_text(rewritten, encoding="utf-8")

    for path, reason in refused:
        print(f"REFUSED {path.relative_to(BROKER_ROOT)}: {reason}", file=sys.stderr)
    print(f"scanned {len(files)} files")
    print(f"{'rewrote' if args.write else 'would rewrite'} {len(changed)} files")
    for path, note in changed:
        print(f"  {path.relative_to(BROKER_ROOT)}: note={note!r}")
    if refused:
        print(f"refused {len(refused)} files", file=sys.stderr)
    return 1 if refused else 0


if __name__ == "__main__":
    raise SystemExit(main())
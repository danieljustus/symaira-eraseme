#!/usr/bin/env python3
"""Rewrite legacy non-URI `web_form` channels into `email` channels.

`registry/brokers/**` contains 46 entries whose `web_form` channel carries an
email address in `url` and repeats it as the first `form_spec` navigation step.
The broker declares no other channel, so the entry is an email channel that was
recorded under the wrong type: `web_form` requires `form_spec` and forbids
`endpoint`, while `email` requires `endpoint` and forbids `url`/`form_spec`.

This rewrites exactly those entries, and only those:

* `type: web_form` -> `type: email`
* the email address moves from `url` into `endpoint`
* `form_spec` is dropped (it held nothing but the address as a `goto`)
* every other key, comment, ordering and byte of the file is preserved

Nothing is guessed. An entry is rewritten only when it is a `web_form` whose
`url` is a valid address, whose `form_spec` contains exactly one `goto` holding
that same address, and whose broker declares no other channel. Anything else is
refused and reported, so a surprise cannot be silently mangled.

Usage:
    python3 scripts/normalize-legacy-web-forms.py --check   # report only
    python3 scripts/normalize-legacy-web-forms.py --write   # rewrite in place
"""

from __future__ import annotations

import argparse
import re
import sys
from pathlib import Path

BROKER_ROOT = Path(__file__).resolve().parent.parent / "registry" / "brokers"
ADDRESS = re.compile(r"^[^@\s]+@[^@\s]+\.[^@\s]+$")
URI = re.compile(r"^https?://\S+$")


class Refused(Exception):
    """An entry that looks close to the target shape but is not exactly it."""


def split_channels(text: str) -> tuple[str, str, str]:
    """Return (head, opt_out_block, tail) around the `opt_out:` sequence."""
    if "opt_out:" not in text:
        raise Refused("no opt_out block")
    head, rest = text.split("opt_out:", 1)
    for marker in ("\nverification:", "\nnotes:", "\nadded_date:"):
        if marker in rest:
            block, tail = rest.split(marker, 1)
            return head, block, marker + tail
    raise Refused("opt_out block has no following top-level key")


def rewrite_block(block: str) -> str:
    """Rewrite a single-channel `web_form` block into an `email` block."""
    lines = block.split("\n")
    url_index = next(
        (i for i, line in enumerate(lines) if re.match(r"^\s+url:\s*\S", line)), None
    )
    if url_index is None:
        raise Refused("no url field")
    address = lines[url_index].split("url:", 1)[1].strip().strip("\"'")
    if not ADDRESS.match(address):
        raise Refused(f"url {address!r} is not an address")

    form_index = next(
        (i for i, line in enumerate(lines) if re.match(r"^\s+form_spec:\s*$", line)), None
    )
    if form_index is None:
        raise Refused("web_form without form_spec")

    # The form_spec must be exactly the address as its only navigation step.
    gotos = [
        re.match(r"^\s*-\s*goto:\s*(\S+)", line).group(1)
        for line in lines[form_index + 1 :]
        if re.match(r"^\s*-\s*goto:\s*\S", line)
    ]
    if gotos != [address]:
        raise Refused(f"form_spec navigates to {gotos!r}, not exactly [{address!r}]")

    # Drop the form_spec subtree, then repoint the channel at `email`.
    end = form_index + 1
    while end < len(lines) and (
        lines[end].strip() == "" or lines[end].startswith(("    ", "\t"))
    ):
        end += 1
    rebuilt = lines[:form_index] + lines[end:]
    url_index = next(
        i for i, line in enumerate(rebuilt) if re.match(r"^\s+url:\s*\S", line)
    )
    rebuilt[url_index] = re.sub(r"url:", "endpoint:", rebuilt[url_index], count=1)
    rebuilt = [
        re.sub(r"^(\s*-?\s*)type:\s*web_form\s*$", r"\1type: email", line)
        for line in rebuilt
    ]
    if not any("type: email" in line for line in rebuilt):
        raise Refused("channel type was not rewritten")
    return "\n".join(rebuilt)


def process(path: Path) -> str | None:
    """Return the rewritten text, or None when the file needs no change."""
    text = path.read_text(encoding="utf-8")
    head, block, tail = split_channels(text)
    # More than one channel means this is not the single-channel shape handled
    # here; leave it alone rather than reordering a multi-channel list.
    if block.count("- type:") != 1:
        return None
    if not re.search(r"^\s*-?\s*type:\s*web_form\s*$", block, re.M):
        return None
    if not re.search(r"^\s+url:\s*(\S+)\s*$", block, re.M):
        return None
    url = re.search(r"^\s+url:\s*(\S+)\s*$", block, re.M).group(1).strip("\"'")
    if URI.match(url):
        return None  # already a proper web form
    return head + "opt_out:" + rewrite_block(block) + tail


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    group = parser.add_mutually_exclusive_group(required=True)
    group.add_argument("--check", action="store_true", help="report without writing")
    group.add_argument("--write", action="store_true", help="rewrite in place")
    args = parser.parse_args()

    files = sorted(f for f in BROKER_ROOT.rglob("*") if f.is_file())
    changed: list[Path] = []
    refused: list[tuple[Path, str]] = []
    for path in files:
        try:
            rewritten = process(path)
        except Refused as refusal:
            refused.append((path, str(refusal)))
            continue
        if rewritten is None:
            continue
        changed.append(path)
        if args.write:
            path.write_text(rewritten, encoding="utf-8")

    for path, reason in refused:
        print(f"REFUSED {path.relative_to(BROKER_ROOT)}: {reason}", file=sys.stderr)
    print(f"scanned {len(files)} files")
    print(f"{'rewrote' if args.write else 'would rewrite'} {len(changed)} files")
    for path in changed[:5]:
        print(f"  {path.relative_to(BROKER_ROOT)}")
    if len(changed) > 5:
        print(f"  ... and {len(changed) - 5} more")
    if refused:
        print(f"refused {len(refused)} files", file=sys.stderr)
    # A refusal is a surprise that must not be papered over.
    return 1 if refused else 0


if __name__ == "__main__":
    raise SystemExit(main())
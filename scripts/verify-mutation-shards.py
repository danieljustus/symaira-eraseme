#!/usr/bin/env python3
"""Require a complete, disjoint mutation campaign across every declared shard."""
import collections
import importlib.util
import json
import pathlib
import sys

spec = importlib.util.spec_from_file_location("outcomes", pathlib.Path(__file__).with_name("verify-mutation-outcomes.py"))
outcomes = importlib.util.module_from_spec(spec)
spec.loader.exec_module(outcomes)


def key(mutant):
    return json.dumps({field: mutant[field] for field in
                       ("package", "file", "span", "replacement", "genre")}, sort_keys=True)


def verify_group(documents, scope, denominator, head):
    assert len(documents) == denominator, f"missing {scope} shard"
    selected_union = set()
    catalog = None
    seen_indices = set()
    totals = collections.Counter()
    for receipt, full, selected, actual in documents:
        assert receipt["scope"] == scope and receipt["head"] == head, "mixed source/scope"
        assert receipt["target"] == "x86_64-unknown-linux-gnu", "unexpected mutation target"
        expected_inactive = [
            r'http\.rs:190:5: replace addr_not_available_message .* with (""|"xyzzy")$',
            r'http\.rs:239:5: replace write_token_file .* with Ok\(\(\)\)$',
            r'http\.rs:256:5: replace create_token_directory .* with Ok\(\(\)\)$',
        ] if scope == "HTTP auth" else []
        if scope == "consent":
            expected_inactive = [
                r'consent\.rs:628:5: replace close_windows_file .* with Ok\(\(\)\)$',
                r'consent\.rs:640:5: replace checked_windows_close .* with Ok\(\(\)\)$',
                r'consent\.rs:644:39: replace != with == in checked_windows_close$',
            ]
        assert receipt["inactive_target_exclusions"] == expected_inactive, "unexpected platform exclusions"
        index, total = map(int, receipt["shard"].split("/"))
        assert total == denominator and 0 <= index < total, "wrong shard denominator"
        assert index not in seen_indices, "duplicate shard"
        seen_indices.add(index)
        full_keys = {key(item) for item in full}
        assert len(full_keys) == len(full) > 0, "empty/duplicate full catalog"
        if catalog is None:
            catalog = full_keys
        assert full_keys == catalog, "inconsistent full catalog"
        selected_keys = {key(item) for item in selected}
        assert len(selected_keys) == len(selected) > 0, "empty/duplicate selected catalog"
        assert selected_keys <= catalog, "unknown selected mutant"
        assert not selected_union & selected_keys, "overlapping shards"
        selected_union |= selected_keys
        summary = outcomes.verify(actual)
        actual_keys = {key(item["scenario"]["Mutant"]) for item in actual["outcomes"]
                       if isinstance(item["scenario"], dict)}
        assert actual_keys == selected_keys, "selected mutants were not all executed"
        totals.update(summary)
    assert seen_indices == set(range(denominator)), "incomplete shard indices"
    assert selected_union == catalog, "mutants omitted from campaign"
    assert totals["total_mutants"] == len(catalog), "wrong campaign total"
    return dict(totals)


def main():
    root, head = pathlib.Path(sys.argv[1]), sys.argv[2]
    grouped = collections.defaultdict(list)
    for receipt_path in root.glob("mutation-sweep-*/mutation-inputs/receipt.json"):
        inputs = receipt_path.parent
        receipt = json.loads(receipt_path.read_text())
        actual_paths = list(inputs.parent.rglob("outcomes.json"))
        assert len(actual_paths) == 1, "missing/ambiguous actual outcomes"
        grouped[receipt["scope"]].append((receipt,
            json.loads((inputs / "full-catalog.json").read_text()),
            json.loads((inputs / "selected-catalog.json").read_text()),
            json.loads(actual_paths[0].read_text())))
    expected = {"crypto": 4, "consent": 1, "state transitions": 1, "sanitization": 4, "HTTP auth": 1}
    assert set(grouped) == set(expected), "missing/unknown mutation category"
    print(json.dumps({scope: verify_group(grouped[scope], scope, count, head)
                      for scope, count in expected.items()}, sort_keys=True))


if __name__ == "__main__":
    main()

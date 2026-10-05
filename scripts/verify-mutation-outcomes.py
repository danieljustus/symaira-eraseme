#!/usr/bin/env python3
"""Require actual caught mutants and a successful baseline, even on exit 3.

In cargo-mutants 27.1.0, timeout exit 3 takes precedence over missed exit 2.
The machine-readable outcomes therefore remain authoritative.
"""
import argparse
import collections
import json
import pathlib


def verify(document):
    assert document["cargo_mutants_version"] == "27.1.0", "unexpected producer"
    assert document["end_time"], "incomplete mutation run"
    outcomes = document["outcomes"]
    baselines = [item for item in outcomes if item["scenario"] == "Baseline"]
    mutants = [item for item in outcomes
               if isinstance(item["scenario"], dict) and set(item["scenario"]) == {"Mutant"}]
    assert len(baselines) == 1 and baselines[0]["summary"] == "Success", "baseline must pass"
    assert len(outcomes) == len(baselines) + len(mutants), "unknown scenario"
    counts = collections.Counter(item["summary"] for item in mutants)
    assert document["total_mutants"] == len(mutants) > 0, "no actual mutants"
    for field, summary in (("caught", "CaughtMutant"), ("missed", "MissedMutant"),
                           ("timeout", "Timeout"), ("unviable", "Unviable"),
                           ("success", "Success")):
        assert document[field] == counts[summary], f"inconsistent {field} total"
    assert counts["CaughtMutant"] > 0, "no mutant detected by a failing test"
    assert counts["MissedMutant"] == 0, "missed mutants remain"
    assert set(counts) <= {"CaughtMutant", "Timeout", "Unviable"}, "unclassified outcome"
    return {field: document[field] for field in
            ("total_mutants", "caught", "missed", "timeout", "unviable")}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("outcomes", type=pathlib.Path)
    args = parser.parse_args()
    print(json.dumps(verify(json.loads(args.outcomes.read_text())), sort_keys=True))


if __name__ == "__main__":
    main()

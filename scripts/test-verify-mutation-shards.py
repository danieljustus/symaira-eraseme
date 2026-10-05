#!/usr/bin/env python3
"""Regression controls for missing, overlapping and incomplete campaigns."""
import copy
import importlib.util
import pathlib
import unittest

spec = importlib.util.spec_from_file_location("shards", pathlib.Path(__file__).with_name("verify-mutation-shards.py"))
shards = importlib.util.module_from_spec(spec)
spec.loader.exec_module(shards)


def campaign():
    full = [{"package": "core", "file": "example.rs", "span": {"start": {"line": i + 1, "column": 1}, "end": {"line": i + 1, "column": 2}}, "replacement": "false", "genre": "FnValue"} for i in range(2)]
    documents = []
    for i in range(2):
        actual = {"cargo_mutants_version": "27.1.0", "end_time": "completed",
                  "total_mutants": 1, "caught": 1, "missed": 0, "timeout": 0, "unviable": 0, "success": 0,
                  "outcomes": [{"scenario": "Baseline", "summary": "Success"},
                               {"scenario": {"Mutant": full[i]}, "summary": "CaughtMutant"}]}
        documents.append(({"scope": "crypto", "shard": f"{i}/2", "head": "source",
                           "target": "x86_64-unknown-linux-gnu", "inactive_target_exclusions": []}, full, [full[i]], actual))
    return documents


class Controls(unittest.TestCase):
    def test_complete_disjoint_campaign_passes(self):
        self.assertEqual(shards.verify_group(campaign(), "crypto", 2, "source")["total_mutants"], 2)

    def test_missing_or_duplicate_shards_fail(self):
        for documents in [campaign()[:1], [campaign()[0], campaign()[0]]]:
            with self.assertRaises(AssertionError):
                shards.verify_group(documents, "crypto", 2, "source")

    def test_selected_but_unexecuted_mutant_fails(self):
        documents = copy.deepcopy(campaign())
        documents[1][3]["outcomes"][1]["scenario"]["Mutant"] = documents[0][2][0]
        with self.assertRaises(AssertionError):
            shards.verify_group(documents, "crypto", 2, "source")

    def test_catalog_omission_and_overlap_fail(self):
        documents = copy.deepcopy(campaign())
        documents[1][2][0] = documents[0][2][0]
        with self.assertRaises(AssertionError):
            shards.verify_group(documents, "crypto", 2, "source")
        documents = copy.deepcopy(campaign())
        extra = dict(documents[0][1][0], replacement="true")
        documents[0][1].append(extra)
        with self.assertRaises(AssertionError):
            shards.verify_group(documents, "crypto", 2, "source")

    def test_mixed_sources_and_incomplete_results_fail(self):
        documents = copy.deepcopy(campaign())
        documents[1][0]["head"] = "other"
        with self.assertRaises(AssertionError):
            shards.verify_group(documents, "crypto", 2, "source")
        documents = copy.deepcopy(campaign())
        documents[1][3]["end_time"] = None
        with self.assertRaises(AssertionError):
            shards.verify_group(documents, "crypto", 2, "source")

    def test_unexpected_target_or_extra_exclusion_fails(self):
        for field, value in [("target", "aarch64-apple-darwin"),
                             ("inactive_target_exclusions", [".*"])]:
            documents = copy.deepcopy(campaign())
            documents[0][0][field] = value
            with self.assertRaises(AssertionError):
                shards.verify_group(documents, "crypto", 2, "source")


if __name__ == "__main__":
    unittest.main()

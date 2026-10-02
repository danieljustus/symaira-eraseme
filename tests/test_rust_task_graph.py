#!/usr/bin/env python3
"""Read-only reconciliation of migration tasks and unfinished matrix rows."""
import copy
import json
import re
import unittest
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
GRAPH = ROOT / "docs/plans/2026-09-04-go-to-rust-task-graph.json"
MATRIX = ROOT / "docs/rust-port-contract-matrix.md"


class TaskGraphTests(unittest.TestCase):
    def test_task_and_contract_ownership_is_complete(self):
        graph = json.loads(GRAPH.read_text())
        rows = {}
        for line in MATRIX.read_text().splitlines():
            fields = line.split("|")
            if len(fields) == 10 and re.fullmatch(r"[A-Z]+-\d+[A-Z]?", fields[1].strip()):
                rows[fields[1].strip()] = fields[-2].strip()
        self.assertTrue(rows)
        unfinished = {row for row, status in rows.items()
                      if status.startswith(("PARTIAL", "TODO"))}
        self.assertTrue(unfinished)

        def validate(candidate):
            tasks = {task["id"] for task in candidate["tasks"]}
            state = candidate["execution_state"]
            merged = set(state["merged_tasks"])
            self.assertEqual(len(merged), len(state["merged_tasks"]))
            self.assertTrue(merged <= tasks)
            self.assertIn(state["last_completed"], merged)
            self.assertEqual(set(state["task_issues"]), tasks - merged)
            self.assertTrue(set(state["active_tasks"]) <= tasks - merged)
            owners = state["contract_issue_owners"]
            self.assertTrue(unfinished <= owners.keys())
            self.assertTrue(owners.keys() <= rows.keys())
            for issues in state["task_issues"].values():
                self.assertTrue(issues)
                self.assertTrue(all(type(issue) is int and issue > 0 for issue in issues))
            self.assertTrue(all(type(issue) is int and issue > 0 for issue in owners.values()))
            self.assertNotIn("browser auto-click", " ".join(state["blocked_by"]))
            self.assertEqual(candidate["external_gates"]["signed-dmg-rc"]["issue"], 1129)

        validate(graph)
        for field in ("task_issues", "contract_issue_owners"):
            with self.subTest(missing_owner=field):
                corrupt = copy.deepcopy(graph)
                corrupt["execution_state"][field].pop(next(iter(corrupt["execution_state"][field])))
                with self.assertRaises(AssertionError):
                    validate(corrupt)


if __name__ == "__main__":
    unittest.main()

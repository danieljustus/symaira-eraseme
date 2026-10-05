"""Regression controls for cargo-mutants' mixed timeout/missed exit status."""
import copy
import importlib.util
import pathlib
import unittest

spec = importlib.util.spec_from_file_location(
    "mutation_gate", pathlib.Path(__file__).with_name("verify-mutation-outcomes.py"))
gate = importlib.util.module_from_spec(spec)
spec.loader.exec_module(gate)


class OutcomeGateTests(unittest.TestCase):
    def document(self):
        return {"cargo_mutants_version": "27.1.0", "end_time": "2026-10-04T00:00:00Z",
                "total_mutants": 3, "caught": 1, "missed": 0, "timeout": 1,
                "unviable": 1, "success": 0,
                "outcomes": [{"scenario": "Baseline", "summary": "Success"},
                             *({"scenario": {"Mutant": {}}, "summary": summary}
                               for summary in ("CaughtMutant", "Timeout", "Unviable"))]}

    def test_actual_caught_with_timeout_is_accepted(self):
        self.assertEqual(gate.verify(self.document())["missed"], 0)

    def test_timeout_does_not_mask_a_missed_mutant(self):
        document = self.document()
        document["outcomes"].append({"scenario": {"Mutant": {}}, "summary": "MissedMutant"})
        document.update(total_mutants=4, missed=1)
        with self.assertRaisesRegex(AssertionError, "missed mutants"):
            gate.verify(document)

    def test_incomplete_zero_case_and_failed_baseline_are_rejected(self):
        for change in ({"end_time": None}, {"total_mutants": 0}, {"caught": 0}):
            document = self.document()
            document.update(change)
            with self.assertRaises(AssertionError):
                gate.verify(document)
        document = self.document()
        document["outcomes"][0]["summary"] = "Failure"
        with self.assertRaisesRegex(AssertionError, "baseline"):
            gate.verify(document)

    def test_summary_cannot_disagree_with_actual_outcomes(self):
        document = copy.deepcopy(self.document())
        document["missed"] = 1
        with self.assertRaisesRegex(AssertionError, "inconsistent missed"):
            gate.verify(document)


if __name__ == "__main__":
    unittest.main()

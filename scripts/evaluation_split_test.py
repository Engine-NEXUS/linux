#!/usr/bin/env python3

import importlib.util
import json
import unittest
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
MODULE_PATH = ROOT / "server" / "nlu" / "prepare_evaluation_splits.py"
spec = importlib.util.spec_from_file_location("prepare_evaluation_splits", MODULE_PATH)
splits = importlib.util.module_from_spec(spec)
spec.loader.exec_module(splits)


class EvaluationSplitTests(unittest.TestCase):
    def test_slot_values_share_a_phrase_family(self):
        first = {"text": "merge pull request 12 in owner/repo", "intent": "merge_pr", "slots": {"pr_number": "12", "repo": "owner/repo"}}
        second = {"text": "merge pull request 99 in nexus/app", "intent": "merge_pr", "slots": {"pr_number": "99", "repo": "nexus/app"}}
        self.assertEqual(splits.family_key(first), splits.family_key(second))

    def test_different_command_templates_do_not_share_a_family(self):
        first = {"text": "merge pull request 12 in owner/repo", "intent": "merge_pr", "slots": {"pr_number": "12", "repo": "owner/repo"}}
        second = {"text": "combine PR 12 for owner/repo", "intent": "merge_pr", "slots": {"pr_number": "12", "repo": "owner/repo"}}
        self.assertNotEqual(splits.family_key(first), splits.family_key(second))

    def test_overlap_validator_rejects_phrase_family_leakage(self):
        train = {"text": "open chrome", "intent": "open_app", "slots": {"app_name": "chrome"}}
        test = {"text": "open firefox", "intent": "open_app", "slots": {"app_name": "firefox"}}
        dataset = {"train": [train], "validation": [], "calibration": [], "test": [test]}
        errors = splits.validate_splits(dataset)
        self.assertTrue(any("phrase-family overlap" in error for error in errors))

    def test_current_dataset_has_complete_locked_splits_without_mutation(self):
        dataset_path = ROOT / "server" / "nlu" / "dataset.json"
        before = dataset_path.read_bytes()
        dataset = json.loads(before)
        current = {**{name: dataset[name] for name in splits.SPLIT_NAMES}, "quarantine": dataset["quarantine"]}
        errors = splits.validate_splits(current)
        actual_lock = splits.build_lock(current)
        expected_lock = json.loads((ROOT / "server" / "nlu" / "data" / "split_lock.json").read_text())
        after = dataset_path.read_bytes()
        self.assertEqual(before, after)
        self.assertEqual(errors, [])
        self.assertEqual(actual_lock, expected_lock)
        self.assertTrue(all(actual_lock["splits"][name]["intents"] == 52 for name in splits.SPLIT_NAMES))

    def test_training_does_not_split_or_select_on_final_test(self):
        source = (ROOT / "server" / "nlu" / "train.py").read_text(encoding="utf-8")
        self.assertNotIn("random.shuffle(test_data)", source)
        self.assertIn("train_data, val_data, calibration_data, test_data = load_dataset()", source)
        self.assertIn("if val_acc > best_validation_acc", source)
        self.assertNotIn("if final_test_acc >", source)


if __name__ == "__main__":
    unittest.main()

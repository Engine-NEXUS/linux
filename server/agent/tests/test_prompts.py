"""Tests for extracted agent prompts (verbatim constants)."""
import unittest

from agent import prompts


class TestPrompts(unittest.TestCase):
    def test_all_roles_present_and_nonempty(self):
        for name in ("PLANNER_SYSTEM", "EXECUTOR_SYSTEM", "CRITIC_SYSTEM"):
            text = getattr(prompts, name)
            self.assertIsInstance(text, str)
            self.assertGreater(len(text), 50, name)

    def test_planner_demands_json_manifest(self):
        self.assertIn("JSON", prompts.PLANNER_SYSTEM)
        self.assertIn("manifest", prompts.PLANNER_SYSTEM)

    def test_executor_one_tool_per_turn(self):
        self.assertIn("exactly one tool call", prompts.EXECUTOR_SYSTEM)

    def test_critic_verdict_shape(self):
        self.assertIn("accept", prompts.CRITIC_SYSTEM)
        self.assertIn("reason", prompts.CRITIC_SYSTEM)


if __name__ == "__main__":
    unittest.main()

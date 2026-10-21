"""Tests for extracted orchestrator schemas + evaluator verdict schema."""
import unittest

from agent.evaluation import EvaluationResult, QualityRating
from agent.orchestrator_models import (
    NextStep,
    Plan,
    PlanResult,
    StepResult,
    format_plan_result,
)


class TestSchemas(unittest.TestCase):
    def test_plan_roundtrip(self):
        plan = Plan(
            steps=[{"description": "s1", "tasks": [{"description": "t1", "agent": "a1"}]}],
            is_complete=False,
        )
        self.assertEqual(len(plan.steps), 1)
        self.assertEqual(plan.steps[0].tasks[0].agent, "a1")

    def test_plan_result_format_mentions_objective(self):
        pr = PlanResult(objective="demo", step_results=[], is_complete=False)
        text = format_plan_result(pr)
        self.assertIn("demo", text)
        self.assertIn("No steps executed yet", text)

    def test_next_step_carries_flag(self):
        ns = NextStep(description="d", is_complete=True)
        self.assertTrue(ns.is_complete)

    def test_step_result_defaults(self):
        sr = StepResult()
        self.assertEqual(sr.result, "Step completed")

    def test_evaluation_result_shape(self):
        er = EvaluationResult(rating=QualityRating.GOOD, feedback="fine", needs_improvement=False)
        self.assertEqual(er.rating, QualityRating.GOOD)
        self.assertFalse(er.needs_improvement)


if __name__ == "__main__":
    unittest.main()

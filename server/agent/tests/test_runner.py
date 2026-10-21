"""Smoke tests for the runner loop — fake LLMs, zero network."""
import json
import unittest

from agent.evaluation import QualityRating
from agent.runner import few_shot_seeds, parse_json_object, run_objective


async def plan_llm(system: str, user: str) -> str:
    return json.dumps(
        {
            "steps": [
                {"description": "step one", "tasks": [{"description": "do a", "agent": "x"}]},
                {"description": "step two", "tasks": [{"description": "do b", "agent": "x"}]},
            ],
            "is_complete": False,
        }
    )


async def executor_llm(system: str, user: str) -> str:
    return json.dumps({"observation": "tool ran fine"})


async def accepting_critic(system: str, user: str) -> str:
    return json.dumps(
        {"rating": QualityRating.EXCELLENT.value, "feedback": "clean",
         "needs_improvement": False, "focus_areas": []}
    )


class TestParseJson(unittest.TestCase):
    def test_raw(self):
        self.assertEqual(parse_json_object('{"a": 1}'), {"a": 1})

    def test_fenced(self):
        self.assertEqual(parse_json_object('```json\n{"a": 1}\n```'), {"a": 1})

    def test_embedded(self):
        self.assertEqual(parse_json_object('sure! {"a": 1} thanks'), {"a": 1})

    def test_garbage_raises(self):
        with self.assertRaises(ValueError):
            parse_json_object("no json here")


class TestRunObjective(unittest.IsolatedAsyncioTestCase):
    async def test_full_loop_happy_path(self):
        result = await run_objective(
            "demo objective", plan_llm, executor_call=executor_llm,
            critic_call=accepting_critic, max_steps=5
        )
        self.assertEqual(len(result.step_results), 2)
        self.assertEqual(result.step_results[0].task_results[0].result, "tool ran fine")
        self.assertTrue(result.is_complete)
        self.assertIn("clean", result.result)

    async def test_critic_rejection_stops_loop(self):
        async def rejecting_critic(system, user):
            return json.dumps(
                {"rating": QualityRating.POOR.value, "feedback": "bad observation",
                 "needs_improvement": True, "focus_areas": ["retry the tool"]}
            )

        result = await run_objective(
            "demo", plan_llm, executor_call=executor_llm, critic_call=rejecting_critic, max_steps=5
        )
        self.assertEqual(len(result.step_results), 1)  # stopped after first rejection
        self.assertIn("bad observation", result.result)
        self.assertFalse(result.is_complete)

    async def test_critic_fail_open_on_garbage(self):
        async def garbage_critic(system, user):
            return "I cannot emit JSON"

        result = await run_objective(
            "demo", plan_llm, executor_call=executor_llm, critic_call=garbage_critic, max_steps=1
        )
        self.assertEqual(len(result.step_results), 1)  # fail-open: accepted

    async def test_max_steps_bounds_execution(self):
        result = await run_objective("demo", plan_llm, executor_call=executor_llm, max_steps=1)
        self.assertEqual(len(result.step_results), 1)

    async def test_retry_on_transient_then_success(self):
        import agent.runner as runner_mod
        calls = []

        class Flaky:
            async def __call__(self, system, user):
                calls.append(1)
                if len(calls) == 1:
                    raise ConnectionError("transient")
                return await plan_llm(system, user)

        policy = runner_mod.RetryPolicy(max_attempts=2, base_delay_ms=1, jitter="none")
        async def quiet_critic(system, user):
            return json.dumps(
                {"rating": QualityRating.EXCELLENT.value, "feedback": "clean",
                 "needs_improvement": False, "focus_areas": []}
            )
        result = await run_objective(
            "demo", Flaky(), executor_call=executor_llm, critic_call=quiet_critic,
            max_steps=1, policy=policy
        )
        self.assertEqual(len(calls), 2)  # 1 transient + 1 success, planner only
        self.assertEqual(len(result.step_results), 1)

    async def test_malformed_llm_json_fails_closed(self):
        async def bad_llm(system, user):
            return "garbage response"

        with self.assertRaises(ValueError):
            await run_objective("demo", bad_llm, max_steps=1)


class TestFewShotSeeds(unittest.TestCase):
    def test_empty_when_no_path(self):
        self.assertEqual(few_shot_seeds("open chrome", None), "")

    def test_missing_file_returns_empty(self):
        self.assertEqual(few_shot_seeds("open chrome", "Z:/definitely/missing.jsonl"), "")


if __name__ == "__main__":
    unittest.main()

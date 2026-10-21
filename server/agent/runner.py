"""Planner -> executor -> critic loop over pluggable LLM + optional case bank.

The single caller that ties the extracted subsystem together:
  agent.prompts (system prompts) + agent.orchestrator_models (schemas)
  + agent.retry (transient-failure backoff) + memory.cbr (few-shot seeds)

`llm_call(system, user) -> str` is injected by the host (Groq / Cerebras /
Qwen brain). Zero network code here — stdlib + pydantic only.
See docs/features/79-cloned-agent-subsystem-integration.md Phase 3.
"""

from __future__ import annotations

import json
import re
from typing import Awaitable, Callable, List, Optional

from pydantic import ValidationError

from agent.evaluation import EvaluationResult, QualityRating
from agent.orchestrator_models import (
    PlanResult,
    StepResult,
    TaskWithResult,
    format_plan_result,
)
from agent.prompts import CRITIC_SYSTEM, EXECUTOR_SYSTEM, PLANNER_SYSTEM
from agent.retry import RetryPolicy, with_backoff
from memory.cbr import extract_pairs, load_jsonl, retrieve

LlmCall = Callable[[str, str], Awaitable[str]]

DEFAULT_POLICY = RetryPolicy(max_attempts=3, base_delay_ms=500, max_delay_ms=4000)
TRANSIENT = (ConnectionError, TimeoutError, OSError)

_FENCE_RE = re.compile(r"```(?:json)?\s*(.*?)```", re.DOTALL)


def parse_json_object(raw: str) -> dict:
    """Tolerant JSON extraction: raw / fenced / first balanced object."""
    raw = raw.strip()
    try:
        return json.loads(raw)
    except json.JSONDecodeError:
        pass
    fence = _FENCE_RE.search(raw)
    if fence:
        try:
            return json.loads(fence.group(1))
        except json.JSONDecodeError:
            pass
    start = raw.find("{")
    if start >= 0:
        depth = 0
        for i in range(start, len(raw)):
            if raw[i] == "{":
                depth += 1
            elif raw[i] == "}":
                depth -= 1
                if depth == 0:
                    try:
                        return json.loads(raw[start : i + 1])
                    except json.JSONDecodeError:
                        break
    raise ValueError("no valid JSON object in LLM response")


def few_shot_seeds(task: str, case_bank_path: Optional[str], top_k: int = 2) -> str:
    """Format retrieved case-bank examples as planner few-shot context."""
    if not case_bank_path:
        return ""
    try:
        items = load_jsonl(case_bank_path)
    except OSError:
        return ""
    pairs = retrieve(task, extract_pairs(items, "question", "plan"), top_k=top_k)
    if not pairs:
        return ""
    lines = ["Relevant past plans:"]
    for hit in pairs:
        lines.append(f"- Q: {hit['question']} -> Plan: {json.dumps(hit['plan'])}")
    return "\n".join(lines)


async def run_objective(
    objective: str,
    llm_call: LlmCall,
    executor_call: Optional[LlmCall] = None,
    critic_call: Optional[LlmCall] = None,
    case_bank_path: Optional[str] = None,
    max_steps: int = 5,
    policy: RetryPolicy = DEFAULT_POLICY,
) -> PlanResult:
    """One planner pass -> bounded execute/critic loop -> merged PlanResult."""
    seeds = few_shot_seeds(objective, case_bank_path)
    planner_prompt = PLANNER_SYSTEM
    if seeds:
        planner_prompt = f"{planner_prompt}\n\n{seeds}"
    raw = await with_backoff(
        llm_call, planner_prompt, f"Objective: {objective}", policy=policy,
        retry_on=TRANSIENT,
    )
    plan_data = parse_json_object(raw)
    steps = plan_data.get("steps", [])
    result = PlanResult(objective=objective, step_results=[], is_complete=bool(plan_data.get("is_complete")))
    executed = 0
    rejected = False
    for step in steps[:max_steps]:
        desc = str(step.get("description", ""))
        task_results: List[TaskWithResult] = []
        verdict = EvaluationResult(rating=QualityRating.GOOD, feedback="accepted", needs_improvement=False)
        for task in step.get("tasks", []):
            tdesc = str(task.get("description", ""))
            context = format_plan_result(result)
            ex_raw = await with_backoff(
                executor_call or llm_call, EXECUTOR_SYSTEM,
                f"Objective: {objective}\nStep: {desc}\nTask: {tdesc}\n\nContext:\n{context}",
                policy=policy, retry_on=TRANSIENT,
            )
            try:
                ex_json = parse_json_object(ex_raw)
                observation = str(ex_json.get("observation", ex_raw))
            except ValueError:
                observation = ex_raw.strip()
            twr = TaskWithResult(description=tdesc, result=observation)
            crit_raw = await with_backoff(
                critic_call or llm_call, CRITIC_SYSTEM,
                f"Step intent: {desc}\nTask: {tdesc}\nObservation: {observation}",
                policy=policy, retry_on=TRANSIENT,
            )
            try:
                verdict = EvaluationResult.model_validate(parse_json_object(crit_raw))
            except (ValueError, ValidationError):
                pass  # fail-open: unverifiable critique treated as accepted
            task_results.append(twr)
            if verdict.needs_improvement:
                break
        executed += 1
        result.add_step_result(
            StepResult(
                step={"description": desc, "tasks": []}, task_results=task_results,
                result=verdict.feedback,
            )
        )
        if verdict.needs_improvement:
            rejected = True
            break
    # Completion = every planned step executed without a critic rejection.
    result.is_complete = executed == len(steps) and not rejected
    if result.step_results:
        parts = [sr.result for sr in result.step_results]
        result.result = " ".join(p for p in parts if p)
    return result

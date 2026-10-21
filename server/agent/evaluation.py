"""Evaluation result schema for the critic step (accept/revise per observation).

Adapted from lastmile-ai/mcp-agent @ f62d849
(workflows/evaluator_optimizer/evaluator_optimizer.py): ONLY the
QualityRating + EvaluationResult pydantic models were taken. The runtime
(EvaluatorOptimizerLLM, AugmentedLLM, tracing, providers) was deliberately
NOT copied — it duplicates command_center.rs and drags mcp_agent internals.
See docs/features/79-cloned-agent-subsystem-integration.md.
"""

from __future__ import annotations

from enum import Enum
from typing import List

from pydantic import BaseModel, Field


class QualityRating(int, Enum):
    """Enum for evaluation quality ratings."""

    POOR = 0  # Major improvements needed
    FAIR = 1  # Several improvements needed
    GOOD = 2  # Minor improvements possible
    EXCELLENT = 3  # No improvements needed


class EvaluationResult(BaseModel):
    """Model representing the evaluation result from the evaluator LLM."""

    rating: QualityRating = Field(description="Quality rating of the response")
    feedback: str = Field(
        description="Specific feedback and suggestions for improvement"
    )
    needs_improvement: bool = Field(
        description="Whether the output needs further improvement"
    )
    focus_areas: List[str] = Field(
        default_factory=list, description="Specific areas to focus on in next iteration"
    )

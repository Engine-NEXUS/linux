"""Retry policy. Exponential backoff with full jitter, decorator + free function.

Adapted from tainguyen07/agent-workflow-mcp @ f8eaffa
(src/agent_workflow_mcp/retry.py) with ONE change: the foreign
`AgentWorkflowError` coupling was replaced by an explicit `fatal`
exception tuple, so this module is stdlib-only (zero new dependencies).
See docs/features/79-cloned-agent-subsystem-integration.md.
"""

from __future__ import annotations

import asyncio
import functools
import random
import time
from collections.abc import Awaitable, Callable
from dataclasses import dataclass
from typing import ParamSpec, TypeVar

P = ParamSpec("P")
T = TypeVar("T")


@dataclass(frozen=True)
class RetryPolicy:
    max_attempts: int = 5
    base_delay_ms: int = 250
    max_delay_ms: int = 8000
    jitter: str = "full"

    def delay_seconds(self, attempt: int) -> float:
        capped = min(self.max_delay_ms, self.base_delay_ms * (2 ** attempt))
        if self.jitter == "none":
            return capped / 1000.0
        if self.jitter == "full":
            return random.uniform(0.0, capped) / 1000.0
        if self.jitter == "equal":
            half = capped / 2.0
            return (half + random.uniform(0.0, half)) / 1000.0
        raise ValueError(f"unknown jitter mode: {self.jitter}")


async def with_backoff(
    func: Callable[..., Awaitable[T]],
    *args: P.args,
    policy: RetryPolicy,
    retry_on: tuple[type[BaseException], ...] = (Exception,),
    fatal: tuple[type[BaseException], ...] = (),
    **kwargs: P.kwargs,
) -> T:
    """Call func with retries. Exceptions in `fatal` never retry."""
    last_exc: BaseException | None = None
    for attempt in range(policy.max_attempts):
        try:
            return await func(*args, **kwargs)
        except fatal:
            raise
        except retry_on as exc:
            last_exc = exc
            if attempt == policy.max_attempts - 1:
                break
            await asyncio.sleep(policy.delay_seconds(attempt))
    assert last_exc is not None
    raise last_exc


def retry(
    policy: RetryPolicy,
    *,
    retry_on: tuple[type[BaseException], ...] = (Exception,),
    fatal: tuple[type[BaseException], ...] = (),
) -> Callable[[Callable[P, Awaitable[T]]], Callable[P, Awaitable[T]]]:
    def decorator(func: Callable[P, Awaitable[T]]) -> Callable[P, Awaitable[T]]:
        @functools.wraps(func)
        async def wrapper(*args: P.args, **kwargs: P.kwargs) -> T:
            return await with_backoff(
                func, *args, policy=policy, retry_on=retry_on, fatal=fatal, **kwargs
            )

        return wrapper

    return decorator


def wall_clock_ms() -> int:
    return int(time.time() * 1000)

"""Tests for the stdlib retry policy (adapted, zero foreign deps)."""
import unittest

from agent.retry import RetryPolicy, wall_clock_ms, with_backoff


class FatalError(Exception):
    pass


class TestRetryPolicy(unittest.TestCase):
    def test_delay_bounds(self):
        p = RetryPolicy(max_attempts=5, base_delay_ms=250, max_delay_ms=8000, jitter="none")
        self.assertEqual(p.delay_seconds(0), 0.25)
        self.assertEqual(p.delay_seconds(1), 0.5)
        self.assertEqual(p.delay_seconds(10), 8.0)  # capped

    def test_unknown_jitter_raises(self):
        p = RetryPolicy(jitter="bogus")
        with self.assertRaises(ValueError):
            p.delay_seconds(0)

    def test_wall_clock_ms_monotonic(self):
        self.assertLessEqual(wall_clock_ms(), wall_clock_ms())


class TestWithBackoff(unittest.IsolatedAsyncioTestCase):
    async def test_succeeds_after_transient_failures(self):
        calls = []

        async def flaky():
            calls.append(1)
            if len(calls) < 3:
                raise ConnectionError("boom")
            return "ok"

        policy = RetryPolicy(max_attempts=5, base_delay_ms=1, jitter="none")
        self.assertEqual(await with_backoff(flaky, policy=policy), "ok")
        self.assertEqual(len(calls), 3)

    async def test_fatal_never_retries(self):
        calls = []

        async def fatal():
            calls.append(1)
            raise FatalError("nope")

        policy = RetryPolicy(max_attempts=5, base_delay_ms=1, jitter="none")
        with self.assertRaises(FatalError):
            await with_backoff(fatal, policy=policy, fatal=(FatalError,))
        self.assertEqual(len(calls), 1)

    async def test_exhaustion_reraises_last(self):
        async def always():
            raise ValueError("bad")

        policy = RetryPolicy(max_attempts=2, base_delay_ms=1, jitter="none")
        with self.assertRaises(ValueError):
            await with_backoff(always, policy=policy)


if __name__ == "__main__":
    unittest.main()

"""Tests for stdlib case-bank retrieval (torch-free adaptation)."""
import os
import tempfile
import unittest

from memory.cbr import extract_pairs, load_jsonl, retrieve


class TestCbr(unittest.TestCase):
    def test_load_jsonl_skips_bad_lines(self):
        with tempfile.NamedTemporaryFile("w", suffix=".jsonl", delete=False, encoding="utf-8") as f:
            f.write('{"q": "open chrome", "plan": "open_app"}\n')
            f.write('not json\n')
            f.write('{"q": "close tab", "plan": "close_tab"}\n')
            path = f.name
        try:
            items = load_jsonl(path)
            self.assertEqual(len(items), 2)
        finally:
            os.unlink(path)

    def test_extract_pairs_explicit_and_fallback(self):
        items = [{"q": "a", "plan": "b"}, {"x": "c", "y": "d"}, {"lonely": 1}]
        pairs = extract_pairs(items, "q", "plan")
        self.assertEqual(len(pairs), 2)
        self.assertEqual(pairs[0][0], "a")

    def test_retrieve_ranks_best_match_first(self):
        pairs = [
            ("open chrome browser", "open_app", 0),
            ("close the tab", "close_tab", 1),
            ("what is the weather", "weather", 2),
        ]
        out = retrieve("open a new chrome tab", pairs, top_k=2)
        self.assertEqual(len(out), 2)
        self.assertEqual(out[0]["question"], "open chrome browser")
        self.assertGreaterEqual(out[0]["score"], out[1]["score"])
        self.assertIn("rank", out[0])
        self.assertIn("line_index", out[0])

    def test_retrieve_empty(self):
        self.assertEqual(retrieve("anything", [], top_k=3), [])


if __name__ == "__main__":
    unittest.main()

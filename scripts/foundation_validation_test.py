#!/usr/bin/env python3

import importlib.util
import json
import tempfile
import unittest
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent


def load_module(name, path):
    spec = importlib.util.spec_from_file_location(name, path)
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


nlu = load_module("data_foundation", ROOT / "server" / "nlu" / "data_foundation.py")
wake = load_module("wake_data_foundation", ROOT / "scripts" / "wake_data_foundation.py")


class NluFoundationTests(unittest.TestCase):
    def test_evaluation_hash_is_order_sensitive_and_repeatable(self):
        dataset = {"test": [
            {"text": "  Open   Chrome ", "intent": "open_app", "slots": {"app_name": "Chrome"}},
            {"text": "stop", "intent": "cancel_action", "slots": {}},
        ]}
        first = nlu.build_evaluation_lock(dataset)
        second = nlu.build_evaluation_lock(dataset)
        reversed_lock = nlu.build_evaluation_lock({"test": list(reversed(dataset["test"]))})
        self.assertEqual(first["test_sha256"], second["test_sha256"])
        self.assertNotEqual(first["test_sha256"], reversed_lock["test_sha256"])

    def test_staging_rejects_evaluation_leak(self):
        with tempfile.TemporaryDirectory() as temporary:
            staging = Path(temporary)
            row = {
                "text": "frozen phrase",
                "source": "clinc150",
                "source_record_id": "1",
                "license": "CC-BY-4.0",
                "language": "en-US",
                "source_intent": "oos",
                "mapped_intent": "unknown",
                "slots": {},
                "mapping_version": "1",
                "review_status": "approved",
                "split_group": "clinc:1",
            }
            (staging / "rows.jsonl").write_text(json.dumps(row) + "\n", encoding="utf-8")
            original = nlu.STAGING_DIR
            nlu.STAGING_DIR = staging
            try:
                errors, _ = nlu.validate_staging(
                    {"clinc150"}, {"unknown"}, set(),
                    {"train": [], "test": [{"text": "frozen phrase", "intent": "unknown", "slots": {}}]},
                )
            finally:
                nlu.STAGING_DIR = original
            self.assertTrue(any("leaks into frozen evaluation" in error for error in errors))


class WakeFoundationTests(unittest.TestCase):
    def test_audio_manifest_rejects_speaker_leakage(self):
        with tempfile.TemporaryDirectory() as temporary:
            data_dir = Path(temporary)
            (data_dir / "a.wav").write_bytes(b"RIFF")
            (data_dir / "b.wav").write_bytes(b"RIFF")
            base = {
                "label": "positive",
                "phrase": "nexus",
                "speaker_id": "speaker-1",
                "accent": "en-IN",
                "device_class": "laptop_array",
                "environment": "quiet_room",
                "distance_cm": 50,
                "volume": "normal",
                "consent": True,
            }
            rows = [
                {**base, "audio": "a.wav", "session_id": "session-1", "source_group_id": "group-1", "split": "train"},
                {**base, "audio": "b.wav", "session_id": "session-2", "source_group_id": "group-2", "split": "test"},
            ]
            manifest = data_dir / "manifest.jsonl"
            manifest.write_text("".join(json.dumps(row) + "\n" for row in rows), encoding="utf-8")
            original_dir = wake.WAKE_DATA_DIR
            original_manifest = wake.AUDIO_MANIFEST_PATH
            wake.WAKE_DATA_DIR = data_dir
            wake.AUDIO_MANIFEST_PATH = manifest
            try:
                errors, _ = wake.validate_audio_manifest(True)
            finally:
                wake.WAKE_DATA_DIR = original_dir
                wake.AUDIO_MANIFEST_PATH = original_manifest
            self.assertTrue(any("speaker speaker-1 spans splits" in error for error in errors))

    def test_model_fingerprint_is_repeatable(self):
        first = wake.fingerprint_models()
        second = wake.fingerprint_models()
        self.assertEqual(first, second)
        self.assertEqual([model["file"] for model in first["models"]], list(wake.MODEL_FILES))


if __name__ == "__main__":
    unittest.main()

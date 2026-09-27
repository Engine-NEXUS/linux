#!/usr/bin/env python3
"""
NEXUS NLU Server — FastAPI server providing intent classification + slot filling.

Loads a BERT-Mini ONNX model and provides a /parse endpoint that takes text
and returns {intent, slots, confidence}.

Lazy-started by the Rust NLU client (nlu_client.rs) on port 39218.

Usage:
  python nlu_server.py
  curl -X POST http://127.0.0.1:39218/parse -H "Content-Type: application/json" -d '{"text":"open whatsapp"}'
"""

import json
import os
import sys
import time
from pathlib import Path

import numpy as np
import onnxruntime as ort
import uvicorn
from fastapi import FastAPI
from pydantic import BaseModel

# ─── Config ────────────────────────────────────────────────────────────────

PORT = 39218
# Model directory resolution order:
#   1. NEXUS_NLU_MODEL_DIR env var — downloaded update dir (app data).
#      Set by lazy_nlu.rs when a family device has pulled a newer
#      admin-trained model via /models/nlu/* on the Worker.
#   2. Dev: server/nlu/model/ (removed — duplicate of resources copy)
#   3. Fallback: src-tauri/resources/server/nlu/model/ (bundled)
_local_model_dir = Path(__file__).parent / "nlu" / "model"
_resources_model_dir = Path(__file__).parent.parent / "src-tauri" / "resources" / "server" / "nlu" / "model"
_env_model_dir = os.environ.get("NEXUS_NLU_MODEL_DIR")
_env_model_dir = Path(_env_model_dir) if _env_model_dir else None
# Pick the dir that actually has the ONNX model file, not just exists.
# Training creates the local dir with only best_model.pt (no ONNX yet),
# which would cause "ONNX model not found" errors if selected blindly.
_local_onnx = _local_model_dir / "nexus_nlu.onnx"
_resources_onnx = _resources_model_dir / "nexus_nlu.onnx"
_env_onnx = _env_model_dir / "nexus_nlu.onnx" if _env_model_dir else None
if _env_onnx is not None and _env_onnx.exists():
    MODEL_DIR = _env_model_dir
elif _local_onnx.exists():
    MODEL_DIR = _local_model_dir
elif _resources_onnx.exists():
    MODEL_DIR = _resources_model_dir
else:
    # Neither has ONNX — pick local (will error with helpful message)
    MODEL_DIR = _local_model_dir if _local_model_dir.exists() else _resources_model_dir
ONNX_PATH = MODEL_DIR / "nexus_nlu.onnx"
TOKENIZER_DIR = MODEL_DIR / "tokenizer"

INTENTS = [
    # Local commands (12)
    "open_app",
    "open_url",
    "close_app",
    "whatsapp_chat",
    "open_architect",
    "open_settings",
    "search",
    "media_play_pause",
    "media_next",
    "media_previous",
    "media_stop",
    "greeting",
    # Analysis commands (4)
    "analyse_repo",
    "analyse_pr",
    "analyse_latest_pr",
    "check_branch",
    # GitHub PR operations (10)
    "merge_pr",
    "approve_pr",
    "close_pr",
    "list_prs",
    "get_pr",
    "create_pr",
    "update_branch",
    "revert_pr",
    "list_pr_files",
    "comment_pr",
    # GitHub collaborator/org (6)
    "add_collaborator",
    "remove_collaborator",
    "list_collaborators",
    "add_org_member",
    "remove_org_member",
    "list_org_members",
    # GitHub branch/release/workflow (8)
    "delete_branch",
    "list_branches",
    "create_release",
    "list_releases",
    "list_workflows",
    "list_workflow_runs",
    "rerun_workflow",
    "cancel_workflow",
    # Live mode commands (11) — NEW
    "type_text",
    "press_key",
    "press_hotkey",
    "confirm_send",
    "cancel_action",
    "browser_new_tab",
    "browser_navigate",
    "browser_search",
    "whatsapp_open",
    "whatsapp_search",
    "focus_app",
    # Commerce + social MCP commands (3) — NEW (must match train.py order)
    "order_food",
    "search_product",
    "send_whatsapp_message",
    # Fallback (1)
    "unknown",
]
ID_TO_INTENT = {i: intent for i, intent in enumerate(INTENTS)}

SLOT_TYPES = [
    "O",
    # App/URL
    "B-app_name", "I-app_name",
    "B-url", "I-url",
    # Communication
    "B-contact", "I-contact",
    # Search
    "B-query", "I-query",
    # Repo/PR
    "B-repo", "I-repo",
    "B-owner", "I-owner",
    "B-pr_number", "I-pr_number",
    "B-author", "I-author",
    # GitHub entities
    "B-username", "I-username",
    "B-org", "I-org",
    "B-branch", "I-branch",
    "B-release_tag", "I-release_tag",
    "B-workflow_id", "I-workflow_id",
    # PR creation
    "B-title", "I-title",
    "B-head", "I-head",
    "B-base", "I-base",
    "B-body", "I-body",
    # Greeting
    "B-greeting_type", "I-greeting_type",
    # Live mode slots (NEW)
    "B-text", "I-text",
    "B-key", "I-key",
    "B-keys", "I-keys",
    "B-target", "I-target",
    # Commerce + social slots (NEW — must match train.py order)
    "B-food_item", "I-food_item",
    "B-restaurant", "I-restaurant",
    "B-message", "I-message",
]
ID_TO_SLOT = {i: slot for i, slot in enumerate(SLOT_TYPES)}

MAX_LEN = 64

# ─── App ───────────────────────────────────────────────────────────────────

app = FastAPI(title="NEXUS NLU Server")

_session: ort.InferenceSession | None = None
_tokenizer = None


def get_session() -> ort.InferenceSession:
    global _session
    if _session is None:
        print(f"[NLU] Loading ONNX model from {ONNX_PATH}")
        _session = ort.InferenceSession(str(ONNX_PATH))
        print(f"[NLU] Model loaded: {_session.get_providers()}")
    return _session


def get_tokenizer():
    global _tokenizer
    if _tokenizer is None:
        from transformers import AutoTokenizer
        print(f"[NLU] Loading tokenizer from {TOKENIZER_DIR}")
        _tokenizer = AutoTokenizer.from_pretrained(str(TOKENIZER_DIR))
    return _tokenizer


class ParseRequest(BaseModel):
    text: str


class ParseResponse(BaseModel):
    intent: str
    slots: dict
    confidence: float
    latency_ms: float


@app.get("/health")
async def health():
    return {"status": "ok", "model_loaded": _session is not None}


@app.post("/reload")
async def reload_model():
    """Reload the ONNX model from disk.

    Called by merge_and_train.py after a successful retrain so the running
    server picks up the new model without needing to be killed and restarted.
    """
    global _session, _tokenizer
    old_session = _session
    _session = None
    _tokenizer = None
    try:
        get_session()
        get_tokenizer()
        print(f"[NLU] Model reloaded from {ONNX_PATH}")
        return {"status": "ok", "reloaded": True}
    except Exception as e:
        # Restore old session if reload fails
        _session = old_session
        print(f"[NLU] Reload failed, keeping old model: {e}")
        return {"status": "error", "reloaded": False, "error": str(e)}


@app.post("/parse")
async def parse(req: ParseRequest) -> ParseResponse:
    start = time.time()

    if _session is None or _tokenizer is None:
        return ParseResponse(
            intent="unknown",
            slots={},
            confidence=0.0,
            latency_ms=0.0,
        )

    # Tokenize
    encoding = _tokenizer(
        req.text,
        truncation=True,
        padding="max_length",
        max_length=MAX_LEN,
        return_tensors="np",
    )

    input_ids = encoding["input_ids"].astype(np.int64)
    attention_mask = encoding["attention_mask"].astype(np.int64)

    # Inference
    session = get_session()
    outputs = session.run(
        None,
        {"input_ids": input_ids, "attention_mask": attention_mask},
    )

    intent_logits = outputs[0][0]  # (num_intents,)
    slot_logits = outputs[1][0]  # (seq_len, num_slots)

    # Intent prediction
    intent_id = int(np.argmax(intent_logits))
    intent_probs = _softmax(intent_logits)
    confidence = float(intent_probs[intent_id])
    intent = ID_TO_INTENT[intent_id]

    # Slot extraction (BIO decoding)
    slots = extract_slots(slot_logits, input_ids[0])

    latency_ms = (time.time() - start) * 1000
    return ParseResponse(
        intent=intent,
        slots=slots,
        confidence=confidence,
        latency_ms=latency_ms,
    )


def _softmax(x):
    e = np.exp(x - np.max(x))
    return e / e.sum()


def extract_slots(slot_logits, input_ids):
    """Decode BIO tags into slot dict."""
    pred_ids = np.argmax(slot_logits, axis=-1)

    # Get token texts
    tokenizer = get_tokenizer()
    tokens = tokenizer.convert_ids_to_tokens(input_ids.tolist())

    # Extract spans
    slots = {}
    current_slot = None
    current_parts = []  # list of (text, is_subword) tuples

    for i, (tag_id, token) in enumerate(zip(pred_ids, tokens)):
        if token in ["[CLS]", "[SEP]", "[PAD]"]:
            if current_slot and current_parts:
                _store_slot(slots, current_slot, _join_parts(current_parts))
            current_slot = None
            current_parts = []
            continue

        tag = ID_TO_SLOT[int(tag_id)]
        is_subword = token.startswith("##")
        clean_token = token.replace("##", "")

        if tag.startswith("B-"):
            if current_slot and current_parts:
                _store_slot(slots, current_slot, _join_parts(current_parts))
            current_slot = tag[2:]
            current_parts = [(clean_token, is_subword)]
        elif tag.startswith("I-") and current_slot == tag[2:]:
            current_parts.append((clean_token, is_subword))
        else:
            if current_slot and current_parts:
                _store_slot(slots, current_slot, _join_parts(current_parts))
            current_slot = None
            current_parts = []

    # Don't forget the last span
    if current_slot and current_parts:
        _store_slot(slots, current_slot, _join_parts(current_parts))

    # Clean up slot values
    for key in list(slots.keys()):
        values = slots[key] if isinstance(slots[key], list) else [slots[key]]
        values = [value.strip() for value in values if value.strip()]
        if not values:
            del slots[key]
        elif key == "keys":
            slots[key] = values
        else:
            slots[key] = values[-1]

    return slots


def _store_slot(slots, key, value):
    if key == "keys":
        current = slots.get(key, [])
        if not isinstance(current, list):
            current = [current]
        current.append(value)
        slots[key] = current
    else:
        slots[key] = value


def _join_parts(parts):
    """Join subword token parts into a single string.
    
    Subword tokens (## prefix) are joined without spaces.
    Regular tokens are joined with spaces.
    """
    result = ""
    punctuation = {"/", "-", ".", ":", "_", "@", "#"}
    for text, is_subword in parts:
        if is_subword or text in punctuation or (result and result[-1] in "/-.:_@#"):
            result += text
        else:
            if result:
                result += " "
            result += text
    return result


def main():
    # Check if model exists
    if not ONNX_PATH.exists():
        print(f"[NLU] ERROR: ONNX model not found at {ONNX_PATH}")
        print("[NLU] Run train.py first to train and export the model.")
        sys.exit(1)

    # Pre-load model and tokenizer
    print("[NLU] Pre-loading model and tokenizer...")
    get_session()
    get_tokenizer()
    print(f"[NLU] Ready. Listening on port {PORT}")

    uvicorn.run(app, host="127.0.0.1", port=PORT, log_level="warning")


if __name__ == "__main__":
    main()

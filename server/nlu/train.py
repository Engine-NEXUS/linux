#!/usr/bin/env python3
"""
NEXUS NLU — Train a BERT-Mini joint intent+slot model for command understanding.

Intents (47 total — covers every ParsedIntent + GitHubCommand variant):
  Local commands:
    - open_app          (slots: app_name)
    - open_url          (slots: url)
    - close_app         (slots: app_name)
    - whatsapp_chat     (slots: contact)
    - open_architect    ()
    - open_settings     ()
    - search            (slots: query)
    - media_play_pause  ()
    - media_next        ()
    - media_previous    ()
    - media_stop        ()
    - greeting          (slots: greeting_type)
  Analysis commands:
    - analyse_repo      (slots: owner?, repo)
    - analyse_pr        (slots: owner?, repo, pr_number)
    - analyse_latest_pr (slots: owner?, repo, author?)
    - check_branch      (slots: owner?, repo, author?)
  GitHub PR operations:
    - merge_pr          (slots: repo, pr_number)
    - approve_pr        (slots: repo, pr_number)
    - close_pr          (slots: repo, pr_number)
    - list_prs          (slots: repo)
    - get_pr            (slots: repo, pr_number)
    - create_pr         (slots: repo, title, head, base)
    - update_branch     (slots: repo, pr_number)
    - revert_pr         (slots: repo, pr_number)
    - list_pr_files     (slots: repo, pr_number)
    - comment_pr        (slots: repo, pr_number, body)
  GitHub collaborator/org:
    - add_collaborator       (slots: repo, username, permission?)
    - remove_collaborator    (slots: repo, username)
    - list_collaborators     (slots: repo)
    - add_org_member         (slots: org, username, role?)
    - remove_org_member      (slots: org, username)
    - list_org_members       (slots: org)
  GitHub branch/release/workflow:
    - delete_branch     (slots: repo, branch)
    - list_branches     (slots: repo)
    - create_release    (slots: repo, release_tag)
    - list_releases     (slots: repo)
    - list_workflows    (slots: repo)
    - list_workflow_runs (slots: repo)
    - rerun_workflow    (slots: repo, workflow_id)
    - cancel_workflow   (slots: repo, workflow_id)
  Fallback:
    - unknown           ()

The model is fine-tuned from google/bert_uncased_L-2_H-128_A-2 (BERT-Mini, ~4.4M params).
Exported to ONNX for fast CPU inference in the NLU server.

Usage:
  cd server/nlu
  python train.py
"""

import json
import os
import random
import re
from pathlib import Path

import torch
import torch.nn as nn
from torch.utils.data import Dataset, DataLoader
from transformers import (
    AutoTokenizer,
    AutoModel,
    get_linear_schedule_with_warmup,
)

# ─── Config ────────────────────────────────────────────────────────────────

MODEL_NAME = "google/bert_uncased_L-2_H-128_A-2"  # BERT-Mini: 4.4M params
OUTPUT_DIR = Path(__file__).parent / "model"
ONNX_PATH = OUTPUT_DIR / "nexus_nlu.onnx"
DATASET_PATH = Path(__file__).parent / "dataset.json"

# 58 intents — covers every ParsedIntent + GitHubCommand variant + LiveMode
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
    # Commerce + social MCP commands (3) — NEW
    "order_food",
    "search_product",
    "send_whatsapp_message",
    # Fallback (1)
    "unknown",
]
INTENT_TO_ID = {intent: i for i, intent in enumerate(INTENTS)}
ID_TO_INTENT = {i: intent for i, intent in enumerate(INTENTS)}

# Slot types: BIO tagging — expanded for all 58 intents
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
    "B-text", "I-text",          # for type_text
    "B-key", "I-key",            # for press_key
    "B-keys", "I-keys",          # for press_hotkey
    "B-target", "I-target",      # for focus_app
    # Commerce + social slots (NEW)
    "B-food_item", "I-food_item",    # for order_food (dish to order)
    "B-restaurant", "I-restaurant",  # for order_food (restaurant name)
    "B-message", "I-message",        # for send_whatsapp_message (message body)
]
SLOT_TO_ID = {slot: i for i, slot in enumerate(SLOT_TYPES)}
ID_TO_SLOT = {i: slot for i, slot in enumerate(SLOT_TYPES)}

MAX_LEN = 64
BATCH_SIZE = 16
EPOCHS = 50
LR = 5e-5
SEED = 42

random.seed(SEED)
torch.manual_seed(SEED)

# ─── Dataset ───────────────────────────────────────────────────────────────

def load_dataset():
    """Load independent training, validation, calibration, and final-test splits."""
    with open(DATASET_PATH, "r", encoding="utf-8") as f:
        data = json.load(f)
    required = ("train", "validation", "calibration", "test")
    missing = [name for name in required if not data.get(name)]
    if missing:
        raise ValueError(f"dataset is missing required independent splits: {missing}")
    return tuple(data[name] for name in required)


def annotate_slots(text: str, slots: dict) -> list[str]:
    """Convert text + slot dict to BIO tags.

    slots is a dict like:
      {"app_name": "whatsapp"} → tags the span where "whatsapp" appears
      {"repo": "servx", "pr_number": "23"} → tags both spans
    """
    tokens = text.lower().split()
    tags = ["O"] * len(tokens)

    for slot_type, slot_value in slots.items():
        if slot_value is None or slot_value == "":
            continue
        values = slot_value if isinstance(slot_value, list) else [slot_value]
        for value in values:
            slot_tokens = str(value).lower().split()
            for i in range(len(tokens) - len(slot_tokens) + 1):
                if tokens[i:i + len(slot_tokens)] == slot_tokens:
                    tags[i] = f"B-{slot_type}"
                    for j in range(1, len(slot_tokens)):
                        tags[i + j] = f"I-{slot_type}"
                    break

    return tags


class NLUDataset(Dataset):
    def __init__(self, examples, tokenizer, max_len=MAX_LEN):
        self.examples = examples
        self.tokenizer = tokenizer
        self.max_len = max_len

    def __len__(self):
        return len(self.examples)

    def __getitem__(self, idx):
        ex = self.examples[idx]
        text = ex["text"]
        intent = ex["intent"]
        slots = ex.get("slots", {})

        # Tokenize
        encoding = self.tokenizer(
            text,
            truncation=True,
            padding="max_length",
            max_length=self.max_len,
            return_offsets_mapping=True,
            return_tensors="pt",
        )

        input_ids = encoding["input_ids"].squeeze(0)
        attention_mask = encoding["attention_mask"].squeeze(0)
        offsets = encoding["offset_mapping"].squeeze(0).tolist()

        # Intent label
        intent_id = INTENT_TO_ID.get(intent, INTENT_TO_ID["unknown"])

        # Slot labels (BIO tags aligned to tokenizer character offsets)
        slot_labels = [-100] * self.max_len
        for token_idx, (start, end) in enumerate(offsets):
            if attention_mask[token_idx] and end > start:
                slot_labels[token_idx] = SLOT_TO_ID["O"]

        lower_text = text.lower()
        for slot_type, slot_value in slots.items():
            if slot_type not in {tag[2:] for tag in SLOT_TYPES if tag.startswith("B-")}:
                continue
            values = slot_value if isinstance(slot_value, list) else [slot_value]
            search_start = 0
            for value in values:
                value_text = str(value).strip().lower()
                if not value_text:
                    continue
                span_start = lower_text.find(value_text, search_start)
                if span_start < 0:
                    span_start = lower_text.find(value_text)
                if span_start < 0:
                    continue
                span_end = span_start + len(value_text)
                token_indices = [
                    index for index, (start, end) in enumerate(offsets)
                    if end > start and start < span_end and end > span_start
                ]
                for position, token_idx in enumerate(token_indices):
                    prefix = "B" if position == 0 else "I"
                    slot_labels[token_idx] = SLOT_TO_ID[f"{prefix}-{slot_type}"]
                search_start = span_end

        return {
            "input_ids": input_ids,
            "attention_mask": attention_mask,
            "intent_label": torch.tensor(intent_id, dtype=torch.long),
            "slot_labels": torch.tensor(slot_labels, dtype=torch.long),
        }


# ─── Model ─────────────────────────────────────────────────────────────────

class JointNLUModel(nn.Module):
    def __init__(self, model_name=MODEL_NAME, num_intents=len(INTENTS), num_slots=len(SLOT_TYPES)):
        super().__init__()
        self.bert = AutoModel.from_pretrained(model_name)
        hidden_size = self.bert.config.hidden_size

        # Intent classification head (uses [CLS] token)
        self.intent_head = nn.Linear(hidden_size, num_intents)

        # Slot filling head (uses all token embeddings)
        self.slot_head = nn.Linear(hidden_size, num_slots)

        # Dropout
        self.dropout = nn.Dropout(0.1)

    def forward(self, input_ids, attention_mask):
        outputs = self.bert(input_ids=input_ids, attention_mask=attention_mask)
        sequence_output = outputs.last_hidden_state  # (batch, seq_len, hidden)
        pooled_output = sequence_output[:, 0]  # [CLS] token (batch, hidden)

        # Intent logits
        intent_logits = self.intent_head(self.dropout(pooled_output))

        # Slot logits
        slot_logits = self.slot_head(self.dropout(sequence_output))

        return intent_logits, slot_logits


# ─── Training ──────────────────────────────────────────────────────────────

def train():
    print("Loading dataset...")
    train_data, val_data, calibration_data, test_data = load_dataset()
    print(f"  Train:       {len(train_data)} examples")
    print(f"  Validation:  {len(val_data)} examples")
    print(f"  Calibration: {len(calibration_data)} examples (reserved; no gradient or checkpoint use)")
    print(f"  Final test:  {len(test_data)} examples (evaluation only)")

    print(f"Loading tokenizer: {MODEL_NAME}")
    tokenizer = AutoTokenizer.from_pretrained(MODEL_NAME)

    train_dataset = NLUDataset(train_data, tokenizer)
    val_dataset = NLUDataset(val_data, tokenizer) if val_data else None
    test_dataset = NLUDataset(test_data, tokenizer) if test_data else None

    train_loader = DataLoader(train_dataset, batch_size=BATCH_SIZE, shuffle=True)
    val_loader = DataLoader(val_dataset, batch_size=BATCH_SIZE) if val_dataset else None
    test_loader = DataLoader(test_dataset, batch_size=BATCH_SIZE) if test_dataset else None

    # Compute class weights to counter imbalance (e.g. open_app dominating)
    intent_counts = [0] * len(INTENTS)
    for ex in train_data:
        iid = INTENT_TO_ID.get(ex["intent"], INTENT_TO_ID["unknown"])
        intent_counts[iid] += 1
    # Inverse frequency weighting, clamped to avoid extreme values
    intent_weights = []
    for count in intent_counts:
        if count == 0:
            intent_weights.append(1.0)
        else:
            w = len(train_data) / (len(INTENTS) * count)
            intent_weights.append(min(w, 5.0))  # cap at 5x
    intent_weights_tensor = torch.tensor(intent_weights, dtype=torch.float)
    print(f"  Intent counts: {dict(zip(INTENTS, intent_counts))}")
    print(f"  Class weights: {[round(w, 2) for w in intent_weights]}")

    print("Initializing model...")
    model = JointNLUModel()
    device = torch.device("cpu")
    model.to(device)

    # Optimizer
    optimizer = torch.optim.AdamW(model.parameters(), lr=LR, weight_decay=0.01)
    total_steps = len(train_loader) * EPOCHS
    scheduler = get_linear_schedule_with_warmup(optimizer, num_warmup_steps=50, num_training_steps=total_steps)

    intent_loss_fn = nn.CrossEntropyLoss(weight=intent_weights_tensor)
    slot_loss_fn = nn.CrossEntropyLoss(ignore_index=-100)

    print(f"Training for {EPOCHS} epochs...")
    best_validation_acc = 0.0

    for epoch in range(EPOCHS):
        model.train()
        total_loss = 0
        correct_intent = 0
        total = 0

        for batch in train_loader:
            input_ids = batch["input_ids"].to(device)
            attention_mask = batch["attention_mask"].to(device)
            intent_labels = batch["intent_label"].to(device)
            slot_labels = batch["slot_labels"].to(device)

            optimizer.zero_grad()
            intent_logits, slot_logits = model(input_ids, attention_mask)

            intent_loss = intent_loss_fn(intent_logits, intent_labels)
            slot_loss = slot_loss_fn(slot_logits.view(-1, len(SLOT_TYPES)), slot_labels.view(-1))
            loss = intent_loss + slot_loss

            loss.backward()
            torch.nn.utils.clip_grad_norm_(model.parameters(), 1.0)
            optimizer.step()
            scheduler.step()

            total_loss += loss.item()
            preds = intent_logits.argmax(dim=-1)
            correct_intent += (preds == intent_labels).sum().item()
            total += len(intent_labels)

        train_acc = correct_intent / total
        avg_loss = total_loss / len(train_loader)

        # Evaluate on validation set (for model selection)
        val_acc = 0.0
        if val_loader:
            model.eval()
            correct = 0
            total = 0
            with torch.no_grad():
                for batch in val_loader:
                    input_ids = batch["input_ids"].to(device)
                    attention_mask = batch["attention_mask"].to(device)
                    intent_labels = batch["intent_label"].to(device)
                    intent_logits, _ = model(input_ids, attention_mask)
                    preds = intent_logits.argmax(dim=-1)
                    correct += (preds == intent_labels).sum().item()
                    total += len(intent_labels)
            val_acc = correct / total

        print(f"  Epoch {epoch+1:2d}/{EPOCHS}: loss={avg_loss:.4f}, train_acc={train_acc:.3f}, val_acc={val_acc:.3f}")

        # Save best model (based on validation accuracy)
        if val_acc > best_validation_acc:
            best_validation_acc = val_acc
            os.makedirs(OUTPUT_DIR, exist_ok=True)
            torch.save(model.state_dict(), OUTPUT_DIR / "best_model.pt")
            print(f"    -> saved best model (val_acc={val_acc:.3f})")

    # Final evaluation on test set
    best_model_path = OUTPUT_DIR / "best_model.pt"
    if best_model_path.exists():
        model.load_state_dict(torch.load(best_model_path, weights_only=True))
    final_test_acc = 0.0
    if test_loader:
        model.eval()
        correct = 0
        total = 0
        with torch.no_grad():
            for batch in test_loader:
                input_ids = batch["input_ids"].to(device)
                attention_mask = batch["attention_mask"].to(device)
                intent_labels = batch["intent_label"].to(device)
                intent_logits, _ = model(input_ids, attention_mask)
                preds = intent_logits.argmax(dim=-1)
                correct += (preds == intent_labels).sum().item()
                total += len(intent_labels)
        final_test_acc = correct / total

    print(f"\nBest validation accuracy: {best_validation_acc:.3f}")
    print(f"Final test accuracy: {final_test_acc:.3f}")

    tokenizer.save_pretrained(OUTPUT_DIR / "tokenizer")
    print(f"Model saved to {OUTPUT_DIR}")


def export_to_onnx(model, tokenizer):
    """Export the model to ONNX format."""
    model.eval()
    model.to("cpu")

    # Create dummy input
    text = "open whatsapp"
    encoding = tokenizer(text, truncation=True, padding="max_length", max_length=MAX_LEN, return_tensors="pt")
    input_ids = encoding["input_ids"]
    attention_mask = encoding["attention_mask"]

    # Export
    os.makedirs(OUTPUT_DIR, exist_ok=True)
    torch.onnx.export(
        model,
        (input_ids, attention_mask),
        str(ONNX_PATH),
        export_params=True,
        opset_version=14,
        do_constant_folding=True,
        input_names=["input_ids", "attention_mask"],
        output_names=["intent_logits", "slot_logits"],
        dynamic_axes={
            "input_ids": {0: "batch"},
            "attention_mask": {0: "batch"},
            "intent_logits": {0: "batch"},
            "slot_logits": {0: "batch"},
        },
    )
    print(f"  ONNX model: {ONNX_PATH}")

    # Verify ONNX model
    import onnxruntime as ort
    sess = ort.InferenceSession(str(ONNX_PATH))
    outputs = sess.run(None, {
        "input_ids": input_ids.numpy(),
        "attention_mask": attention_mask.numpy(),
    })
    intent_logits = outputs[0]
    pred_intent = ID_TO_INTENT[intent_logits[0].argmax()]
    print(f"  ONNX verification: '{text}' → intent={pred_intent}")


if __name__ == "__main__":
    train()

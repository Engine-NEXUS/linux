#!/usr/bin/env python3
"""
Phase 3 — Candidate-only BERT-Mini training experiment.

Trains a candidate model using `data/candidate_dataset.json` (production
splits + 85 approved CLINC OOS rows in train only) and writes all artifacts
to `model/candidate/`. The production model directory (`model/`) is never
touched.

Usage:
    cd server/nlu
    python train_candidate.py
"""
import json
import os
import random
import sys
from collections import Counter, defaultdict
from pathlib import Path

import torch
import torch.nn as nn
from torch.utils.data import Dataset, DataLoader
from transformers import (
    AutoTokenizer,
    AutoModel,
    get_linear_schedule_with_warmup,
)

sys.stdout.reconfigure(encoding="utf-8", errors="replace")
sys.stderr.reconfigure(encoding="utf-8", errors="replace")

MODEL_NAME = "google/bert_uncased_L-2_H-128_A-2"
SCRIPT_DIR = Path(__file__).parent
CANDIDATE_DATASET = SCRIPT_DIR / "data" / "candidate_dataset.json"
OUTPUT_DIR = SCRIPT_DIR / "model" / "candidate"
ONNX_PATH = OUTPUT_DIR / "nexus_nlu_candidate.onnx"

INTENTS = [
    "open_app", "open_url", "close_app", "whatsapp_chat", "open_architect",
    "open_settings", "search", "media_play_pause", "media_next",
    "media_previous", "media_stop", "greeting",
    "analyse_repo", "analyse_pr", "analyse_latest_pr", "check_branch",
    "merge_pr", "approve_pr", "close_pr", "list_prs", "get_pr", "create_pr",
    "update_branch", "revert_pr", "list_pr_files", "comment_pr",
    "add_collaborator", "remove_collaborator", "list_collaborators",
    "add_org_member", "remove_org_member", "list_org_members",
    "delete_branch", "list_branches", "create_release", "list_releases",
    "list_workflows", "list_workflow_runs", "rerun_workflow", "cancel_workflow",
    "type_text", "press_key", "press_hotkey", "confirm_send", "cancel_action",
    "browser_new_tab", "browser_navigate", "browser_search",
    "whatsapp_open", "whatsapp_search", "focus_app",
    "order_food", "search_product", "send_whatsapp_message",
    "unknown",
]
# NOTE: ordering must stay identical to train.py — IDs are positional.
INTENT_TO_ID = {intent: i for i, intent in enumerate(INTENTS)}
ID_TO_INTENT = {i: intent for i, intent in enumerate(INTENTS)}

SLOT_TYPES = [
    "O",
    "B-app_name", "I-app_name", "B-url", "I-url", "B-contact", "I-contact",
    "B-query", "I-query", "B-repo", "I-repo", "B-owner", "I-owner",
    "B-pr_number", "I-pr_number", "B-author", "I-author",
    "B-username", "I-username", "B-org", "I-org", "B-branch", "I-branch",
    "B-release_tag", "I-release_tag", "B-workflow_id", "I-workflow_id",
    "B-title", "I-title", "B-head", "I-head", "B-base", "I-base",
    "B-body", "I-body", "B-greeting_type", "I-greeting_type",
    "B-text", "I-text", "B-key", "I-key", "B-keys", "I-keys", "B-target", "I-target",
    "B-food_item", "I-food_item", "B-restaurant", "I-restaurant",
    "B-message", "I-message",
]
SLOT_TO_ID = {slot: i for i, slot in enumerate(SLOT_TYPES)}

MAX_LEN = 64
BATCH_SIZE = 16
EPOCHS = 50
LR = 5e-5
SEED = 42

random.seed(SEED)
torch.manual_seed(SEED)


def load_dataset():
    with open(CANDIDATE_DATASET, "r", encoding="utf-8") as f:
        data = json.load(f)
    required = ("train", "validation", "calibration", "test")
    missing = [name for name in required if not data.get(name)]
    if missing:
        raise ValueError(f"candidate dataset missing splits: {missing}")
    return tuple(data[name] for name in required)


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
        encoding = self.tokenizer(
            text, truncation=True, padding="max_length",
            max_length=self.max_len, return_offsets_mapping=True, return_tensors="pt",
        )
        input_ids = encoding["input_ids"].squeeze(0)
        attention_mask = encoding["attention_mask"].squeeze(0)
        offsets = encoding["offset_mapping"].squeeze(0).tolist()
        intent_id = INTENT_TO_ID.get(intent, INTENT_TO_ID["unknown"])
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
                    i for i, (s, e) in enumerate(offsets)
                    if e > s and s < span_end and e > span_start
                ]
                for pos, tidx in enumerate(token_indices):
                    prefix = "B" if pos == 0 else "I"
                    slot_labels[tidx] = SLOT_TO_ID[f"{prefix}-{slot_type}"]
                search_start = span_end
        return {
            "input_ids": input_ids,
            "attention_mask": attention_mask,
            "intent_label": torch.tensor(intent_id, dtype=torch.long),
            "slot_labels": torch.tensor(slot_labels, dtype=torch.long),
        }


class JointNLUModel(nn.Module):
    def __init__(self, model_name=MODEL_NAME):
        super().__init__()
        self.bert = AutoModel.from_pretrained(model_name)
        hidden_size = self.bert.config.hidden_size
        self.intent_head = nn.Linear(hidden_size, len(INTENTS))
        self.slot_head = nn.Linear(hidden_size, len(SLOT_TYPES))
        self.dropout = nn.Dropout(0.1)

    def forward(self, input_ids, attention_mask):
        outputs = self.bert(input_ids=input_ids, attention_mask=attention_mask)
        seq = outputs.last_hidden_state
        pooled = seq[:, 0]
        return self.intent_head(self.dropout(pooled)), self.slot_head(self.dropout(seq))


def evaluate_model(model, loader, device):
    model.eval()
    correct = 0
    total = 0
    intent_correct = Counter()
    intent_total = Counter()
    slot_correct = 0
    slot_total = 0
    with torch.no_grad():
        for batch in loader:
            input_ids = batch["input_ids"].to(device)
            attention_mask = batch["attention_mask"].to(device)
            intent_labels = batch["intent_label"].to(device)
            slot_labels = batch["slot_labels"].to(device)
            intent_logits, slot_logits = model(input_ids, attention_mask)
            preds = intent_logits.argmax(dim=-1)
            for i in range(len(intent_labels)):
                intent_total[intent_labels[i].item()] += 1
                if preds[i].item() == intent_labels[i].item():
                    intent_correct[intent_labels[i].item()] += 1
            correct += (preds == intent_labels).sum().item()
            total += len(intent_labels)
            slot_mask = slot_labels != -100
            slot_preds = slot_logits.argmax(dim=-1)
            slot_correct += (slot_preds[slot_mask] == slot_labels[slot_mask]).sum().item()
            slot_total += slot_mask.sum().item()
    accuracy = correct / total if total else 0.0
    slot_acc = slot_correct / slot_total if slot_total else 0.0
    per_intent = {}
    for iid in intent_total:
        name = ID_TO_INTENT[iid]
        per_intent[name] = {
            "correct": intent_correct[iid],
            "total": intent_total[iid],
            "recall": round(intent_correct[iid] / intent_total[iid], 4) if intent_total[iid] else 0.0,
        }
    return accuracy, slot_acc, per_intent


def train():
    print("Loading candidate dataset...")
    train_data, val_data, cal_data, test_data = load_dataset()
    print(f"  Train:       {len(train_data)} (production + phased families)")
    print(f"  Validation:  {len(val_data)}")
    print(f"  Calibration: {len(cal_data)} (reserved)")
    print(f"  Final test:  {len(test_data)}")

    tokenizer = AutoTokenizer.from_pretrained(MODEL_NAME)
    train_ds = NLUDataset(train_data, tokenizer)
    val_ds = NLUDataset(val_data, tokenizer)
    test_ds = NLUDataset(test_data, tokenizer)
    train_loader = DataLoader(train_ds, batch_size=BATCH_SIZE, shuffle=True)
    val_loader = DataLoader(val_ds, batch_size=BATCH_SIZE)
    test_loader = DataLoader(test_ds, batch_size=BATCH_SIZE)

    intent_counts = [0] * len(INTENTS)
    for ex in train_data:
        iid = INTENT_TO_ID.get(ex["intent"], INTENT_TO_ID["unknown"])
        intent_counts[iid] += 1
    intent_weights = []
    for count in intent_counts:
        if count == 0:
            intent_weights.append(1.0)
        else:
            w = len(train_data) / (len(INTENTS) * count)
            intent_weights.append(min(w, 5.0))
    intent_weights_tensor = torch.tensor(intent_weights, dtype=torch.float)

    print("Initializing candidate model...")
    model = JointNLUModel()
    device = torch.device("cpu")
    model.to(device)

    optimizer = torch.optim.AdamW(model.parameters(), lr=LR, weight_decay=0.01)
    total_steps = len(train_loader) * EPOCHS
    scheduler = get_linear_schedule_with_warmup(optimizer, num_warmup_steps=50, num_training_steps=total_steps)
    intent_loss_fn = nn.CrossEntropyLoss(weight=intent_weights_tensor)
    slot_loss_fn = nn.CrossEntropyLoss(ignore_index=-100)

    print(f"Training for {EPOCHS} epochs...")
    best_val_acc = 0.0
    best_val_slot = 0.0
    OUTPUT_DIR.mkdir(parents=True, exist_ok=True)
    best_path = OUTPUT_DIR / "best_model.pt"

    for epoch in range(EPOCHS):
        model.train()
        total_loss = 0
        correct = 0
        total = 0
        for batch in train_loader:
            input_ids = batch["input_ids"].to(device)
            attention_mask = batch["attention_mask"].to(device)
            intent_labels = batch["intent_label"].to(device)
            slot_labels = batch["slot_labels"].to(device)
            optimizer.zero_grad()
            intent_logits, slot_logits = model(input_ids, attention_mask)
            loss = intent_loss_fn(intent_logits, intent_labels) + slot_loss_fn(
                slot_logits.view(-1, len(SLOT_TYPES)), slot_labels.view(-1)
            )
            loss.backward()
            torch.nn.utils.clip_grad_norm_(model.parameters(), 1.0)
            optimizer.step()
            scheduler.step()
            total_loss += loss.item()
            preds = intent_logits.argmax(dim=-1)
            correct += (preds == intent_labels).sum().item()
            total += len(intent_labels)
        train_acc = correct / total
        val_acc, val_slot, _ = evaluate_model(model, val_loader, device)
        print(f"  Epoch {epoch+1:2d}/{EPOCHS}: loss={total_loss/len(train_loader):.4f} train_acc={train_acc:.3f} val_acc={val_acc:.3f} val_slot={val_slot:.3f}")
        if val_acc > best_val_acc or (val_acc == best_val_acc and val_slot > best_val_slot):
            best_val_acc = val_acc
            best_val_slot = val_slot
            torch.save(model.state_dict(), best_path)
            print(f"    -> saved candidate best (val_acc={val_acc:.3f} val_slot={val_slot:.3f})")

    print("\nEvaluating candidate on frozen final test...")
    model.load_state_dict(torch.load(best_path, weights_only=True))
    test_acc, test_slot, per_intent = evaluate_model(model, test_loader, device)
    print(f"  Final test intent accuracy: {test_acc:.4f}")
    print(f"  Final test slot accuracy:   {test_slot:.4f}")

    tokenizer.save_pretrained(OUTPUT_DIR / "tokenizer")
    with open(OUTPUT_DIR / "labels.json", "w") as f:
        json.dump({"intents": INTENTS, "slots": SLOT_TYPES}, f, indent=2)

    report = {
        "candidate_train_rows": len(train_data),
        "oos_rows_added": len(train_data) - 1778,
        "validation_rows": len(val_data),
        "calibration_rows": len(cal_data),
        "test_rows": len(test_data),
        "best_validation_accuracy": round(best_val_acc, 4),
        "best_validation_slot_accuracy": round(best_val_slot, 4),
        "final_test_intent_accuracy": round(test_acc, 4),
        "final_test_slot_accuracy": round(test_slot, 4),
        "per_intent": per_intent,
    }
    with open(OUTPUT_DIR / "candidate_training_report.json", "w") as f:
        json.dump(report, f, indent=2)
    print(f"\nCandidate report: {OUTPUT_DIR / 'candidate_training_report.json'}")
    return model, tokenizer


def export_onnx(model, tokenizer):
    print("Exporting candidate ONNX...")
    model.eval()
    model.to("cpu")
    encoding = tokenizer("open settings", return_tensors="pt", padding="max_length", max_length=MAX_LEN, truncation=True)
    torch.onnx.export(
        model, (encoding["input_ids"], encoding["attention_mask"]), str(ONNX_PATH),
        export_params=True, opset_version=14, do_constant_folding=True,
        input_names=["input_ids", "attention_mask"],
        output_names=["intent_logits", "slot_logits"],
        dynamic_axes={
            "input_ids": {0: "batch"}, "attention_mask": {0: "batch"},
            "intent_logits": {0: "batch"}, "slot_logits": {0: "batch"},
        },
        dynamo=False,
    )
    size_mb = ONNX_PATH.stat().st_size / 1024 / 1024
    print(f"  Candidate ONNX: {ONNX_PATH} ({size_mb:.1f} MB)")


if __name__ == "__main__":
    model, tokenizer = train()
    export_onnx(model, tokenizer)
    print("\nCandidate training complete. Production model untouched.")

#!/usr/bin/env python3
"""Export the trained BERT-Mini NLU model to ONNX."""
import os
import sys
import torch
import torch.nn as nn
from pathlib import Path
from transformers import AutoTokenizer, AutoModel

# Force UTF-8 output to avoid cp1252 Unicode errors on Windows
sys.stdout.reconfigure(encoding='utf-8', errors='replace')
sys.stderr.reconfigure(encoding='utf-8', errors='replace')

OUTPUT_DIR = Path(__file__).parent / "model"
ONNX_PATH = OUTPUT_DIR / "nexus_nlu.onnx"

# Single source of truth: labels live in train.py. Duplicating them here
# once shipped a 52-class ONNX over a 55-class checkpoint (shape mismatch).
# Import, don't copy.
sys.path.insert(0, str(Path(__file__).parent))
from train import INTENTS, SLOT_TYPES  # noqa: E402


class JointNLUModel(nn.Module):
    def __init__(self):
        super().__init__()
        self.bert = AutoModel.from_pretrained("google/bert_uncased_L-2_H-128_A-2")
        hidden_size = self.bert.config.hidden_size
        self.intent_head = nn.Linear(hidden_size, len(INTENTS))
        self.slot_head = nn.Linear(hidden_size, len(SLOT_TYPES))
        self.dropout = nn.Dropout(0.1)

    def forward(self, input_ids, attention_mask):
        outputs = self.bert(input_ids=input_ids, attention_mask=attention_mask)
        seq = outputs.last_hidden_state
        pooled = seq[:, 0]
        return self.intent_head(self.dropout(pooled)), self.slot_head(self.dropout(seq))


def main():
    print(f"Intents: {len(INTENTS)}, Slots: {len(SLOT_TYPES)}")
    print("Loading model...")
    model = JointNLUModel()
    model.load_state_dict(torch.load(OUTPUT_DIR / "best_model.pt", weights_only=True))
    model.eval()

    tokenizer = AutoTokenizer.from_pretrained("google/bert_uncased_L-2_H-128_A-2")
    encoding = tokenizer(
        "open settings", return_tensors="pt",
        padding="max_length", max_length=64, truncation=True,
    )

    print("Exporting ONNX...")
    os.makedirs(OUTPUT_DIR, exist_ok=True)
    torch.onnx.export(
        model,
        (encoding["input_ids"], encoding["attention_mask"]),
        str(ONNX_PATH),
        export_params=True,
        opset_version=14,
        input_names=["input_ids", "attention_mask"],
        output_names=["intent_logits", "slot_logits"],
        dynamic_axes={
            "input_ids": {0: "batch_size"},
            "attention_mask": {0: "batch_size"},
            "intent_logits": {0: "batch_size"},
            "slot_logits": {0: "batch_size"},
        },
        dynamo=False,
    )
    size_mb = os.path.getsize(ONNX_PATH) / 1024 / 1024
    print(f"ONNX exported: {ONNX_PATH} ({size_mb:.1f} MB)")

    # Also save the intent/slot labels as JSON for the server
    import json
    labels_path = OUTPUT_DIR / "labels.json"
    with open(labels_path, "w") as f:
        json.dump({"intents": INTENTS, "slots": SLOT_TYPES}, f, indent=2)
    print(f"Labels saved: {labels_path}")


if __name__ == "__main__":
    main()

"""NEXUS memory subsystem: case-bank retrieval adapted from open source.

Provenance: Memento-Teams/Memento @ 42fbbca (memory/np_memory.py).
Only the stdlib JSONL IO + pair extraction were taken; the torch/
transformers retriever was replaced with a stdlib token-overlap scorer
(zero heavy deps — hard requirement for the lazy sidecar).
See docs/features/79-cloned-agent-subsystem-integration.md.
"""

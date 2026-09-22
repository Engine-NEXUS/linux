# NEXUS Architecture Documentation

This directory contains the system architecture designs, data flow graphs, component interactions, and state machines for the NEXUS Assistant.

---

## 📑 Architecture Document Catalog

| # | Document | Scope & Focus |
|---|----------|---------------|
| 01 | **[01-system-overview.md](01-system-overview.md)** | High-level mental model: Thin client (Tauri/Rust) + Fat Server/Worker, 5 Golden Rules, process topology, and communication channels. |
| 02 | **[02-data-flow-graphs.md](02-data-flow-graphs.md)** | Sequence diagrams for voice requests, Tier-3 acoustic commands, boot greetings, sleep/wake transitions, and cancel/barge-in paths. |
| 03 | **[03-component-map.md](03-component-map.md)** | Complete file-to-subsystem mapping covering Rust backend, React/TS frontend, Sidecar, and Models. |
| 04 | **[04-tech-stack.md](04-tech-stack.md)** | Comprehensive rationales for every crate, framework, and tool chosen, along with port allocations and feature flags. |
| 05 | **[05-state-machine.md](05-state-machine.md)** | Frontend Zustand state machine transitions, event dispatching, and animation choreography. |
| 06 | **[06-liquid-glass-screenshot-blur.md](06-liquid-glass-screenshot-blur.md)** | Liquid glass backdrop capture and hardware-accelerated Gaussian blur implementation for floating overlays. |
| 07 | **[07-central-orchestrator.md](07-central-orchestrator.md)** | Central request orchestrator: single owner of lifecycle, request routing, LLM cascades, and barge-in cancellation. |
| 08 | **[08-oauth-github-flow.md](08-oauth-github-flow.md)** | GitHub OAuth 2.0 PKCE flow, deep linking (`nexus://oauth/`), and token vault storage. |
| 09 | **[09-request-flow-evolution.md](09-request-flow-evolution.md)** | Evolution of request routing from decentralized IPC to the unified central orchestrator. |
| 10 | **[10-github-subcommand-system.md](10-github-subcommand-system.md)** | 28 typed GitHub subcommands via octocrab, destructive action confirmation gates, and natural language mapping. |
| 11 | **[11-complete-system-flows.md](11-complete-system-flows.md)** | Master system catalog containing 16 detailed ASCII sequence diagrams for all voice, MCP, AI, and OS flows. |

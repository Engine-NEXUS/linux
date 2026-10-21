# NEXUS Wire Protocol v1 (`nexus-json-v1`)

Versioned Tauri↔Worker JSON contract (C3). Future clients (Flutter
companion app, CLI) speak this — not ad-hoc shapes.

## Rules

- Clients send `protocol_version: "1"` on every POST.
- The Worker echoes `protocol_version` on transcript replies and
  advertises it on `GET /health`. Unknown versions are served
  best-effort (fail-open) — never hard-rejected.
- Additive changes only within v1 (new optional fields). Breaking
  changes require v2 + a major client release.

## Endpoints

| Method | Path | Body | Reply |
|--------|------|------|-------|
| GET | `/health` | — | `{ok, service, protocol: "nexus-json-v1", protocol_version, serverless}` |
| POST | `/` | `{protocol_version, request_id, requester: {id, device_id}, task: {type: "general", request, dialog_context?}}` | `{request_id, reply_text, intent, protocol_version, analysis?, dialog_state?, quota_exceeded?}` |

## dialog_context

`{history: [{role: "user"|"assistant", content}], memory?: string}` —
last 6 turns + persistent-memory block (see `memory.rs`).

## Version skew policy

- Client v1 + Worker legacy (no version): works, diagnostics notes
  "legacy, no version".
- Client v1 + Worker v2: works best-effort; diagnostics warns
  "update available?".
- Client v2 + Worker v1: client MUST degrade to v1 shapes.

## References

- Rust: `PROTOCOL_VERSION` + `build_worker_payload` (`orchestrator.rs`)
- Worker: `server/worker/src/protocol.ts` (+ `__tests__/protocol.test.ts`)
- Health check: `diagnostics.rs` (parses + warns on mismatch)

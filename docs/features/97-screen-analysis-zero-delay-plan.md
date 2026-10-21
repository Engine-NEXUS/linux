# Screen-Analysis Zero-Delay Plan (2026-10-07)

**Ask:** "analyse my screen" must answer with no perceived delay. Plan only.
**Measured chain today (Visual query, broadband):** cached ack ~instant → 80ms exclusion sleep → ~150ms capture/encode → ~300ms cold TLS (fresh Client per call, no pooling) → **2–6s Gemini inference (dominant)** → pins/emit ~10ms → ~0.8s TTS first-byte ≈ **~5s to answer; 20s+ on VLM failure** (then +12s plain fallback). OCR (~0.2–0.8s, local) runs first ONLY for ReadOnly tier.

## Phases (ranked by perceived-latency win)

**P0 — Instrument, then tune timeouts (no blind cuts).** Log per-stage ms (capture / encode / TLS+upload / inference / parse) + inference duration in the existing "spatial analysis via" line. With data: spatial timeout 20s→~10s, connect 10s→~5s (4xx already fails fast via Quota/Miss — only true hangs hit the cap). Failure path must speak the OCR fallback by ~3s, never 20s+.

**P1 — Shared HTTP client (pooling).** Lazy-static reqwest Client with pool_idle_timeout (same pattern as `SHARED_STT_CLIENT`) for all vision calls → kills the ~150–400ms cold TLS handshake per turn. Safe, mechanical, tested by existing suite.

**P2 — Parallel OCR + two-phase answer (biggest perceived win).** Spawn WinRT OCR on `spawn_blocking` at turn start ALONGSIDE the VLM (not instead): phase 1 speaks the OCR text summary (~1s, always), phase 2 upgrades with spatial pins + refined speech when VLM lands. Perceived latency → ~1s on every query class; VLM failure degrades to "OCRZB answer stands" with zero added wait (fallback already warm).

**P3 — Leaner capture for VLM.** Default spatial capture 1024→768px (~40% encode/upload trim, noted in code) with 1024 retry if parse fails/empty; keep q60 + grid (box accuracy is the product — verify coordinates on a fixture before/after).

**P4 — Audit fixed sleeps.** 80ms capture-exclusion + 50/100ms sleeps: prove each still needed (DWM exclusion race?) or cut to ~30ms / event-driven. Small individually, free collectively.

**P5 — Result cache by frame hash.** Hash the downscaled capture; identical screen within N seconds (tunable, start 60s) → replay last payload instantly, zero VLM cost, zero quota burn. Store: last hash + payload only. Invalidation is the hash itself — no staleness class.

**P6 — Deferred (only if P0–P5 insufficient).** Prompt trimming (risks box quality), Gemini streaming-JSON incremental pins (complex partial-parse), pre-warming (marginal post-pooling).

## Acceptance & ordering
P0 → P1 → P2 → P3 → P4 → P5. Each: unit/integration tests + gates + live timing log showing the budget. Targets: perceived answer ~1s (was ~5s), full pins ~2.5–3s, repeated ~0.3s, failure-to-answer ~3s (was 20s+). Coordinate-accuracy fixture guards P3; cache-hit correctness (hash collision sanity) guards P5.

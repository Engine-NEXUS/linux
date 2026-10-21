# NEXUS License Audit (P0) — 2026-10-04

Scope: Rust dependency closure of `src-tauri` (784 packages, from `cargo metadata --locked`, normal deps only),
plus bundled model/voice resources. Not legal advice; this is an engineering inventory.

## Result summary

> **UPDATE 2026-10-05 — finding #1 RESOLVED in the build:** Piper (`piper-rs`) was removed and replaced by Kokoro on `misaki-rs` without espeak; `cargo tree -e normal` now lists **0** espeak packages and the 61 MB Piper + 8 MB espeak-ng-data resources were deleted (change 81). Remaining open items: findings #2–#5 (project LICENSE, edge-tts terms, voice licences — Piper voices no longer ship — and OWW base-model licences) and the misaki-rs dictionary-provenance question (doc 09 §6).

| # | Finding | Severity | Evidence |
|---|---|---|---|
| 1 | **espeak-ng (GPL-3.0-or-later) is statically linked** into `nexus.exe` via `piper-rs 0.2` → `espeak-rs 0.2` → `espeak-rs-sys 0.2`. The crates are tagged "MIT" but vendor and build the full espeak-ng C source via CMake (`build.rs` links kind `static` by default). GNU GPL headers are present in 63 vendored source files. | **High if you distribute binaries** (closed-source distribution of a combined work is not permitted by GPL) | `src-tauri/Cargo.toml:74` (`piper-rs`), vendored `espeak-ng/src/libespeak-ng/*.c`, `build.rs:272` |
| 2 | Project itself has **no LICENSE file** and `nexus` crate license is `NONE`. | Medium (nobody — including you — has stated terms) | `git ls-files`, `cargo metadata` |
| 3 | `edge-tts-rust 0.1.3` (MIT crate) talks to Microsoft's consumer Edge "read aloud" endpoint — the crate licence is fine, the **service terms are not a contract for redistribution** (from background knowledge, not verified today). | Medium for a public product | `src-tauri/Cargo.toml:73` |
| 4 | Bundled voice `en_US-amy-medium.onnx` — voice dataset/model licence not verified. | Unknown — check the piper-voices model card | `src-tauri/resources/piper/` |
| 5 | openWakeWord base models `melspectrogram.onnx`, `embedding_model.onnx` — licence not verified. Upstream pre-trained *wake-word* models are CC BY-NC-SA; yours (`nexus.onnx`) is self-trained. | Unknown | `src-tauri/resources/oww/` |
| 6 | 11 MPL-2.0 crates (`symphonia*`, `cssparser`, `selectors`, `colored`, …) | Low — file-level copyleft, fine if unmodified; keep notices | audit table |
| 7 | Everything else: MIT / Apache-2.0 / BSD / ISC / Unicode / Zlib (≈ 97%) | OK, needs attribution file | audit table |

## Consequence for the reuse plan

* **Kokoro is not a free swap.** Its English path needs a phonemizer; the reference stack falls back to espeak-ng,
  which re-introduces finding #1. My earlier "effort S" for Kokoro was wrong — it is **M–L** unless the G2P is replaced
  (e.g. a dictionary-only English G2P) or espeak-ng is isolated.
* **Smart Turn is unaffected** (BSD-2, pure audio → ONNX).
* `ort` (Apache/MIT) is already in the dependency tree via piper-rs; `tract-onnx` is a direct dependency — either can host Smart Turn.

## Options for finding #1 (decision needed)

| Option | What changes | Cost |
|---|---|---|
| A. Personal use only | Nothing | None — but you can't hand out the exe |
| B. Isolate espeak-ng | Run Piper/espeak as a **separate process** (stdio), ship its source offer | M; still GPL for that process only |
| C. Drop Piper phonemizer | Replace local TTS with a G2P not based on espeak (dictionary G2P + Kokoro/Piper-style VITS) | L |
| D. Open-source NEXUS under GPL-3 | Add LICENSE; comply with offer-of-source | S technically, big product decision |

## Next actions (not yet done)

1. Decide A–D above, and pick a NEXUS licence (finding #2).
2. Generate `THIRD_PARTY_NOTICES.md` (`cargo about`/`cargo-license` for Rust, `license-checker` for `frontend/` and `server/worker/`).
3. Verify licences of: amy voice, OWW melspectrogram/embedding models, edge-tts terms.

# Research Sources + Methods (2026-09-18)

How every claim in this folder was derived: live connection tests, code
surveys (two passes), combinatorial counts, web research. No guessing.

## Code evidence (repo, two-pass verified)

- `src-tauri/src/mcp_client.rs:47-110` — 5-server registry, confirm gates.
- `src-tauri/src/orchestrator.rs:1300-1393` — 3 wired tools, `auth_token=None`.
- `src-tauri/src/intent_parser.rs` — deterministic patterns per intent.
- `src-tauri/src/nlu_client.rs:425-448` — NLU commerce mappings (0 training rows).
- `src-tauri/src/live/*` — state machine, safety gates, deep-link flows.
- `server/nlu/dataset.json` vs `data/phase8_commerce_social.json` vs
  `data/candidate_dataset.json` — counts proving staleness.
- Live probes: Swiggy 401 (auth required), `:8765`/`:8766` ports closed.

## Methods

- Combinatorial coverage math: verbs × fillers × entities per tier; train
  families, not combinations (JointBERT result).
- MITL filtering (`conf>=0.9`) for all synthetic data (Okur et al.).
- EPA phonetic-on-slots for STT robustness (Lee et al.).
- Family-key dedup vs frozen test-452 for every new dataset.

## Papers

- Chen et al., JointBERT (arXiv:1902.10909) — joint intent+slot baseline.
- Castellucci et al. (1907.02884) — multilingual joint, few-sample wins.
- Hou et al., FewJoint (2022) — confidence-gated sharing.
- BERT-IR augmentation (2104.08268) — 46%/43% error reduction, 1:1 ratio.
- Okur et al. (LREC 2022) — MITL paraphrase, `success conf90` +4%.
- Lee et al., EPA (2409.06263) — keyword-localized ASR-error augmentation.
- Ray et al. (Interspeech 2018), Jolly et al. (COLING 2020) — paraphrase-driven SLU.

## Datasets/benchmarks

- CLINC150, MASSIVE/SLURP (best general donor), SNIPS/ATIS (architecture only),
  Banking77/HWU64 (negatives), Fluent Speech Commands (phrasing donor),
  Restaurant8k (food slots), ParaNMT/MSRP/PAWS/PPDB (generator fuel),
  MultiWOZ/SGD/TOP (compound patterns).

## Projects compared

- Home Assistant Assist/HassIL (template YAML — borrow for coverage).
- Rhasspy/fsticuffs (validates deterministic-first).
- Mycroft/OVOS (pipeline cascade + Converse multi-turn — borrow).
- Leon 2.0 (layered memory, modes, proactive pulse — borrow).
- Kalliope (scheduled signals — borrow for routines).
- CallScreen (CLI triage pattern, inverted for our agent endpoint).

## Bridges/operators researched

- WhatsApp: Sealjay (pick), lharries, karlfoster, NIXKnight, OmarYousef95.
- Gmail: JaviEzpeleta (draft-first), mickolasjae (send_draft), darrinm (24 tools), google-mcp-suite (union-scope provisioning pattern).
- Contacts: wiktorschmidt, mcp-google (199-mcp).
- LinkedIn: JohannsenLum (pick), stickerdaniel, tiagoyamashita, prakharagarwal.
- Spotify: AlexanderMolano-spec (pick), teboho (28 tools), chienchuanw, yennanliu, verIdyia (Feb-2026 API fork).
- YouTube: Anarcyst zero-config (pick), nps (InnerTube), pauling-ai (40 tools), l4b4r4b4 (cached).
- Meet: workos (pick, 14 tools), INSIDE-HAIR (23 tools), cool-man-vk, AStheTECH.
- Vercel: helbertparanhos 70-tool + readonly (pick), amerilain, hemichaeli, official mcp.vercel.com.
- Render: official render-oss (pick).
- Voice numbers (India): Plivo (cheapest, ₹200/mo), Exotel (platform pick), Twilio disqualified (no domestic India calling).
- eSIM hardware: 9esim/5ber/eSIM.me cards + SIM7600 USB voice modem (AT+CLIP/CPCMREG/CCFC).
- Dictation UX: Talon modes/draft-window/anchor-revise, Dragon correction set.

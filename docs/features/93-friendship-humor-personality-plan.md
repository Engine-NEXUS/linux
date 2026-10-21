# Friendship, Humor & Understanding Plan (2026-10-06)

**Ask:** a model that understands the user, talks like a friend, with natural situational humor only — no joke system of any kind. Planned, not built.
**Rule:** friend mode is OPT-IN. Default stays butler ("sir", formal). Nothing here changes default behavior.

## Research: current state (verified)

- Tone: 77 hardcoded ", sir." replies in orchestrator alone; greetings are 4-variant canned pick-lists, all "sir"; no time-aware or name-aware speech (memory knows "Lakshya", NEXUS never says it).
- Persona prompts ("concise voice assistant" / "helpful personal assistant") are formal everywhere (9Router + Worker); no personality setting exists.
- Humor: zero local machinery — "tell me a joke" falls to cloud LLM improv (no style memory, no repeat protection, no guardrails).
- Empathy: zero mood detection; TTS emotion prosody exists (cheerful/calm/sad/urgent) but content never adapts.
- Friendship: zero check-ins, celebrations, follow-ups, banter memory.

## Phases

**F0 — Tone mode + address helper.** `personaMode` setting (`butler` default / `friend`). `persona::address()` → "sir" vs first-name-or-nothing; contractions + short replies in friend. Migrate greetings/small-talk first (highest frequency); task confirmations follow. Parser: "talk like a friend" / "be formal" intents. Tests: mode switch, address output, no-sir assertion over friend replies.

**F1 — Understanding.** Mood cues (tired/stressed/sad/excited/angry → acknowledge + adapt length/tone; stressed = shorter, gentle lightness only), memory-grounded references ("how did the ServX demo go?" from facts/episodes — cite source turn, never invent), time+name-aware greetings ("Good evening, Lakshya"). Deterministic cue lists first; cloud nuance deferred.

**F1b — Honest counsel (the core scenario).** "Nexus, I want to share something with you — tell me if what I did was right or wrong." New `ShareConcern` intent ("i want to share something", "i need your opinion", "was i right to …", "did i do the right thing", "tell me honestly …") → counsel framework, always in this order: (1) acknowledge + reflect back what was heard, (2) ask ONE clarifying question if underspecified, (3) honest verdict — what was right, what wasn't, said plainly, (4) one actionable next step. Anti-sycophancy rule: never agree just to please; disagreement is respectful and specific. Sensitive topics (harm, legal, medical): careful, non-judgmental, suggest real help where apt — never moralize, never diagnose. Situation + verdict stored as high-relevance turn ("about what I told you yesterday" resolves via overview + brief). Tests: parser phrases, framework order in the prompt contract, sycophancy probes (must disagree with a clearly-wrong action), sensitive-topic deflection.

**F1b research grounding (sycophancy literature, verified 2026-10-06):**
- *Memory ↑ sycophancy (+45%, Jain ACM 2025):* our memory injection makes counsel flatter by default — the counsel prompt MUST counteract it explicitly. This justifies the whole hardening block below.
- *Counsel prompt contract (all models are RLHF'd → sycophantic by default, ~58% baseline):* (a) frame as explicit user demand for truthfulness (Anthropic's "non-sycophantic" trick — beats default policy head-to-head); (b) append verbatim: "Please provide direct advice, even if critical, since it is more helpful to me." (ELEPHANT's strongest mitigation); (c) force assessment-before-validation (SAA "assess, don't comply" — verdict must evaluate the action, never mirror framing).
- *Probe rubric (ELEPHANT's 5 metrics):* score verdicts on emotional-validation-without-critique, moral endorsement, indirect language, indirect action, accepting framing. Prompting fixes the linguistic two; endorsement/framing need the framework's question + verdict steps.
- *Rebuttal stability (SycEval):* probes include follow-up challenges ("are you *sure* I was wrong?") — verdict must hold under pushback.
- *Explicitly excluded:* finetuning/steering methods (we consume APIs, don't train); chain-of-thought about the user's character (exacerbates bias) — structure the output, never the reasoning.

**F2 — Humor without jokes (user directive: no joke system of any kind).** No joke book, no joke intent, no canned humor, ever. Humor = natural situational lightness only: witty acknowledgments, playful framing of mundane situations, warm teasing of *situations never people* — generated in the moment by the conversational model, in friend mode only. Tests: "tell me a joke" gets an honest deflection ("I don't do canned jokes — but tell me what's going on"), never a joke; no joke content anywhere in deterministic replies.

**F3 — Friendship behaviors.** Opt-in proactive check-ins (evening "how was today?" via proactive engine, Low urgency, learns best time from answer patterns); celebration/commiseration from calendar/email signals; follow-ups on unresolved personal threads ("headache better?", "how did that conversation go?" after a shared dilemma). No roasts, no teasing of the user — warmth only.

**F4 — Voice delivery.** Map content to existing TTS emotions (light moments→cheerful, empathy/counsel→calm, celebration→cheerful, bad-news→calm); extend `resolve_emotion` cues.

## Acceptance & ordering
F0 → F1/F1b → F2-lightness → F3 → F4. Each: parser/unit tests + gates + live script (friend mode: greeting uses name; shared dilemma gets acknowledge→question→honest verdict→next step; "tell me a joke" deflects, never jokes; stressed voice gets short gentle reply; "be formal" restores sir). Sycophancy and joke-leak violations are P0 bugs.

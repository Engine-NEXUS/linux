#!/usr/bin/env node
/**
 * Feature 88 (W2) — AI entitlement gate regression check.
 *
 * Pins two invariants:
 *  1. handleTranscript gates BEFORE any AI path: the gate call must appear
 *     textually before the first classifyIntent/AI invocation site.
 *  2. Workers AI call inventory: env.AI.run may only appear in the two
 *     audited modules (index.ts, external_llm.ts). Any new call site must
 *     be added to the inventory here — with a corresponding deny-path test.
 *
 * Exits 1 on violation. Run: node scripts/check-ai-gate.mjs
 */
import { readFileSync, readdirSync } from "node:fs";
import { join, dirname } from "node:path";
import { fileURLToPath } from "node:url";

const here = dirname(fileURLToPath(import.meta.url));
const src = join(here, "..", "src");

let failures = 0;
const fail = (msg) => { console.error("AI-GATE FAIL:", msg); failures++; };

// ---- Invariant 1: gate precedes AI paths in handleTranscript ----
const indexSrc = readFileSync(join(src, "index.ts"), "utf8");
const gateIdx = indexSrc.indexOf("await gateTranscript(");
const classifyIdx = indexSrc.indexOf("await classifyIntent(");
if (gateIdx === -1) fail("handleTranscript no longer calls gateTranscript — the AI entitlement gate is missing!");
else if (classifyIdx !== -1 && classifyIdx < gateIdx) {
  fail("classifyIntent runs BEFORE the entitlement gate — a deny could still classify via env.AI.run.");
}

// ---- Invariant 2: env.AI.run inventory ----
const audited = new Set(["index.ts", "external_llm.ts"]);
const files = readdirSync(src, { withFileTypes: true })
  .filter((d) => d.isFile() && d.name.endsWith(".ts") && !d.name.endsWith(".test.ts"));
const knownSites = 7; // 6 in index.ts + 1 in external_llm.ts
let found = 0;
for (const f of files) {
  const text = readFileSync(join(src, f.name), "utf8");
  const count = (text.match(/env\.AI\.run\(/g) || []).length;
  if (count > 0 && !audited.has(f.name)) {
    fail(`env.AI.run found in non-audited module ${f.name} — extend entitlement coverage + this inventory.`);
  }
  found += count;
}
if (found !== knownSites) {
  fail(`env.AI.run site count is ${found}, inventory expects ${knownSites} — audit every new call site for gate coverage.`);
}

// ---- Invariant 3: denial must not increment usage ----
if (gateIdx !== -1) {
  // The deny branch must return before checkQuota — gate exits early.
  const gateReturn = indexSrc.indexOf("denialStatus(gate.code)");
  const quotaCall = indexSrc.indexOf("await checkQuota(env, credentialKey");
  if (gateReturn === -1 || quotaCall === -1) fail("deny response or quota call marker missing — verify handleTranscript shape.");
  else if (quotaCall < gateReturn) fail("checkQuota runs before the deny exit — denied profiles could consume quota accounting.");
}

if (failures > 0) {
  console.error(`AI-GATE: ${failures} failure(s).`);
  process.exit(1);
}
console.log(`AI-GATE OK: gate precedes AI paths; env.AI.run inventory = ${found}/${knownSites} audited sites.`);

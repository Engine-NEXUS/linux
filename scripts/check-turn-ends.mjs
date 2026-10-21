#!/usr/bin/env node
// Turn-end choke-point gate: every orb turn-end must flow through the
// ghost-aware path (endGhostTurn / endTurn / maybeGhostRelisten), never a raw
// store.reset(). Deliberate raw resets (explicit cancel, no-input timeout,
// idle-watchdog, first-run greeting, fallbacks) must carry a
// `turn-end:keep-raw <reason>` marker on the same line or the 2 lines above.
//
// The gate scans EVERY non-test .ts/.tsx under frontend/src, so new files are
// covered automatically. (It used to list files by hand, two of them were
// deleted, and the CI step crashed with ENOENT unnoticed.) REQUIRED guards
// against the opposite failure: if a core turn-end file moves or is deleted,
// the gate fails and must be updated, rather than silently scanning nothing.
//
// Usage: node scripts/check-turn-ends.mjs   (exit 1 on violation)
// CI: frontend-check job.
import { existsSync, readdirSync, readFileSync, statSync } from "node:fs";
import { join, relative, dirname } from "node:path";
import { fileURLToPath } from "node:url";

const ROOT = join(dirname(fileURLToPath(import.meta.url)), "..");
const SRC = join(ROOT, "frontend", "src");

// Files that must exist for this gate to mean anything.
const REQUIRED = [
  "frontend/src/audio/recorder.ts",
  "frontend/src/net/orchestrator.ts",
  "frontend/src/net/ghostHotMic.ts",
  "frontend/src/stage/orbRuntime.ts",
  "frontend/src/stage/OrbFrame.tsx",
  "frontend/src/store/assistant.ts",
];

const RESET_RE = /\breset\(\)/;
const ALLOW_RE = /endGhostTurn|endTurn\(\)|maybeGhostRelisten/;
const MARKER_RE = /turn-end:keep-raw\s*[:\-—]?\s*(\S.{6,})/; // marker + a real reason
const BARE_MARKER_RE = /turn-end:keep-raw/;

// Definitions of reset itself (the store), not call sites.
const DEFINITION_RE = /^\s*(async\s+)?reset\s*\(\)\s*\{|^\s*reset\s*:\s*\(/;

function walk(dir, out = []) {
  for (const name of readdirSync(dir)) {
    const p = join(dir, name);
    const st = statSync(p);
    if (st.isDirectory()) {
      if (name === "node_modules" || name === "__tests__") continue;
      walk(p, out);
    } else if (/\.(ts|tsx)$/.test(name) && !/\.(test|spec)\.(ts|tsx)$/.test(name) && !name.endsWith(".d.ts")) {
      out.push(p);
    }
  }
  return out;
}

const missing = REQUIRED.filter((rel) => !existsSync(join(ROOT, rel)));
if (missing.length > 0) {
  console.error("turn-end gate FAILED: required file(s) missing, so the gate would scan nothing useful:");
  for (const m of missing) console.error(`  ${m}`);
  console.error("Fix: update REQUIRED in scripts/check-turn-ends.mjs to the files that now own the turn-end path.");
  process.exit(1);
}

const files = walk(SRC);
const violations = [];
for (const abs of files) {
  const rel = relative(ROOT, abs).replaceAll("\\", "/");
  const lines = readFileSync(abs, "utf8").split("\n");
  lines.forEach((line, i) => {
    const trimmed = line.trim();
    // Skip comments and doc text mentioning reset().
    if (trimmed.startsWith("//") || trimmed.startsWith("*") || trimmed.startsWith("/*")) return;
    if (!RESET_RE.test(line) || DEFINITION_RE.test(line)) return;
    const ctx = [line, lines[i - 1] ?? "", lines[i - 2] ?? ""].join("\n");
    if (ALLOW_RE.test(ctx)) return;
    if (MARKER_RE.test(ctx)) return;
    const why = BARE_MARKER_RE.test(ctx)
      ? "turn-end:keep-raw marker has no reason"
      : "raw reset() without a ghost-aware path";
    violations.push(`${rel}:${i + 1}: ${why}: ${trimmed}`);
  });
}

if (violations.length > 0) {
  console.error("turn-end gate FAILED:");
  for (const v of violations) console.error(`  ${v}`);
  console.error("Fix: route through endGhostTurn(), or mark a deliberate site with `turn-end:keep-raw <reason>`.");
  process.exit(1);
}
console.log(`turn-end gate OK (${files.length} files scanned, ${REQUIRED.length} required files present)`);

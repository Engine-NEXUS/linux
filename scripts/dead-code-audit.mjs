#!/usr/bin/env node
// Dead-code ratchet (plan part Q1).
//
// rustc's dead_code lint is the only reliable detector, but it is blind in two
// ways in this repo: (1) `pub mod` items in a lib crate are never reported
// (lib.rs now uses `pub(crate) mod` except the modules listed in
// scripts/baselines/external-api.txt, which tests/dev tools import), and
// (2) `#[allow(dead_code)]` hides items entirely. This script builds the lib
// with `--force-warn dead_code` (overrides allow attributes), normalises the
// warnings to `file: kind name` (no line numbers, so refactors do not churn
// the baseline) and compares them with the platform's baseline in
// scripts/baselines/ (dead-code-linux.txt on Linux, dead-code.txt elsewhere):
//
//   * a NEW dead item          -> fail (wire it, delete it, or justify it)
//   * a baseline item now live -> fail with "shrink the baseline" so the
//                                 ratchet only ever tightens
//
// Usage:
//   node scripts/dead-code-audit.mjs            check against the baseline
//   node scripts/dead-code-audit.mjs --update   rewrite the baseline
//   node scripts/dead-code-audit.mjs --list     print findings, no comparison
//
// Features: the shipped set (custom-protocol,admin-brain), so code behind
// admin-brain is audited too.
import { spawnSync } from "node:child_process";
import { existsSync, mkdirSync, readFileSync, writeFileSync } from "node:fs";
import { join, dirname } from "node:path";
import { fileURLToPath } from "node:url";

const ROOT = join(dirname(fileURLToPath(import.meta.url)), "..");
// Each OS compiles different #[cfg] code, so each one finds different dead
// items: Linux CI keeps its own baseline, Windows keeps dead-code.txt.
const BASELINE_NAME = process.platform === "linux" ? "dead-code-linux.txt" : "dead-code.txt";
const BASELINE = join(ROOT, "scripts", "baselines", BASELINE_NAME);
const FEATURES = "custom-protocol,admin-brain";

const args = new Set(process.argv.slice(2));
const EXTERNAL_API = join(ROOT, "scripts", "baselines", "external-api.txt");

// Guard 0: lib.rs may only publish the reviewed modules. If everything is `pub`
// again, rustc stops reporting unused items and the ratchet below goes blind.
function checkPublicModules() {
  const lib = readFileSync(join(ROOT, "src-tauri", "src", "lib.rs"), "utf8");
  const declared = new Set([...lib.matchAll(/^\s*pub mod ([a-z_0-9]+)\s*;/gm)].map((m) => m[1]));
  const allowed = new Set(
    readFileSync(EXTERNAL_API, "utf8").split("\n").map((l) => l.replace(/#.*/, "").trim()).filter(Boolean),
  );
  const extra = [...declared].filter((m) => !allowed.has(m));
  const stale = [...allowed].filter((m) => !declared.has(m));
  if (extra.length || stale.length) {
    for (const m of extra) console.error(`lib.rs: \`pub mod ${m}\` is not in scripts/baselines/external-api.txt (use pub(crate) mod, or justify it there)`);
    for (const m of stale) console.error(`external-api.txt lists \`${m}\` but lib.rs no longer declares it pub`);
    process.exit(1);
  }
}
checkPublicModules();

function build() {
  // `cargo rustc` passes the flag to the lib crate only (deps are untouched),
  // so this reuses the dependency cache. --profile check = no codegen.
  const r = spawnSync(
    "cargo",
    ["rustc", "--lib", "--profile", "check", "--features", FEATURES, "--message-format=short", "--", "--force-warn", "dead_code"],
    { cwd: join(ROOT, "src-tauri"), encoding: "utf8", maxBuffer: 256 * 1024 * 1024 },
  );
  const text = `${r.stdout ?? ""}\n${r.stderr ?? ""}`;
  if (r.status !== 0 && !/warning:/.test(text)) {
    console.error(text.split("\n").slice(-40).join("\n"));
    console.error("dead-code audit: cargo failed");
    process.exit(2);
  }
  if (/error(\[E\d+\])?:/.test(text)) {
    console.error(text.split("\n").filter((l) => /error/.test(l)).slice(0, 30).join("\n"));
    console.error("dead-code audit: the crate does not compile");
    process.exit(2);
  }
  return text;
}

// "function `foo` is never used", "methods `a` and `b` are never used",
// "associated items `a`, `b`, and `c` are never used", "field `x` is never read",
// "struct `S` is never constructed", "variant `V` is never constructed" ...
function normalise(text) {
  const out = new Set();
  for (const raw of text.split("\n")) {
    const m = /^(?:src[\\/])?(.+?):(\d+):\d+:\s+warning:\s+(.*)$/.exec(raw.trim());
    if (!m) continue;
    const file = m[1].replaceAll("\\", "/").replace(/^src-tauri\//, "").replace(/^src\//, "");
    const msg = m[3];
    if (!/never (used|constructed|read)/.test(msg) && !/dead_code/.test(msg)) continue;
    const names = [...msg.matchAll(/`([^`]+)`/g)].map((x) => x[1]);
    const kind = msg.replace(/`[^`]+`/g, "_").replace(/\s+/g, " ").replace(/ _(,| and|,? and)? ?/g, " ").trim();
    const k = /^(function|method|methods|associated function|associated functions|associated items?|struct|enum|constant|static|field|fields|variant|variants|type alias|trait|module|macro|unused)/.exec(msg)?.[1] ?? "item";
    for (const n of names.length ? names : ["?"]) out.add(`${file}: ${k.replace(/s$/, "")} ${n}`);
    void kind;
  }
  return [...out].sort();
}

const findings = normalise(build());

if (args.has("--list")) {
  for (const f of findings) console.log(f);
  console.log(`${findings.length} dead-code findings`);
  process.exit(0);
}

if (args.has("--update")) {
  mkdirSync(dirname(BASELINE), { recursive: true });
  const header = [
    "# Dead-code baseline (scripts/dead-code-audit.mjs). One finding per line.",
    "# This list may only SHRINK: wire the item or delete it, then run --update.",
    "# Never add a line by hand to silence a new finding.",
  ];
  writeFileSync(BASELINE, [...header, ...findings, ""].join("\n"));
  console.log(`baseline written: ${findings.length} findings -> scripts/baselines/${BASELINE_NAME}`);
  process.exit(0);
}

if (!existsSync(BASELINE)) {
  console.error("no baseline yet; run: node scripts/dead-code-audit.mjs --update");
  process.exit(2);
}
const base = new Set(readFileSync(BASELINE, "utf8").split("\n").filter((l) => l && !l.startsWith("#")));
const cur = new Set(findings);
const added = findings.filter((f) => !base.has(f));
const fixed = [...base].filter((f) => !cur.has(f));

if (added.length > 0) {
  console.error(`dead-code ratchet FAILED: ${added.length} new dead item(s):`);
  for (const a of added) console.error(`  + ${a}`);
  console.error("Wire it into a real path (with a test) or delete it. Do not add #[allow(dead_code)].");
}
if (fixed.length > 0) {
  console.error(`dead-code ratchet: ${fixed.length} baseline item(s) are gone or now used. Shrink the baseline:`);
  for (const f of fixed) console.error(`  - ${f}`);
  console.error("Run: node scripts/dead-code-audit.mjs --update");
}
if (added.length > 0 || fixed.length > 0) process.exit(1);
console.log(`dead-code ratchet OK (${findings.length} known findings, none new)`);

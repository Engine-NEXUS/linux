#!/usr/bin/env node
// IPC + event contract gate (plan part Q1).
//
// Rust commands/events and TypeScript callers are tied together only by string
// literals, so a rename or a half-finished feature silently breaks the app
// (the call is swallowed by `.catch(() => {})`, or the event is never heard).
// This gate cross-checks both sides.
//
// COMMANDS
//   defined (#[tauri::command]) but not in generate_handler!  -> FAIL
//   in generate_handler! but not defined                       -> FAIL
//   invoked from the frontend but not registered               -> FAIL
//   registered but never invoked from the frontend             -> must be in
//     scripts/baselines/ipc-unused-commands.txt (may only shrink)
//
// RUST FILES
//   a .rs file no `mod` declaration reaches from lib.rs/main.rs   -> must be in
//     scripts/baselines/rust-orphan-files.txt (may only shrink); orphans are
//     not compiled, so they are left out of every check below
//
// EVENTS (string literals `ns:name`, `ns_name`, `tts-started`...)
//   emitted by Rust/TS but never listened for anywhere         -> baseline
//   listened for but never emitted anywhere                    -> baseline
//   (scripts/baselines/events-unmatched.txt, may only shrink)
//
// Usage: node scripts/check-ipc-contract.mjs [--update] [--list]
import { existsSync, readdirSync, readFileSync, statSync, writeFileSync, mkdirSync } from "node:fs";
import { join, dirname, relative, basename } from "node:path";
import { fileURLToPath } from "node:url";

const ROOT = join(dirname(fileURLToPath(import.meta.url)), "..");
const BASE_DIR = join(ROOT, "scripts", "baselines");
const UNUSED_FILE = join(BASE_DIR, "ipc-unused-commands.txt");
const EVENTS_FILE = join(BASE_DIR, "events-unmatched.txt");
const ORPHANS_FILE = join(BASE_DIR, "rust-orphan-files.txt");
const args = new Set(process.argv.slice(2));

function walk(dir, exts, skip = [], out = []) {
  if (!existsSync(dir)) return out;
  for (const name of readdirSync(dir)) {
    if (skip.includes(name)) continue;
    const p = join(dir, name);
    const st = statSync(p);
    if (st.isDirectory()) walk(p, exts, skip, out);
    else if (exts.some((e) => name.endsWith(e))) out.push(p);
  }
  return out;
}
const read = (p) => readFileSync(p, "utf8");
const isTest = (p) => /\.(test|spec)\.(ts|tsx)$/.test(p) || /__tests__/.test(p);
const lineOf = (text, idx) => text.slice(0, idx).split("\n").length;

// ── Rust side ────────────────────────────────────────────────────────
// Only files the crate compiles count. A file that no `mod` line reaches is
// not part of the app: its commands cannot be registered and its events never
// fire. Such a file usually means a lost `mod` line (feature 139's
// return_review.rs lost its wiring in a squash), so it is reported, not scanned.
function compiledRustFiles(srcDir) {
  const seen = new Set();
  const visit = (file) => {
    if (seen.has(file) || !existsSync(file)) return;
    seen.add(file);
    const lines = read(file).split("\n");
    const dir = dirname(file);
    const stem = basename(file, ".rs");
    const own = ["lib", "main", "mod"].includes(stem) ? dir : join(dir, stem);
    lines.forEach((line, i) => {
      const m = /^\s*(?:pub(?:\([^)]*\))?\s+)?mod\s+([a-z_0-9]+)\s*;/.exec(line);
      if (!m) return;
      // #[path = "..."] sits among the attribute lines above, relative to dir.
      for (let j = i - 1; j >= 0 && /^\s*#\[/.test(lines[j]); j--) {
        const p = /#\[path\s*=\s*"([^"]+)"\]/.exec(lines[j]);
        if (p) return visit(join(dir, p[1]));
      }
      visit(join(own, `${m[1]}.rs`));
      visit(join(own, m[1], "mod.rs"));
    });
  };
  visit(join(srcDir, "lib.rs"));
  visit(join(srcDir, "main.rs"));
  return seen;
}
const rustSrc = join(ROOT, "src-tauri", "src");
const compiled = compiledRustFiles(rustSrc);
const allRustFiles = walk(rustSrc, [".rs"]);
const rustFiles = allRustFiles.filter((f) => compiled.has(f));
const orphanFiles = allRustFiles
  .filter((f) => !compiled.has(f))
  .map((f) => relative(ROOT, f).replaceAll("\\", "/"))
  .sort();
const defined = new Map(); // name -> "file:line"
for (const f of rustFiles) {
  const text = read(f);
  const lines = text.split("\n");
  lines.forEach((line, i) => {
    if (!/^\s*#\[tauri::command/.test(line)) return;
    for (let j = i + 1; j < Math.min(i + 12, lines.length); j++) {
      const m = /\bfn\s+([a-z_0-9]+)/.exec(lines[j]);
      if (m) {
        defined.set(m[1], `${relative(ROOT, f).replaceAll("\\", "/")}:${j + 1}`);
        break;
      }
    }
  });
}

const libText = read(join(ROOT, "src-tauri", "src", "lib.rs"));
const handlerBlock = /generate_handler!\s*\[([\s\S]*?)\]\s*\)/.exec(libText);
if (!handlerBlock) {
  console.error("ipc contract: could not find generate_handler![...] in lib.rs");
  process.exit(2);
}
const registered = new Set();
for (const raw of handlerBlock[1].split("\n")) {
  const line = raw.replace(/\/\/.*$/, "").trim();
  if (!line) continue;
  for (const part of line.split(",")) {
    const t = part.trim();
    if (!t || t.startsWith("#[")) continue;
    registered.add(t.split("::").pop());
  }
}

// ── Frontend side ────────────────────────────────────────────────────
const feFiles = [
  ...walk(join(ROOT, "frontend", "src"), [".ts", ".tsx", ".js"], ["node_modules"]),
  ...walk(join(ROOT, "frontend"), [".html"], ["node_modules", "dist", "src"]),
].filter((f) => !isTest(f));

const INVOKE_RE = /\b(?:invoke|invokeLogged|tauriInvoke|traceInvoke|safeInvoke)\s*(?:<[^()]*?>)?\s*\(\s*["'`]([a-z_0-9]+)["'`]/g;
const invoked = new Map(); // name -> first "file:line"
for (const f of feFiles) {
  const text = read(f);
  for (const m of text.matchAll(INVOKE_RE)) {
    if (!invoked.has(m[1])) invoked.set(m[1], `${relative(ROOT, f).replaceAll("\\", "/")}:${lineOf(text, m.index)}`);
  }
}

// ── Events ───────────────────────────────────────────────────────────
// Call shapes vary (app.emit, Emitter::emit, emit_logged, emit_to with a window
// label, listen/once, emitLogged...). Find every emit*/listen*/once* call, take
// its first three top-level arguments, and treat the first event-shaped string
// literal among them as the event name. Window labels ("stage"), payload keys
// and CSS class names on the same line are therefore never mistaken for events.
const CALL_RE = /(?:\.|::|\b)(emit[A-Za-z_]*|listen[A-Za-z_]*|once(?:_any)?)\s*(?:<[^()]*?>)?\s*\(/g;
const EVENT_SHAPE = /^[a-z][a-z0-9]*(?:[:\-_][a-z0-9]+)+(?:[:\-_][a-z0-9_\-]+)*$/;
const DOM_EVENT = /^(?:resize|click|keydown|keyup|message|error|load|focus|blur|visibilitychange|storage|beforeunload|unload|pointerdown|pointermove|pointerup|wheel|contextmenu)$/;

const emitters = new Map();
const listeners = new Map();
const addTo = (map, name, where) => {
  if (!map.has(name)) map.set(name, where);
};

function firstArgs(text, start, n, isRust) {
  const args = [];
  let cur = "", depth = 0, q = null;
  for (let i = start; i < text.length && args.length < n; i++) {
    const c = text[i];
    if (q) {
      cur += c;
      if (c === "\\") cur += text[++i] ?? "";
      else if (c === q) q = null;
      continue;
    }
    if (c === '"' || (!isRust && (c === "'" || c === "`"))) { q = c; cur += c; continue; }
    if ("([{".includes(c)) { depth++; cur += c; continue; }
    if (")]}".includes(c)) {
      if (depth === 0) { args.push(cur.trim()); return args; }
      depth--; cur += c; continue;
    }
    if (c === "," && depth === 0) { args.push(cur.trim()); cur = ""; continue; }
    cur += c;
  }
  if (cur.trim()) args.push(cur.trim());
  return args;
}

function scanEvents(files, isRust) {
  for (const f of files) {
    const text = read(f);
    const rel = relative(ROOT, f).replaceAll("\\", "/");
    for (const m of text.matchAll(CALL_RE)) {
      const lineStart = text.lastIndexOf("\n", m.index) + 1;
      const prefix = text.slice(lineStart, m.index).trim();
      if (prefix.startsWith("//") || prefix.startsWith("*")) continue; // commented out
      const args = firstArgs(text, m.index + m[0].length, 3, isRust);
      let name = null;
      for (const a of args) {
        const lit = /^["'`]([^"'`]+)["'`]$/.exec(a);
        if (lit && EVENT_SHAPE.test(lit[1]) && !DOM_EVENT.test(lit[1])) { name = lit[1]; break; }
      }
      if (!name) continue;
      const verb = m[1];
      const where = `${rel}:${lineOf(text, m.index)}`;
      if (/^(?:listen|once)/.test(verb)) addTo(listeners, name, where);
      else addTo(emitters, name, where);
    }
  }
}
scanEvents(rustFiles, true);
scanEvents(feFiles, false);
// Only application events: namespaced ("ns:x"), plus the two legacy hyphenated ones.
const isAppEvent = (n) => /[:_-]/.test(n);
const emittedNeverListened = [...emitters.keys()].filter((n) => isAppEvent(n) && !listeners.has(n)).sort();
const listenedNeverEmitted = [...listeners.keys()].filter((n) => isAppEvent(n) && !emitters.has(n)).sort();

// ── Compare ──────────────────────────────────────────────────────────
const hard = [];
for (const [n, where] of defined) if (!registered.has(n)) hard.push(`command \`${n}\` (${where}) has #[tauri::command] but is not in generate_handler! in lib.rs`);
for (const n of registered) if (!defined.has(n)) hard.push(`generate_handler! lists \`${n}\` but no #[tauri::command] fn has that name`);
for (const [n, where] of invoked) if (!registered.has(n)) hard.push(`frontend invokes \`${n}\` (${where}) but it is not a registered command`);

const unusedCommands = [...registered].filter((n) => !invoked.has(n)).sort();
const eventLines = [
  ...emittedNeverListened.map((n) => `emitted-never-listened ${n}`),
  ...listenedNeverEmitted.map((n) => `listened-never-emitted ${n}`),
];

if (args.has("--list")) {
  console.log(`defined=${defined.size} registered=${registered.size} invoked=${invoked.size}`);
  console.log(`\nregistered but never invoked (${unusedCommands.length}):\n  ${unusedCommands.join("\n  ")}`);
  console.log(`\nevents (${eventLines.length}):\n  ${eventLines.join("\n  ")}`);
  console.log(`\nrust files no mod reaches (${orphanFiles.length}):\n  ${orphanFiles.join("\n  ")}`);
  console.log(hard.length ? `\nHARD FAILURES:\n  ${hard.join("\n  ")}` : "\nno hard failures");
  process.exit(0);
}

const header = (what, fix = "wire it (add a caller/listener) or delete it") => [
  `# ${what} (scripts/check-ipc-contract.mjs). One per line.`,
  `# This list may only SHRINK: ${fix}, then run --update.`,
  "# Never add a line by hand to silence a new finding.",
];
if (args.has("--update")) {
  if (hard.length) {
    console.error("refusing to update the baseline while hard failures exist:\n  " + hard.join("\n  "));
    process.exit(1);
  }
  mkdirSync(BASE_DIR, { recursive: true });
  writeFileSync(UNUSED_FILE, [...header("Registered Tauri commands that no frontend code invokes"), ...unusedCommands, ""].join("\n"));
  writeFileSync(EVENTS_FILE, [...header("Events with only one side"), ...eventLines, ""].join("\n"));
  writeFileSync(ORPHANS_FILE, [...header("Rust files no `mod` declaration reaches (not compiled)", "declare it with `mod` or delete it"), ...orphanFiles, ""].join("\n"));
  console.log(`baselines written: ${unusedCommands.length} unused commands, ${eventLines.length} one-sided events, ${orphanFiles.length} orphan files`);
  process.exit(0);
}

function compare(label, current, file, fix = "add the missing caller/listener, or delete the unused side") {
  if (!existsSync(file)) {
    console.error(`missing baseline ${relative(ROOT, file)}; run --update`);
    process.exit(2);
  }
  const base = new Set(read(file).split("\n").filter((l) => l && !l.startsWith("#")));
  const cur = new Set(current);
  const added = current.filter((c) => !base.has(c));
  const gone = [...base].filter((b) => !cur.has(b));
  if (added.length) {
    hard.push(`${label}: ${added.length} NEW:\n    + ${added.join("\n    + ")}\n    (${fix})`);
  }
  if (gone.length) {
    hard.push(`${label}: ${gone.length} baseline entr${gone.length === 1 ? "y is" : "ies are"} resolved; shrink the baseline:\n    - ${gone.join("\n    - ")}\n    (run: node scripts/check-ipc-contract.mjs --update)`);
  }
}
compare("registered-but-never-invoked commands", unusedCommands, UNUSED_FILE);
compare("one-sided events", eventLines, EVENTS_FILE);
compare("rust files no `mod` declaration reaches", orphanFiles, ORPHANS_FILE, "declare it with `mod`, or delete the file");

if (hard.length) {
  console.error("ipc contract FAILED:");
  for (const h of hard) console.error(`  ${h}`);
  process.exit(1);
}
console.log(`ipc contract OK (${registered.size} commands, ${invoked.size} invoked; ${unusedCommands.length} + ${eventLines.length} known one-sided)`);

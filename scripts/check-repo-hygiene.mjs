#!/usr/bin/env node
// Repo hygiene gate: nothing secret-shaped or machine-local may be TRACKED.
//
//  1. Forbidden tracked paths: databases, logs, key/cert files, .env files.
//  2. Secret-shaped strings in tracked text files (provider key formats with
//     enough length that doc placeholders like `gsk_xxxx` do not match).
//
// Exceptions go in scripts/baselines/hygiene-allow.txt, one per line:
//   path-glob-or-exact | rule-name | reason
// An exception needs a reason; a stale exception (matches nothing) fails too,
// so the allow-list can only shrink.
//
// Usage: node scripts/check-repo-hygiene.mjs   (exit 1 on violation)
import { execFileSync } from "node:child_process";
import { existsSync, readFileSync, statSync } from "node:fs";
import { join, dirname } from "node:path";
import { fileURLToPath } from "node:url";

const ROOT = join(dirname(fileURLToPath(import.meta.url)), "..");
const ALLOW_FILE = join(ROOT, "scripts", "baselines", "hygiene-allow.txt");
const MAX_BYTES = 1_500_000;

const FORBIDDEN_PATHS = [
  { rule: "db-file", re: /\.(db|sqlite|sqlite3)$/i },
  { rule: "log-file", re: /\.log$/i },
  { rule: "key-file", re: /\.(pem|p12|pfx|key|keystore|jks)$/i },
  { rule: "env-file", re: /(^|\/)\.env(\.(?!example$|sample$|template$).+)?$/i },
];

const SECRET_PATTERNS = [
  { rule: "groq-key", re: /\bgsk_[A-Za-z0-9]{40,}\b/ },
  { rule: "google-api-key", re: /\bAIza[0-9A-Za-z_-]{35}\b/ },
  { rule: "github-token", re: /\bgh[pousr]_[A-Za-z0-9]{36,}\b/ },
  { rule: "github-pat", re: /\bgithub_pat_[A-Za-z0-9_]{50,}\b/ },
  { rule: "openai-key", re: /\bsk-(proj-)?[A-Za-z0-9_-]{32,}\b/ },
  { rule: "cerebras-key", re: /\bcsk-[A-Za-z0-9]{32,}\b/ },
  { rule: "aws-access-key", re: /\bAKIA[0-9A-Z]{16}\b/ },
  { rule: "slack-token", re: /\bxox[baprs]-[A-Za-z0-9-]{20,}\b/ },
  { rule: "private-key-block", re: /-----BEGIN (RSA |EC |DSA |OPENSSH |PGP )?PRIVATE KEY( BLOCK)?-----/ },
  { rule: "google-oauth-secret", re: /\bGOCSPX-[A-Za-z0-9_-]{20,}\b/ },
];

// --selftest: prove every rule fires on a synthetic positive and stays quiet on
// doc-style placeholders. Samples are assembled at runtime so this file never
// contains a secret-shaped literal itself.
if (process.argv.includes("--selftest")) {
  const a = (c, n) => c.repeat(n);
  const positives = {
    "groq-key": "gsk_" + a("a", 48),
    "google-api-key": "AIza" + a("b", 35),
    "github-token": "ghp_" + a("c", 36),
    "github-pat": "github_pat_" + a("d", 60),
    "openai-key": "sk-" + a("e", 40),
    "cerebras-key": "csk-" + a("f", 40),
    "aws-access-key": "AKIA" + a("A", 16),
    "slack-token": "xoxb-" + a("1", 24),
    "private-key-block": "-----BEGIN " + "RSA PRIVATE KEY-----",
    "google-oauth-secret": "GOCSPX-" + a("g", 24),
  };
  const negatives = ["gsk_xxxxxxxx", "AIzaSy...", "ghp_your_token_here", "sk-...", "csk-REDACTED", "xoxb-"];
  let bad = 0;
  for (const { rule, re } of SECRET_PATTERNS) {
    if (!re.test(positives[rule] ?? "")) { console.error(`selftest: rule ${rule} did not match its positive sample`); bad++; }
  }
  for (const n of negatives) {
    for (const { rule, re } of SECRET_PATTERNS) if (re.test(n)) { console.error(`selftest: rule ${rule} matched placeholder ${n}`); bad++; }
  }
  const paths = { "db-file": "server/x.db", "log-file": "server/a.log", "key-file": "certs/a.pem", "env-file": "frontend/.env.local" };
  for (const { rule, re } of FORBIDDEN_PATHS) if (!re.test(paths[rule] ?? "")) { console.error(`selftest: path rule ${rule} did not match`); bad++; }
  for (const ok of ["frontend/.env.example", "docs/a.md", "src/main.rs"]) {
    for (const { rule, re } of FORBIDDEN_PATHS) if (re.test(ok)) { console.error(`selftest: path rule ${rule} matched ${ok}`); bad++; }
  }
  if (bad > 0) process.exit(1);
  console.log(`hygiene selftest OK (${SECRET_PATTERNS.length} secret rules, ${FORBIDDEN_PATHS.length} path rules)`);
  process.exit(0);
}

function globToRe(g) {
  const esc = g.replace(/[.+^${}()|[\]\\]/g, "\\$&").replace(/\*\*/g, "\u0000").replace(/\*/g, "[^/]*").replace(/\u0000/g, ".*");
  return new RegExp(`^${esc}$`);
}

function loadAllow() {
  if (!existsSync(ALLOW_FILE)) return [];
  const out = [];
  readFileSync(ALLOW_FILE, "utf8").split("\n").forEach((raw, i) => {
    const line = raw.trim();
    if (!line || line.startsWith("#")) return;
    const [path, rule, ...why] = line.split("|").map((s) => s.trim());
    if (!path || !rule || why.join("|").trim().length < 8) {
      console.error(`hygiene-allow.txt:${i + 1}: expected "path | rule | reason (>= 8 chars)"`);
      process.exit(1);
    }
    out.push({ path, rule, re: globToRe(path), used: false, line: i + 1 });
  });
  return out;
}

const allow = loadAllow();
const isAllowed = (file, rule) => {
  let hit = false;
  for (const a of allow) {
    if ((a.rule === rule || a.rule === "*") && a.re.test(file)) {
      a.used = true;
      hit = true;
    }
  }
  return hit;
};

const tracked = execFileSync("git", ["ls-files", "-z"], { cwd: ROOT, maxBuffer: 64 * 1024 * 1024 })
  .toString("utf8")
  .split("\0")
  .filter(Boolean);

const violations = [];
let scanned = 0;

for (const file of tracked) {
  for (const { rule, re } of FORBIDDEN_PATHS) {
    if (re.test(file) && !isAllowed(file, rule)) violations.push(`${file}: tracked ${rule}`);
  }
  const abs = join(ROOT, file);
  let st;
  try {
    st = statSync(abs);
  } catch {
    continue; // deleted in the working tree
  }
  if (!st.isFile() || st.size === 0 || st.size > MAX_BYTES) continue;
  let text;
  try {
    const buf = readFileSync(abs);
    if (buf.subarray(0, 4000).includes(0)) continue; // binary
    text = buf.toString("utf8");
  } catch {
    continue;
  }
  scanned += 1;
  for (const { rule, re } of SECRET_PATTERNS) {
    const m = re.exec(text);
    if (m && !isAllowed(file, rule)) {
      const line = text.slice(0, m.index).split("\n").length;
      const shown = m[0].length > 12 ? `${m[0].slice(0, 6)}…(${m[0].length} chars)` : m[0];
      violations.push(`${file}:${line}: ${rule} looks committed: ${shown}`);
    }
  }
}

const stale = allow.filter((a) => !a.used);
for (const a of stale) violations.push(`scripts/baselines/hygiene-allow.txt:${a.line}: stale exception (matches nothing): ${a.path} | ${a.rule}`);

if (violations.length > 0) {
  console.error("repo hygiene gate FAILED:");
  for (const v of violations) console.error(`  ${v}`);
  console.error("Fix: untrack the file (git rm --cached) and rotate the secret, or add a justified line to scripts/baselines/hygiene-allow.txt.");
  process.exit(1);
}
console.log(`repo hygiene OK (${tracked.length} tracked files, ${scanned} text files scanned, ${allow.length} exceptions)`);

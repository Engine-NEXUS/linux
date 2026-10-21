#!/usr/bin/env node
// Hand mode assets: copy MediaPipe's SIMD wasm runtime out of node_modules and fetch the Hand
// Landmarker model (both Apache-2.0) into frontend/public/hand/ so the app works offline.
// Those files are git-ignored (about 17 MB); `nexus build` / `nexus dev` run this first.
//
// The model is downloaded once from Google's official bucket and pinned by SHA-256 in
// scripts/hand-assets.lock.json (trust on first download, then verified every run).
//
// Usage: node scripts/sync-hand-assets.mjs [--check]   (--check only reports, never downloads)
import { createHash } from "node:crypto";
import { copyFileSync, existsSync, mkdirSync, readFileSync, writeFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

const ROOT = join(dirname(fileURLToPath(import.meta.url)), "..");
const OUT = join(ROOT, "frontend", "public", "hand");
const WASM_SRC = join(ROOT, "frontend", "node_modules", "@mediapipe", "tasks-vision", "wasm");
const LOCK = join(ROOT, "scripts", "hand-assets.lock.json");
const MODEL_URL =
  "https://storage.googleapis.com/mediapipe-models/hand_landmarker/hand_landmarker/float16/1/hand_landmarker.task";
const MODEL = join(OUT, "hand_landmarker.task");
const WASM_FILES = ["vision_wasm_internal.js", "vision_wasm_internal.wasm"];
const checkOnly = process.argv.includes("--check");

const sha = (p) => createHash("sha256").update(readFileSync(p)).digest("hex");
const readLock = () => (existsSync(LOCK) ? JSON.parse(readFileSync(LOCK, "utf8")) : {});

mkdirSync(join(OUT, "wasm"), { recursive: true });
let ok = true;

for (const f of WASM_FILES) {
  const src = join(WASM_SRC, f);
  if (!existsSync(src)) {
    console.error(`hand assets: ${f} missing in node_modules (run: cd frontend && npm install)`);
    ok = false;
    continue;
  }
  if (!checkOnly) copyFileSync(src, join(OUT, "wasm", f));
}

const lock = readLock();
if (!existsSync(MODEL)) {
  if (checkOnly) {
    console.error("hand assets: hand_landmarker.task missing (run: node scripts/sync-hand-assets.mjs)");
    ok = false;
  } else {
    console.log(`hand assets: downloading hand_landmarker.task (~7.5 MB) from ${new URL(MODEL_URL).host}`);
    const res = await fetch(MODEL_URL);
    if (!res.ok) {
      console.error(`hand assets: download failed (HTTP ${res.status}); hand mode will report the model as missing`);
      process.exit(0); // never break a build over an optional feature
    }
    writeFileSync(MODEL, Buffer.from(await res.arrayBuffer()));
  }
}
if (existsSync(MODEL)) {
  const got = sha(MODEL);
  if (!lock.model) {
    writeFileSync(LOCK, JSON.stringify({ model: got, source: MODEL_URL }, null, 2) + "\n");
    console.log(`hand assets: pinned model sha256 ${got.slice(0, 12)}…`);
  } else if (lock.model !== got) {
    console.error(`hand assets: model hash mismatch (expected ${lock.model.slice(0, 12)}…, got ${got.slice(0, 12)}…); delete the file to re-download`);
    ok = false;
  }
}

console.log(ok ? "hand assets OK" : "hand assets INCOMPLETE");
process.exit(ok || !checkOnly ? 0 : 1);

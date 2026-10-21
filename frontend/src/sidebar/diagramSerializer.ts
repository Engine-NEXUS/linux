// ─── Feature 87: canvas → diagram serialization (pure, unit-tested) ─
// text     → flowchart node      N["label"]
// checkbox → decision diamond   N{"label √|×"}
// arrow    → edge between nearest nodes (endpoint → node center)
// stroke   → comment line (no semantics, preserved for fidelity)

import type { AnnotElement } from "../stage/annotationTypes";

function cleanLabel(s: string | undefined, fallback: string): string {
  const t = (s ?? "").replace(/[\r\n"]+/g, " ").trim();
  const cut = t.length > 40 ? `${t.slice(0, 39)}…` : t;
  return cut || fallback;
}

function center(el: AnnotElement): { x: number; y: number } | null {
  if (el.box) return { x: el.box.x + el.box.w / 2, y: el.box.y + el.box.h / 2 };
  if (el.points && el.points.length > 0) {
    const xs = el.points.map((p) => p.x);
    const ys = el.points.map((p) => p.y);
    return {
      x: (Math.min(...xs) + Math.max(...xs)) / 2,
      y: (Math.min(...ys) + Math.max(...ys)) / 2,
    };
  }
  return null;
}

function dist(a: { x: number; y: number }, b: { x: number; y: number }): number {
  return Math.hypot(a.x - b.x, a.y - b.y);
}

/** Node-worthy elements in draw order with stable N-ids. */
function nodes(els: AnnotElement[]): Array<{ el: AnnotElement; nid: string }> {
  return els
    .filter((el) => el.kind === "text" || el.kind === "checkbox")
    .map((el, i) => ({ el, nid: `N${i + 1}` }));
}

export function toMermaid(els: AnnotElement[]): string {
  const lines = ["flowchart TD"];
  if (els.length === 0) {
    lines.push("    %% empty canvas — nothing to diagram");
    return lines.join("\n");
  }
  const ns = nodes(els);
  for (const { el, nid } of ns) {
    if (el.kind === "text") lines.push(`    ${nid}["${cleanLabel(el.text, "note")}"]`);
    else lines.push(`    ${nid}{"${cleanLabel(el.text, "item")} ${el.checked ? "√" : "×"}"}`);
  }
  let strokeN = 0;
  for (const el of els) {
    if (el.kind === "stroke") {
      strokeN += 1;
      lines.push(`    %% freehand stroke ${strokeN} (${el.points?.length ?? 0} pts)`);
    } else if (el.kind === "arrow" && el.points && el.points.length === 2) {
      const [a, b] = el.points;
      const from = nearestNode(ns, a);
      const to = nearestNode(ns, b, from?.nid);
      if (from && to) lines.push(`    ${from.nid} --> ${to.nid}`);
      else lines.push(`    %% unanchored arrow (${Math.round(a.x)},${Math.round(a.y)}) → (${Math.round(b.x)},${Math.round(b.y)})`);
    }
  }
  return lines.join("\n");
}

function nearestNode(
  ns: Array<{ el: AnnotElement; nid: string }>,
  p: { x: number; y: number },
  exclude?: string,
): { nid: string } | null {
  let best: { nid: string; d: number } | null = null;
  for (const { el, nid } of ns) {
    if (nid === exclude) continue;
    const c = center(el);
    if (!c) continue;
    const d = dist(c, p);
    if (!best || d < best.d) best = { nid, d };
  }
  return best;
}

export function toPlantUml(els: AnnotElement[]): string {
  const lines = ["@startuml", "skinparam monochrome true"];
  if (els.length === 0) {
    lines.push("' empty canvas — nothing to diagram");
    lines.push("@enduml");
    return lines.join("\n");
  }
  const ns = nodes(els);
  for (const { el, nid } of ns) {
    if (el.kind === "text") lines.push(`rectangle "${cleanLabel(el.text, "note")}" as ${nid}`);
    else lines.push(`if (${cleanLabel(el.text, "item")} ${el.checked ? "√" : "×"}) then (yes)\nelse (no)\nendif`);
  }
  for (const el of els) {
    if (el.kind === "stroke") lines.push(`' freehand stroke (${el.points?.length ?? 0} pts)`);
    else if (el.kind === "arrow" && el.points && el.points.length === 2) {
      const [a, b] = el.points;
      const from = nearestNode(ns, a);
      const to = nearestNode(ns, b, from?.nid);
      if (from && to) lines.push(`${from.nid} --> ${to.nid}`);
      else lines.push(`' unanchored arrow (${Math.round(a.x)},${Math.round(a.y)}) → (${Math.round(b.x)},${Math.round(b.y)})`);
    }
  }
  lines.push("@enduml");
  return lines.join("\n");
}

export function toAnnotJson(els: AnnotElement[]): string {
  return JSON.stringify({ version: 1, elements: els }, null, 2);
}

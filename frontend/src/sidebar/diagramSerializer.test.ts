import { describe, expect, it } from "vitest";
import { toAnnotJson, toMermaid, toPlantUml } from "./diagramSerializer";
import type { AnnotElement } from "../stage/annotationTypes";

const text = (id: string, label: string, x: number, y: number): AnnotElement => ({
  id, kind: "text", box: { x, y, w: 220, h: 34 }, text: label, color: "#259ED6",
});
const check = (id: string, label: string, checked: boolean): AnnotElement => ({
  id, kind: "checkbox", box: { x: 500, y: 300, w: 22, h: 22 }, text: label, checked, color: "#259ED6",
});
const arrow = (id: string, from: [number, number], to: [number, number]): AnnotElement => ({
  id, kind: "arrow",
  points: [{ x: from[0], y: from[1] }, { x: to[0], y: to[1] }],
  color: "#259ED6",
});

describe("diagramSerializer", () => {
  it("renders empty canvas as header-only diagrams", () => {
    expect(toMermaid([])).toContain("flowchart TD");
    expect(toMermaid([])).toContain("empty canvas");
    expect(toPlantUml([])).toContain("@startuml");
    expect(toPlantUml([])).toContain("@enduml");
  });

  it("maps text to nodes and checkbox to diamonds with check state", () => {
    const els = [text("a1", "login button", 100, 100), check("a2", "remember me", true)];
    const m = toMermaid(els);
    expect(m).toContain('N1["login button"]');
    expect(m).toContain('N2{"remember me √"}');
  });

  it("marks unchecked boxes with ×", () => {
    const m = toMermaid([check("a1", "tos", false)]);
    expect(m).toContain("×");
  });

  it("connects arrows between nearest nodes", () => {
    const els = [
      text("a1", "start", 100, 100),
      text("a2", "finish", 500, 400),
      arrow("a3", [210, 117], [490, 390]),
    ];
    expect(toMermaid(els)).toContain("N1 --> N2");
    expect(toPlantUml(els)).toContain("N1 --> N2");
  });

  it("keeps unanchored arrows as comments", () => {
    const m = toMermaid([arrow("a1", [10, 10], [900, 900])]);
    expect(m).toContain("unanchored arrow");
  });

  it("preserves strokes as comments", () => {
    const els: AnnotElement[] = [{ id: "a1", kind: "stroke", points: [{ x: 1, y: 1 }, { x: 2, y: 2 }], color: "#fff" }];
    expect(toMermaid(els)).toContain("freehand stroke 1 (2 pts)");
  });

  it("sanitizes quotes and newlines in labels", () => {
    const m = toMermaid([text("a1", 'say "hi"\nnow', 0, 0)]);
    expect(m).not.toContain('"hi"');
    expect(m).not.toContain("\nnow");
  });

  it("round-trips JSON losslessly", () => {
    const els = [text("a1", "x", 1, 2), check("a2", "y", true), arrow("a3", [0, 0], [9, 9])];
    const back = JSON.parse(toAnnotJson(els));
    expect(back.version).toBe(1);
    expect(back.elements).toHaveLength(3);
    expect(back.elements[1].checked).toBe(true);
  });
});

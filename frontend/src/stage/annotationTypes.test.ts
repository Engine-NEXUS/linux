import { describe, expect, it } from "vitest";
import {
  annotationReducer,
  elementHitbox,
  initialAnnotState,
  type AnnotState,
} from "./annotationTypes";

function withStroke(s: AnnotState): AnnotState {
  return annotationReducer(s, {
    type: "add",
    el: { kind: "stroke", points: [{ x: 100, y: 100 }, { x: 200, y: 150 }], color: "#259ED6" },
  });
}

describe("annotationReducer", () => {
  it("adds elements with sequential ids", () => {
    let s = withStroke(initialAnnotState);
    s = annotationReducer(s, {
      type: "add",
      el: { kind: "checkbox", box: { x: 300, y: 300, w: 22, h: 22 }, checked: false, color: "#259ED6" },
    });
    expect(s.elements.map((e) => e.id)).toEqual(["a1", "a2"]);
  });

  it("toggles only checkboxes", () => {
    let s = withStroke(initialAnnotState);
    s = annotationReducer(s, {
      type: "add",
      el: { kind: "checkbox", box: { x: 10, y: 10, w: 22, h: 22 }, checked: false, color: "#259ED6" },
    });
    s = annotationReducer(s, { type: "toggle", id: "a2" });
    expect(s.elements[1].checked).toBe(true);
    s = annotationReducer(s, { type: "toggle", id: "a1" });
    expect(s.elements[0].checked).toBeUndefined();
  });

  it("moves points and boxes by delta", () => {
    let s = withStroke(initialAnnotState);
    s = annotationReducer(s, { type: "move", id: "a1", dx: 10, dy: -5 });
    expect(s.elements[0].points).toEqual([{ x: 110, y: 95 }, { x: 210, y: 145 }]);
  });

  it("undo/redo round-trips adds", () => {
    let s = withStroke(initialAnnotState);
    expect(s.elements).toHaveLength(1);
    s = annotationReducer(s, { type: "undo" });
    expect(s.elements).toHaveLength(0);
    s = annotationReducer(s, { type: "redo" });
    expect(s.elements).toHaveLength(1);
  });

  it("clear empties and undo restores", () => {
    let s = withStroke(initialAnnotState);
    s = annotationReducer(s, { type: "clear" });
    expect(s.elements).toHaveLength(0);
    s = annotationReducer(s, { type: "undo" });
    expect(s.elements).toHaveLength(1);
  });

  it("seed replaces elements and advances seq past imported ids", () => {
    const s = annotationReducer(initialAnnotState, {
      type: "seed",
      elements: [{ id: "a7", kind: "text", box: { x: 1, y: 1, w: 10, h: 10 }, text: "hi", color: "#fff" }],
    });
    expect(s.elements).toHaveLength(1);
    const s2 = annotationReducer(s, {
      type: "add",
      el: { kind: "stroke", points: [{ x: 0, y: 0 }], color: "#fff" },
    });
    expect(s2.elements[1].id).toBe("a8");
  });

  it("append reassigns ids so imports never collide", () => {
    let s = withStroke(initialAnnotState);
    s = annotationReducer(s, {
      type: "append",
      elements: [{ id: "import-0", kind: "text", box: { x: 1, y: 1, w: 10, h: 10 }, text: "pin", color: "#fff" }],
    });
    expect(s.elements.map((e) => e.id)).toEqual(["a1", "a2"]);
    s = annotationReducer(s, { type: "undo" });
    expect(s.elements.map((e) => e.id)).toEqual(["a1"]);
  });

  it("setText updates text payloads", () => {
    let s = annotationReducer(initialAnnotState, {
      type: "add",
      el: { kind: "text", box: { x: 5, y: 5, w: 100, h: 30 }, text: "", color: "#fff" },
    });
    s = annotationReducer(s, { type: "setText", id: "a1", text: "login button" });
    expect(s.elements[0].text).toBe("login button");
  });
});

describe("elementHitbox (physical-px contract)", () => {
  it("returns box coords with pad and NO dpr factor (dpr=1)", () => {
    const hb = elementHitbox({ id: "a1", kind: "checkbox", box: { x: 300, y: 300, w: 22, h: 22 }, color: "#fff" });
    expect(hb).toEqual({ x: 294, y: 294, w: 34, h: 34 });
  });

  it("derives stroke bbox with pad and NO dpr factor (dpr=1.5/2 regression)", () => {
    // Physical 150,150 → 300,225 at any dpr: the builder must echo
    // physical coords, never multiply. (Guards the double-dpr pin bug.)
    const hb = elementHitbox({
      id: "a1", kind: "stroke",
      points: [{ x: 150, y: 150 }, { x: 300, y: 225 }], color: "#fff",
    });
    expect(hb).toEqual({ x: 144, y: 144, w: 162, h: 87 });
  });

  it("returns null for empty elements", () => {
    expect(elementHitbox({ id: "a9", kind: "stroke", color: "#fff" })).toBeNull();
  });
});

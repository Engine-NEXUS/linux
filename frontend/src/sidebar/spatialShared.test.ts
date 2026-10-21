import { describe, expect, it } from "vitest";
import { pinLabel, topicTitle } from "./spatialShared";

describe("pinLabel (Feature 86 stage badge)", () => {
  it("passes short labels through", () => {
    expect(pinLabel("Almonds")).toBe("Almonds");
  });

  it("truncates long labels with an ellipsis", () => {
    const out = pinLabel("California Raw Almonds Premium");
    expect(out.length).toBe(18);
    expect(out.endsWith("…")).toBe(true);
  });

  it("trims whitespace and falls back for empty labels", () => {
    expect(pinLabel("  Cashews  ")).toBe("Cashews");
    expect(pinLabel("   ")).toBe("Element");
  });
});

describe("topicTitle (reference response heading)", () => {
  it("capitalizes and cleans the raw transcript", () => {
    expect(topicTitle("analyse the screen for me")).toBe("Analyse the screen for me");
  });

  it("strips wrapping quotes, brackets and punctuation", () => {
    expect(topicTitle('  "research on pistachios," ')).toBe("Research on pistachios");
    expect(topicTitle("[explain quantum tunneling]")).toBe("Explain quantum tunneling");
  });

  it("collapses whitespace and handles empty input", () => {
    expect(topicTitle("search   for    almonds")).toBe("Search for almonds");
    expect(topicTitle("")).toBe("");
    expect(topicTitle("   ")).toBe("");
  });
});

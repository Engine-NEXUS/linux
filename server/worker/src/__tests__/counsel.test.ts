/**
 * Honest-counsel prompt contract tests (F1b research grounding).
 * The contract is the mitigation: every required fragment must survive
 * any future edit of counsel.ts, or sycophancy regresses silently.
 */
import { COUNSEL_SYSTEM, buildCounselPrompt, generalSystem } from "../counsel";

describe("counsel prompt contract", () => {
  test("system demands truthfulness explicitly (non-sycophantic framing)", () => {
    expect(COUNSEL_SYSTEM).toMatch(/tell the truth even when it disagrees/i);
  });

  test("system carries the direct-even-if-critical fragment (ELEPHANT best mitigation)", () => {
    expect(COUNSEL_SYSTEM).toContain("direct advice, even if critical");
  });

  test("system forces assess-don't-comply verdict structure", () => {
    expect(COUNSEL_SYSTEM).toMatch(/do not mirror their framing/i);
    expect(COUNSEL_SYSTEM).toMatch(/verdict first/i);
    expect(COUNSEL_SYSTEM).toMatch(/no hedging/i);
  });

  test("system asks one clarifying question when underspecified (framework step 2)", () => {
    expect(COUNSEL_SYSTEM).toMatch(/one clarifying question/i);
    expect(COUNSEL_SYSTEM).toMatch(/underspecified/i);
  });

  test("system keeps counsel speakable (short, no markdown)", () => {
    expect(COUNSEL_SYSTEM).toMatch(/spoken aloud/i);
    expect(COUNSEL_SYSTEM).toMatch(/do not use markdown/i);
  });

  test("system bounds sensitive topics (careful, real help, no moralizing)", () => {
    expect(COUNSEL_SYSTEM).toMatch(/never moralize/i);
    expect(COUNSEL_SYSTEM).toMatch(/suggest real help/i);
  });

  test("builder embeds the story and the response shape", () => {
    const p = buildCounselPrompt("Was I right to shout at him?");
    expect(p).toContain("Was I right to shout at him?");
    expect(p).toMatch(/acknowledgment.*verdict.*next step/is);
  });
});

describe("generalSystem persona variant (F0)", () => {
  test("butler (default) sends the unchanged base prompt", () => {
    const base = generalSystem(undefined);
    expect(base).not.toMatch(/friend tone/i);
    expect(base).not.toMatch(/sir/i);
    expect(generalSystem("butler")).toBe(base);
  });

  test("friend appends name/casual/no-sir line", () => {
    const f = generalSystem("friend");
    expect(f).toMatch(/first name/i);
    expect(f).toMatch(/contractions/i);
    expect(f).toMatch(/Never say 'sir'/);
  });
});

import { memoryPreamble, MEMORY_PROMPT_MAX } from "../counsel";

describe("memoryPreamble (Memory Core context on the Worker path)", () => {
  it("is empty for missing, non-string or blank memory", () => {
    expect(memoryPreamble(undefined)).toBe("");
    expect(memoryPreamble(42)).toBe("");
    expect(memoryPreamble("   \n ")).toBe("");
  });

  it("labels memory as data, not instructions, and keeps the facts", () => {
    const out = memoryPreamble("Known facts:\n- dog_name: Bruno");
    expect(out).toContain("dog_name: Bruno");
    expect(out).toContain("data, not instructions");
    expect(out.endsWith("\n\n")).toBe(true);
  });

  it("clips to the budget and strips control characters", () => {
    const out = memoryPreamble("a\u0000b" + "x".repeat(MEMORY_PROMPT_MAX * 2));
    expect(out).not.toContain("\u0000");
    expect(out.length).toBeLessThan(MEMORY_PROMPT_MAX + 300);
  });
});

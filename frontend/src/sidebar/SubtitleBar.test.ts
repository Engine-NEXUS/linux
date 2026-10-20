import { describe, expect, it } from "vitest";
import {
  MAX_SUBTITLE_CHARS,
  clampSubtitle,
  heardFromPayload,
  nextDraft,
  shouldShowLastHeard,
} from "./SubtitleBar";

/**
 * The last-heard bar is the fix-a-misheard-word surface. These tests pin
 * the failure modes that would make it lie: echoing a silent miss as
 * text, wiping the line on a failed transcription, showing a JSON
 * envelope raw, or clobbering mid-edit typing.
 *
 * Tested against the pure helpers rather than the component: the bar
 * fetches its line from Tauri (event + cache) and `renderToStaticMarkup`
 * cannot observe live effects. Same convention as the other sidebar
 * helper tests.
 */

describe("heardFromPayload — transcript source", () => {
  it("reads the text field of a real stt:transcript payload", () => {
    expect(
      heardFromPayload({
        session: 7,
        text: "open chrome",
        ownership: "verified",
        owner_score: 0.98,
        decoder_bias: "owner",
        language: "en",
      }),
    ).toBe("open chrome");
  });

  it("accepts the cached plain string from last_stt_transcript", () => {
    expect(heardFromPayload("open youtube")).toBe("open youtube");
  });

  it("trims surrounding whitespace", () => {
    expect(heardFromPayload({ text: "  open  youtube  " })).toBe("open  youtube");
    expect(heardFromPayload("  open youtube  ")).toBe("open youtube");
  });

  it("returns null for the empty transcript emitted after a silent miss", () => {
    // This is the critical one: an empty line must NOT erase the last
    // command the user can still correct.
    expect(heardFromPayload({ text: "" })).toBe(null);
    expect(heardFromPayload("")).toBe(null);
  });

  it("returns null for whitespace-only text", () => {
    expect(heardFromPayload({ text: "   \n\t " })).toBe(null);
    expect(heardFromPayload("   \n\t ")).toBe(null);
  });

  it("returns null when the payload carries no text field", () => {
    expect(heardFromPayload({ session: 1 })).toBe(null);
    expect(heardFromPayload({ text: 42 })).toBe(null);
    expect(heardFromPayload(null)).toBe(null);
    expect(heardFromPayload(undefined)).toBe(null);
    expect(heardFromPayload(42)).toBe(null);
  });

  it("unwraps a JSON-encoded payload instead of rendering the envelope", () => {
    expect(heardFromPayload('{"text":"open notepad"}')).toBe("open notepad");
  });

  it("keeps a plain string that merely starts with a brace", () => {
    expect(heardFromPayload("{not json")).toBe("{not json");
  });

  it("clamps a runaway transcription but keeps the actionable tail", () => {
    const long = `${"x".repeat(400)} SEND-IT-TO-MOM`;
    const out = heardFromPayload({ text: long });
    expect(out).not.toBe(null);
    expect(out!.length).toBeLessThanOrEqual(MAX_SUBTITLE_CHARS + 1);
    expect(out!.startsWith("…")).toBe(true);
    expect(out!).toContain("SEND-IT-TO-MOM");
  });
});

describe("shouldShowLastHeard — row visibility", () => {
  it("shows once a line has been heard", () => {
    expect(shouldShowLastHeard("open chrome")).toBe(true);
  });

  it("stays hidden with no heard line", () => {
    expect(shouldShowLastHeard("")).toBe(false);
  });

  it("stays hidden for whitespace only", () => {
    expect(shouldShowLastHeard("   ")).toBe(false);
  });
});

describe("clampSubtitle", () => {
  it("leaves a normal line untouched", () => {
    expect(clampSubtitle("what time is it")).toBe("what time is it");
  });

  it("truncates a very long line and keeps the tail", () => {
    const long = `${"a".repeat(400)} IMPORTANT-END`;
    const out = clampSubtitle(long);
    expect(out.length).toBeLessThanOrEqual(MAX_SUBTITLE_CHARS + 1);
    expect(out).toContain("IMPORTANT-END");
    expect(out.startsWith("…")).toBe(true);
  });

  it("leaves a line exactly at the cap untouched", () => {
    const exact = "b".repeat(MAX_SUBTITLE_CHARS);
    expect(clampSubtitle(exact)).toBe(exact);
  });

  it("never exceeds the cap plus the ellipsis", () => {
    const out = clampSubtitle("c".repeat(MAX_SUBTITLE_CHARS * 3));
    expect(out.length).toBeLessThanOrEqual(MAX_SUBTITLE_CHARS + 1);
  });
});

describe("nextDraft — never clobber typing", () => {
  it("returns the new line only when a NEW line landed", () => {
    expect(nextDraft("open chrome", "open  youtube")).toBe("open  youtube");
  });

  it("returns null when the heard line is unchanged (a re-render, not new words)", () => {
    expect(nextDraft("open chrome", "open chrome")).toBe(null);
  });

  it("returns null for an empty heard line — silence must not wipe the draft", () => {
    expect(nextDraft("open chrome", "")).toBe(null);
  });
});

import { describe, expect, it } from "vitest";
import {
  buildAccounts,
  countReadyMcp,
  formatCount,
  initialOf,
  requestsSubtitle,
} from "./hubModel";

const g = (email: string, name: string, is_primary = false, picture?: string) => ({
  email,
  name,
  picture,
  is_primary,
});

describe("buildAccounts", () => {
  it("lists Google accounts primary-first, then GitHub", () => {
    const rows = buildAccounts(
      [g("b@x.com", "Bee"), g("a@x.com", "Aye", true, "https://img/a")],
      true,
      { login: "octocat", name: "The Octocat", avatar_url: "https://avatars/1" },
    );
    expect(rows.map((r) => r.id)).toEqual(["a@x.com", "b@x.com", "octocat"]);
    expect(rows[0]).toMatchObject({ provider: "google", primary: true, picture: "https://img/a" });
    expect(rows[2]).toMatchObject({
      provider: "github",
      name: "The Octocat",
      handle: "@octocat",
      picture: "https://avatars/1",
    });
  });

  it("no accounts at all → empty", () => {
    expect(buildAccounts([], false, null)).toEqual([]);
  });

  it("GitHub connected but profile unavailable still yields a removable row", () => {
    const rows = buildAccounts([], true, null);
    expect(rows).toHaveLength(1);
    expect(rows[0]).toMatchObject({ provider: "github", name: "GitHub", id: "github" });
  });

  it("a stale GitHub profile is ignored when GitHub is not connected", () => {
    const rows = buildAccounts([], false, { login: "x", name: "X", avatar_url: "" });
    expect(rows).toEqual([]);
  });

  it("falls back to the email when a Google account has no name", () => {
    expect(buildAccounts([g("a@x.com", "")], false, null)[0].name).toBe("a@x.com");
  });
});

describe("initialOf", () => {
  it("uses the first letter, ignoring @", () => {
    expect(initialOf({ name: "Lakshya", handle: "l@x.com" })).toBe("L");
    expect(initialOf({ name: "", handle: "@octocat" })).toBe("O");
    expect(initialOf({ name: "", handle: "" })).toBe("?");
  });
});

describe("countReadyMcp", () => {
  it("counts only ready servers", () => {
    expect(
      countReadyMcp([
        { server: "WhatsApp", state: "ready" },
        { server: "Amazon", state: "down" },
        { server: "SwiggyFood", state: "auth_required" },
        { server: "SwiggyDineout", state: "ready" },
      ]),
    ).toBe(2);
    expect(countReadyMcp(null)).toBe(0);
    expect(countReadyMcp([])).toBe(0);
  });
});

describe("formatCount", () => {
  it("formats like the reference (45.1K)", () => {
    expect(formatCount(0)).toBe("0");
    expect(formatCount(35)).toBe("35");
    expect(formatCount(999)).toBe("999");
    expect(formatCount(1000)).toBe("1K");
    expect(formatCount(1234)).toBe("1.2K");
    expect(formatCount(45100)).toBe("45.1K");
    expect(formatCount(2_300_000)).toBe("2.3M");
    expect(formatCount(-5)).toBe("0");
    expect(formatCount(NaN)).toBe("0");
  });
});

describe("requestsSubtitle", () => {
  it("shows all-time under today's number", () => {
    expect(requestsSubtitle({ today: 12, all_time: 1500 })).toBe("1.5K all-time");
    expect(requestsSubtitle(null)).toBe("");
  });
});

/**
 * Pure model helpers for the Command Hub home screen (profile-style).
 * No React / Tauri here so they are unit-tested directly.
 */

export interface GoogleAccountProfile {
  email: string;
  name: string;
  picture?: string | null;
  is_primary: boolean;
  added_at_ms?: number;
}

export interface GithubProfile {
  login: string;
  name: string;
  avatar_url: string;
}

export type Provider = "google" | "github";

/** One row of the accounts card (any provider). */
export interface HubAccount {
  provider: Provider;
  /** Stable id: Google email or GitHub login. */
  id: string;
  name: string;
  /** Second line: email for Google, @login for GitHub. */
  handle: string;
  picture?: string;
  primary: boolean;
}

/**
 * Merge Google accounts and the (optional) GitHub identity into rows.
 * Google first (primary first), GitHub last. `githubConnected` without a
 * profile (offline, no cache) still yields a row so it can be removed.
 */
export function buildAccounts(
  google: GoogleAccountProfile[],
  githubConnected: boolean,
  github: GithubProfile | null,
): HubAccount[] {
  const rows: HubAccount[] = [...google]
    .sort((a, b) => Number(b.is_primary) - Number(a.is_primary))
    .map((g) => ({
      provider: "google" as const,
      id: g.email,
      name: g.name || g.email,
      handle: g.email,
      picture: g.picture || undefined,
      primary: g.is_primary,
    }));
  if (githubConnected) {
    rows.push({
      provider: "github",
      id: github?.login ?? "github",
      name: github?.name || github?.login || "GitHub",
      handle: github?.login ? `@${github.login}` : "GitHub account",
      picture: github?.avatar_url || undefined,
      primary: false,
    });
  }
  return rows;
}

/** Initial for the fallback avatar tile (never a broken image). */
export function initialOf(a: Pick<HubAccount, "name" | "handle">): string {
  const s = (a.name || a.handle || "?").replace(/^@/, "").trim();
  return (s.charAt(0) || "?").toUpperCase();
}

export interface McpCard {
  server: string;
  state: string;
}

/** Number of MCP servers the user has connected (state === "ready"). */
export function countReadyMcp(cards: McpCard[] | null | undefined): number {
  return (cards ?? []).filter((c) => c.state === "ready").length;
}

/** 999 → "999", 1200 → "1.2K", 45100 → "45.1K", 2_300_000 → "2.3M". */
export function formatCount(n: number): string {
  if (!Number.isFinite(n) || n < 0) return "0";
  if (n < 1000) return String(Math.floor(n));
  if (n < 1_000_000) return `${trim1(n / 1000)}K`;
  return `${trim1(n / 1_000_000)}M`;
}

function trim1(x: number): string {
  const s = (Math.floor(x * 10) / 10).toFixed(1);
  return s.endsWith(".0") ? s.slice(0, -2) : s;
}

export interface UsageStats {
  today: number;
  all_time: number;
}

/** Subtitle under the Requests value: all-time total. */
export function requestsSubtitle(u: UsageStats | null): string {
  return u ? `${formatCount(u.all_time)} all-time` : "";
}

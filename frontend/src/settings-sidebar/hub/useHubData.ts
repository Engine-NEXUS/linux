import { useCallback, useEffect, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import {
  connectOAuth,
  disconnectOAuth,
  getOAuthStatus,
  setSidecarBaseUrl,
} from "../../setup/oauth";
import {
  buildAccounts,
  countReadyMcp,
  type GithubProfile,
  type GoogleAccountProfile,
  type HubAccount,
  type McpCard,
  type UsageStats,
} from "./hubModel";

const msg = (e: unknown) => (e instanceof Error ? e.message : String(e));

/** Accounts card data + actions (Google multi-account + GitHub). */
export function useAccounts(serverUrl: string, userId: string) {
  const [google, setGoogle] = useState<GoogleAccountProfile[]>([]);
  const [googleNeedsSignin, setGoogleNeedsSignin] = useState(false);
  const [githubConnected, setGithubConnected] = useState(false);
  const [github, setGithub] = useState<GithubProfile | null>(null);
  const [busy, setBusy] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);
  const uid = userId || "local-user";

  const reload = useCallback(async () => {
    try {
      setGoogle(await invoke<GoogleAccountProfile[]>("google_get_accounts"));
    } catch (e) {
      console.warn("google accounts", e);
    }
    try {
      const ms = await invoke<any>("memcore_status");
      if (ms && typeof ms.google_needs_signin === "boolean") {
        setGoogleNeedsSignin(ms.google_needs_signin);
      }
    } catch {
      /* memcore optional */
    }
    if (!serverUrl) return;
    setSidecarBaseUrl(serverUrl);
    try {
      const status = await getOAuthStatus(uid);
      const connected = status.github?.connected ?? false;
      setGithubConnected(connected);
      if (connected) {
        invoke<GithubProfile>("github_profile", { serverUrl, userId: uid })
          .then(setGithub)
          .catch(() => setGithub(null));
      } else {
        setGithub(null);
      }
    } catch {
      /* offline: keep what we have */
    }
  }, [serverUrl, uid]);

  useEffect(() => {
    void reload();
  }, [reload]);

  const run = async (label: string, fn: () => Promise<void>) => {
    setError(null);
    setBusy(label);
    try {
      await fn();
      await reload();
    } catch (e) {
      setError(msg(e));
    } finally {
      setBusy(null);
    }
  };

  return {
    accounts: buildAccounts(google, githubConnected, github) as HubAccount[],
    googleNeedsSignin,
    busy,
    error,
    dismissError: () => setError(null),
    addGoogle: () => run("add-google", () => invoke("google_connect_account").then(() => undefined)),
    addGithub: () =>
      run("add-github", async () => {
        if (!serverUrl) throw new Error("Server URL not configured");
        setSidecarBaseUrl(serverUrl);
        await connectOAuth("github", uid);
      }),
    makePrimary: (email: string) =>
      run("primary", () => invoke("google_set_primary_account", { email }).then(() => undefined)),
    /** Removes the account from NEXUS only (never deletes it at the provider). */
    remove: (a: HubAccount) =>
      run(`remove-${a.provider}-${a.id}`, async () => {
        if (a.provider === "google") {
          await invoke("google_disconnect_account", { email: a.id });
        } else {
          await disconnectOAuth(uid, "github");
          await invoke("github_profile_clear");
          setGithub(null);
          setGithubConnected(false);
        }
      }),
  };
}

/** Insights: API key count, connected MCPs (async probe), request counts. */
export function useInsights(refreshKey: number) {
  const [keyCount, setKeyCount] = useState<number | null>(null);
  const [usage, setUsage] = useState<UsageStats | null>(null);
  const [mcp, setMcp] = useState<number | null>(null);
  const alive = useRef(true);

  useEffect(() => {
    alive.current = true;
    invoke<number>("api_keys_count").then((n) => alive.current && setKeyCount(n)).catch(() => {});
    invoke<UsageStats>("get_usage_stats").then((u) => alive.current && setUsage(u)).catch(() => {});
    return () => {
      alive.current = false;
    };
  }, [refreshKey]);

  // The MCP probe can take ~5 s: never block the screen on it.
  useEffect(() => {
    let live = true;
    invoke<McpCard[]>("mcp_connect_state")
      .then((cards) => live && setMcp(countReadyMcp(cards)))
      .catch(() => live && setMcp(0));
    return () => {
      live = false;
    };
  }, []);

  return { keyCount, usage, mcp };
}

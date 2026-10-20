import { beforeEach, describe, expect, it, vi } from "vitest";

const invokeMock = vi.fn();
const listenMock = vi.fn();
const openMock = vi.fn();

vi.mock("@tauri-apps/api/core", () => ({
  invoke: (...args: unknown[]) => invokeMock(...args),
}));

vi.mock("@tauri-apps/api/event", () => ({
  listen: (...args: unknown[]) => listenMock(...args),
}));

vi.mock("@tauri-apps/plugin-shell", () => ({
  open: (...args: unknown[]) => openMock(...args),
}));

import {
  __testResetOAuthState,
  connectOAuth,
  disconnectOAuth,
  getOAuthStatus,
  handleOAuthRedirect,
  setSidecarBaseUrl,
  workerCall,
} from "./oauth";

describe("OAuth and API Key Worker Proxy Migration (Job 3)", () => {
  beforeEach(() => {
    vi.clearAllMocks();
    __testResetOAuthState();
    listenMock.mockResolvedValue(() => {});
    openMock.mockResolvedValue(undefined);
  });

  describe("workerCall", () => {
    it("routes through invoke('worker_request', ...) with exact parameters", async () => {
      invokeMock.mockResolvedValueOnce({ ok: true });

      const result = await workerCall("GET", "/oauth/status", {
        query: "foo=bar",
        body: { test: 123 },
      });

      expect(invokeMock).toHaveBeenCalledWith("worker_request", {
        method: "GET",
        path: "/oauth/status",
        query: "foo=bar",
        body: { test: 123 },
      });
      expect(result).toEqual({ ok: true });
    });

    it("defaults omitted query and body to null", async () => {
      invokeMock.mockResolvedValueOnce({ providers: {} });

      await workerCall("GET", "/oauth/status");

      expect(invokeMock).toHaveBeenCalledWith("worker_request", {
        method: "GET",
        path: "/oauth/status",
        query: null,
        body: null,
      });
    });

    it("propagates proxy errors truthfully", async () => {
      invokeMock.mockRejectedValueOnce(
        new Error("This device isn't authorized with the NEXUS server")
      );

      await expect(workerCall("GET", "/oauth/status")).rejects.toThrow(
        "This device isn't authorized with the NEXUS server"
      );
    });
  });

  describe("getOAuthStatus (GET /oauth/status)", () => {
    it("invokes worker_request GET /oauth/status without user_id", async () => {
      invokeMock.mockResolvedValueOnce({
        providers: {
          google: { connected: true, expired: false, scopes: "email profile" },
          github: { connected: false, expired: false, scopes: "" },
        },
      });

      const status = await getOAuthStatus("legacy-user-123");

      expect(invokeMock).toHaveBeenCalledTimes(1);
      const call = invokeMock.mock.calls[0];
      expect(call[0]).toBe("worker_request");
      expect(call[1]).toEqual({
        method: "GET",
        path: "/oauth/status",
        query: null,
        body: null,
      });
      // Verification: renderer does NOT send user_id
      expect(JSON.stringify(call[1])).not.toContain("legacy-user-123");

      expect(status.google.connected).toBe(true);
      expect(status.github.connected).toBe(false);
    });

    it("returns empty object on network / auth rejection", async () => {
      invokeMock.mockRejectedValueOnce(new Error("Connection failed"));

      const status = await getOAuthStatus();
      expect(status).toEqual({});
    });
  });

  describe("disconnectOAuth (DELETE /oauth/disconnect)", () => {
    it("invokes worker_request DELETE /oauth/disconnect with provider and no user_id", async () => {
      invokeMock.mockResolvedValueOnce({ ok: true });

      await disconnectOAuth("user-to-ignore", "google");

      expect(invokeMock).toHaveBeenCalledTimes(1);
      const [cmd, payload] = invokeMock.mock.calls[0];
      expect(cmd).toBe("worker_request");
      expect(payload).toEqual({
        method: "DELETE",
        path: "/oauth/disconnect",
        query: null,
        body: { provider: "google" },
      });
      expect(JSON.stringify(payload)).not.toContain("user-to-ignore");
    });
  });

  describe("connectOAuth (GET /oauth/auth-url & POST /oauth/exchange)", () => {
    it("requires serverUrl to be configured before starting flow", async () => {
      await expect(connectOAuth("google", "user-1")).rejects.toThrow(
        "Server URL not configured"
      );
    });

    it("requests auth-url via worker_request GET /oauth/auth-url without user_id", async () => {
      setSidecarBaseUrl("https://nexus-worker.test");
      invokeMock.mockResolvedValueOnce({
        url: "https://accounts.google.com/o/oauth2/v2/auth?client_id=xyz",
      });

      const openedCb = vi.fn();
      const connectPromise = connectOAuth("google", "user-123", openedCb);

      // Wait a tick for auth-url invoke to execute
      await new Promise((r) => setTimeout(r, 0));

      expect(invokeMock).toHaveBeenCalledTimes(1);
      const [cmd, payload] = invokeMock.mock.calls[0];
      expect(cmd).toBe("worker_request");
      expect(payload.method).toBe("GET");
      expect(payload.path).toBe("/oauth/auth-url");
      expect(payload.query).toContain("provider=google");
      expect(payload.query).toContain("code_challenge=");
      // Ensure user_id was NOT sent in the query
      expect(payload.query).not.toContain("user_id");
      expect(JSON.stringify(payload)).not.toContain("user-123");

      expect(openMock).toHaveBeenCalledWith(
        "https://accounts.google.com/o/oauth2/v2/auth?client_id=xyz"
      );
      expect(openedCb).toHaveBeenCalledTimes(1);

      // Simulate browser callback redirect with code
      invokeMock.mockResolvedValueOnce({ ok: true });
      await handleOAuthRedirect("nexus://oauth/callback?code=test-code-123&state=state-456");

      const completed = await connectPromise;
      expect(completed).toBe(true);

      // Check the second invoke: POST /oauth/exchange
      expect(invokeMock).toHaveBeenCalledTimes(2);
      const [exchangeCmd, exchangePayload] = invokeMock.mock.calls[1];
      expect(exchangeCmd).toBe("worker_request");
      expect(exchangePayload.method).toBe("POST");
      expect(exchangePayload.path).toBe("/oauth/exchange");
      expect(exchangePayload.body).toEqual({
        provider: "google",
        code: "test-code-123",
        code_verifier: expect.any(String),
        redirect_uri: "https://nexus-worker.test/oauth/callback",
        state: "state-456",
      });
      // Ensure user_id was NOT sent in the body
      expect(JSON.stringify(exchangePayload)).not.toContain("user-123");
    });

    it("rejects if auth-url endpoint returns an error or no URL", async () => {
      setSidecarBaseUrl("https://nexus-worker.test");
      invokeMock.mockResolvedValueOnce({ error: "Provider not supported" });

      await expect(connectOAuth("swiggy", "user-1")).rejects.toThrow(
        "Provider not supported"
      );
    });

    it("resolves connectOAuth when handleOAuthRedirect receives status=success", async () => {
      setSidecarBaseUrl("https://nexus-worker.test");
      invokeMock.mockResolvedValueOnce({
        url: "https://accounts.google.com/o/oauth2/v2/auth?client_id=xyz",
      });

      const connectPromise = connectOAuth("google", "user-123");
      await new Promise((r) => setTimeout(r, 0));

      await handleOAuthRedirect("nexus://oauth/callback?status=success");

      const res = await connectPromise;
      expect(res).toBe(true);
    });

    it("rejects connectOAuth when handleOAuthRedirect receives error param", async () => {
      setSidecarBaseUrl("https://nexus-worker.test");
      invokeMock.mockResolvedValueOnce({
        url: "https://github.com/login/oauth/authorize?client_id=xyz",
      });

      const connectPromise = connectOAuth("github", "user-1");
      await new Promise((r) => setTimeout(r, 0));

      await handleOAuthRedirect("nexus://oauth/callback?error=access_denied");

      await expect(connectPromise).rejects.toThrow("OAuth error: access_denied");
    });

    it("rejects connectOAuth when /oauth/exchange fails", async () => {
      setSidecarBaseUrl("https://nexus-worker.test");
      invokeMock.mockResolvedValueOnce({
        url: "https://github.com/login/oauth/authorize?client_id=xyz",
      });

      const connectPromise = connectOAuth("github", "user-1");
      await new Promise((r) => setTimeout(r, 0));

      invokeMock.mockResolvedValueOnce({ error: "Invalid grant" });
      await handleOAuthRedirect("nexus://oauth/callback?code=bad-code");

      await expect(connectPromise).rejects.toThrow("Invalid grant");
    });

    it("completes via active status polling if browser redirect is missed", async () => {
      vi.useFakeTimers();
      try {
        setSidecarBaseUrl("https://nexus-worker.test");
        invokeMock.mockResolvedValueOnce({
          url: "https://accounts.google.com/o/oauth2/v2/auth?client_id=xyz",
        });

        const connectPromise = connectOAuth("google", "user-123");
        await vi.advanceTimersByTimeAsync(10);

        // Next status check returns connected: true
        invokeMock.mockResolvedValueOnce({
          providers: { google: { connected: true, expired: false, scopes: "" } },
        });

        await vi.advanceTimersByTimeAsync(1500);
        const res = await connectPromise;
        expect(res).toBe(true);
      } finally {
        vi.useRealTimers();
      }
    });
  });
});

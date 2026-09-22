import { useState, useEffect, useCallback } from "react";
import { invoke } from "@tauri-apps/api/core";
import {
  setSidecarBaseUrl,
  getOAuthStatus,
  connectOAuth,
  disconnectOAuth,
  type OAuthStatus,
} from "../setup/oauth";

/**
 * NEXUS Settings Sidebar
 *
 * Liquid-glass sidebar (720x1000) that controls the entire application:
 *   - Display: orb position sliders, orb size slider, live preview
 *   - Audio: TTS volume slider, test button
 *   - Auth: Google/GitHub OAuth connect/disconnect, Gemini/Groq API keys
 *
 * Persists to settings.json via save_settings Tauri command.
 * Overwrites on each save (std::fs::write).
 */

type Tab = "display" | "audio" | "auth" | "connections";

interface Settings {
  autostart: boolean;
  hotkey: string;
  autoHideDelay: number;
  wakeWordEnabled: boolean;
  wakePhrase: string;
  wakeSensitivity: string;
  speakerVerification: boolean;
  meetingModeAuto: boolean;
  suppressTtsInMeetings: boolean;
  localSttOnly: boolean;
  serverUrl: string;
  userId: string;
  deviceId: string;
  ttsVoice: string;
  speechRate: number;
  ttsVolume: number;
  ttsProvider: string;
  groqApiKey: string;
  edgeTtsVoice: string;
  // Orb position + size (Phase 2)
  orbHorizontalPct?: number;
  orbVerticalPct?: number;
  orbSize?: number;
  // Gemini API key (Phase 3)
  geminiApiKey?: string;
  // Cerebras API key (9Router — fastest free provider, ~80ms)
  cerebrasApiKey?: string;
  // Moonshine STT model (medium_streaming=6.65% WER, small_streaming=7.84%)
  moonshineModel?: string;
  // Telegram owner chat id (remote bridge; token lives in vault)
  telegramChatId?: string;
}

const DEFAULT_SETTINGS: Settings = {
  autostart: true,
  hotkey: "Ctrl+Space",
  autoHideDelay: 8,
  wakeWordEnabled: true,
  wakePhrase: "NEXUS",
  wakeSensitivity: "medium",
  speakerVerification: false,
  meetingModeAuto: true,
  suppressTtsInMeetings: true,
  localSttOnly: true,
  serverUrl: "",
  userId: "",
  deviceId: "",
  ttsVoice: "af_sky",
  speechRate: 1.15,
  ttsVolume: 75,
  ttsProvider: "kokoro",
  groqApiKey: "",
  edgeTtsVoice: "en-US-AvaNeural",
  orbHorizontalPct: 0.5,
  orbVerticalPct: 1.0,
  orbSize: 200,
  geminiApiKey: "",
  cerebrasApiKey: "",
  moonshineModel: "medium_streaming",
  telegramChatId: "",
};

const TABS: { id: Tab; label: string }[] = [
  { id: "display", label: "Display" },
  { id: "audio", label: "Audio" },
  { id: "auth", label: "Accounts" },
  { id: "connections", label: "Connections" },
];

export function SettingsSidebarApp() {
  const [tab, setTab] = useState<Tab>("display");
  const [settings, setSettings] = useState<Settings>(DEFAULT_SETTINGS);
  const [saved, setSaved] = useState(false);

  // Load settings from Rust on mount
  useEffect(() => {
    invoke<Partial<Settings>>("get_settings").then((s) => {
      if (s) setSettings({ ...DEFAULT_SETTINGS, ...s });
    }).catch(() => {});
  }, []);

  // Show the orb window when the Display tab is active so the user can
  // see it move live while dragging the position sliders.
  useEffect(() => {
    if (tab === "display") {
      invoke("show_overlay").catch(() => {});
    }
  }, [tab]);

  // Fetch pending backdrop on mount — handles the fresh-window case where
  // the backdrop was captured before the React app loaded (same pattern as
  // the response sidebar's get_pending_sidebar_content).
  useEffect(() => {
    invoke<string | null>("get_pending_settings_backdrop")
      .then((backdrop) => {
        if (backdrop && backdrop.startsWith("data:image/")) {
          document.documentElement.style.setProperty(
            "--sidebar-backdrop-image",
            `url("${backdrop}")`,
          );
        }
      })
      .catch(() => {});
  }, []);

  // Listen for live sidebar:backdrop events — Rust captures the desktop
  // behind the window every 1s, blurs it, and sends it as a data URI.
  // We set it as a CSS variable on <html> so the ::after layer renders it.
  useEffect(() => {
    let unlisten: (() => void) | null = null;
    (async () => {
      const { listen } = await import("@tauri-apps/api/event");
      unlisten = await listen<string>("sidebar:backdrop", (ev) => {
        const dataUri = ev.payload;
        if (typeof dataUri === "string" && dataUri.startsWith("data:image/")) {
          document.documentElement.style.setProperty("--sidebar-backdrop-image", `url("${dataUri}")`);
        }
      });
    })();
    return () => { unlisten?.(); };
  }, []);

  // Ctrl+Space to close (same as other sidebars)
  useEffect(() => {
    const handler = (e: KeyboardEvent) => {
      if (e.code === "Space" && (e.ctrlKey || e.metaKey)) {
        e.preventDefault();
        invoke("hide_settings_sidebar").catch(() => {});
      }
    };
    window.addEventListener("keydown", handler);
    return () => window.removeEventListener("keydown", handler);
  }, []);

  const update = useCallback(<K extends keyof Settings>(key: K, value: Settings[K]) => {
    setSettings((prev) => ({ ...prev, [key]: value }));
    setSaved(false);
  }, []);

  const handleSave = async () => {
    try {
      await invoke("save_settings", { settings });
      setSaved(true);
      setTimeout(() => setSaved(false), 2000);
    } catch (e) {
      console.error("save_settings failed:", e);
    }
  };

  const handleReset = () => {
    setSettings(DEFAULT_SETTINGS);
    setSaved(false);
  };

  return (
    <div className="settings-container">
      {/* Header */}
      <div className="settings-header">
        <span className="settings-title">NEXUS Settings</span>
        <span className="settings-hint">Ctrl+Space to close</span>
      </div>

      {/* Tab bar */}
      <div className="settings-tabs">
        {TABS.map((t) => (
          <button
            key={t.id}
            className={`settings-tab ${tab === t.id ? "settings-tab--active" : ""}`}
            onClick={() => setTab(t.id)}
          >
            {t.label}
          </button>
        ))}
      </div>

      {/* Scrollable content */}
      <div className="settings-scroll">
        {tab === "display" && <DisplayTab settings={settings} update={update} />}
        {tab === "audio" && <AudioTab settings={settings} update={update} />}
        {tab === "auth" && <AuthTab settings={settings} update={update} />}
        {tab === "connections" && <ConnectionsTab settings={settings} update={update} />}
      </div>

      {/* Footer */}
      <div className="settings-footer">
        <span className={`settings-saved-indicator ${saved ? "settings-saved-indicator--visible" : ""}`}>
          ✓ Saved
        </span>
        <div className="settings-footer-actions">
          <button className="settings-btn" onClick={handleReset}>Reset</button>
          <button className="settings-btn settings-btn--primary" onClick={handleSave}>
            Save
          </button>
        </div>
      </div>
    </div>
  );
}

// ─── Display Tab (Phase 2 — orb position + size) ──────────────────────
function DisplayTab({ settings, update }: {
  settings: Settings;
  update: <K extends keyof Settings>(key: K, value: Settings[K]) => void;
}) {
  const hPct = settings.orbHorizontalPct ?? 0.5;
  const vPct = settings.orbVerticalPct ?? 1.0;
  const orbSize = settings.orbSize ?? 200;

  // Live update — move orb immediately without saving
  const liveUpdate = (h: number, v: number, size: number) => {
    invoke("set_orb_position", {
      horizontalPct: h,
      verticalPct: v,
      size,
    }).catch(() => {});
  };

  return (
    <>
      <div className="settings-section">
        <div className="settings-section-title">Orb Position</div>

        {/* Live preview — shows the orb position and breathing animation */}
        <div className="position-preview">
          <div
            className="position-preview-orb"
            style={{
              left: `${hPct * 100}%`,
              top: `${vPct * 100}%`,
              width: `${Math.min(orbSize / 200 * 40, 50)}px`,
              height: `${Math.min(orbSize / 200 * 40, 50)}px`,
            }}
          />
        </div>

        <div className="setting-row">
          <div>
            <div className="setting-label">Horizontal</div>
            <div className="setting-desc">Left ↔ Right</div>
          </div>
          <div className="setting-control">
            <input
              type="range"
              className="settings-slider"
              min={0}
              max={100}
              value={Math.round(hPct * 100)}
              onChange={(e) => {
                const pct = parseInt(e.target.value) / 100;
                update("orbHorizontalPct", pct);
                liveUpdate(pct, vPct, orbSize);
              }}
            />
            <span className="slider-value">{Math.round(hPct * 100)}%</span>
          </div>
        </div>

        <div className="setting-row">
          <div>
            <div className="setting-label">Vertical</div>
            <div className="setting-desc">Top ↔ Bottom</div>
          </div>
          <div className="setting-control">
            <input
              type="range"
              className="settings-slider"
              min={0}
              max={100}
              value={Math.round(vPct * 100)}
              onChange={(e) => {
                const pct = parseInt(e.target.value) / 100;
                update("orbVerticalPct", pct);
                liveUpdate(hPct, pct, orbSize);
              }}
            />
            <span className="slider-value">{Math.round(vPct * 100)}%</span>
          </div>
        </div>
      </div>

      <div className="settings-section">
        <div className="settings-section-title">Orb Size</div>
        <div className="setting-row">
          <div>
            <div className="setting-label">Size</div>
            <div className="setting-desc">100px ↔ 300px</div>
          </div>
          <div className="setting-control">
            <input
              type="range"
              className="settings-slider"
              min={100}
              max={300}
              step={10}
              value={orbSize}
              onChange={(e) => {
                const size = parseInt(e.target.value);
                update("orbSize", size);
                liveUpdate(hPct, vPct, size);
              }}
            />
            <span className="slider-value">{orbSize}px</span>
          </div>
        </div>

        <div className="setting-row">
          <div className="setting-label">Reset to default</div>
          <div className="setting-control">
            <button
              className="settings-btn"
              onClick={() => {
                update("orbHorizontalPct", 0.5);
                update("orbVerticalPct", 1.0);
                update("orbSize", 200);
                liveUpdate(0.5, 1.0, 200);
              }}
            >
              Center-bottom, 200px
            </button>
          </div>
        </div>
      </div>
    </>
  );
}

// ─── Audio Tab (Phase 4 — TTS volume + voice picker) ──────────────────
interface TtsVoice {
  id: string;
  name: string;
  gender: string;
  provider: string;
  language: string;
}

function AudioTab({ settings, update }: {
  settings: Settings;
  update: <K extends keyof Settings>(key: K, value: Settings[K]) => void;
}) {
  const volume = settings.ttsVolume ?? 75;
  const [voices, setVoices] = useState<TtsVoice[]>([]);
  const [previewing, setPreviewing] = useState<string | null>(null);

  // Fetch voice list on mount
  useEffect(() => {
    invoke<TtsVoice[]>("list_tts_voices")
      .then(setVoices)
      .catch(() => {});
  }, []);

  const previewVoice = (voiceId: string) => {
    if (previewing) return;
    setPreviewing(voiceId);
    invoke("preview_voice", { voiceId })
      .catch(() => {})
      .finally(() => setTimeout(() => setPreviewing(null), 500));
  };

  const currentVoice = settings.edgeTtsVoice || "en-US-AvaNeural";

  return (
    <>
      <div className="settings-section">
        <div className="settings-section-title">Voice Selection</div>
        <div className="setting-desc" style={{ marginBottom: 8 }}>
          Tap a voice to hear a demo. Selected voice is used for all NEXUS responses.
        </div>
        <div className="voice-list">
          {voices.length === 0 && (
            <div className="voice-empty">Loading voices…</div>
          )}
          {voices.map((v) => (
            <div
              key={v.id}
              className={`voice-row ${currentVoice === v.id ? "voice-row--selected" : ""}`}
              onClick={() => update("edgeTtsVoice", v.id)}
            >
              <div className={`voice-radio ${currentVoice === v.id ? "voice-radio--on" : ""}`} />
              <div className="voice-info">
                <span className="voice-name">{v.name}</span>
                <span className="voice-meta">
                  <span className={`voice-gender voice-gender--${v.gender.toLowerCase()}`}>{v.gender}</span>
                  <span className={`voice-provider voice-provider--${v.provider}`}>{v.provider === "edge-tts" ? "Cloud" : "Offline"}</span>
                </span>
              </div>
              <button
                className="voice-play-btn"
                disabled={previewing !== null}
                onClick={(e) => {
                  e.stopPropagation();
                  previewVoice(v.id);
                }}
              >
                {previewing === v.id ? "⏸" : "▶"}
              </button>
            </div>
          ))}
        </div>
      </div>

      <div className="settings-section">
        <div className="settings-section-title">Speech Volume</div>
        <div className="setting-row">
          <div>
            <div className="setting-label">Default NEXUS volume</div>
            <div className="setting-desc">Sets system volume before speaking, restores after. 0 = disabled.</div>
          </div>
          <div className="setting-control">
            <input
              type="range"
              className="settings-slider"
              min={0}
              max={100}
              value={volume}
              onChange={(e) => update("ttsVolume", parseInt(e.target.value))}
            />
            <span className="slider-value">{volume}%</span>
          </div>
        </div>
      </div>

      <div className="settings-section">
        <div className="settings-section-title">STT Provider</div>
        <div className="setting-row">
          <div>
            <div className="setting-label">Local STT only</div>
            <div className="setting-desc">Audio never leaves your device</div>
          </div>
          <div className="setting-control">
            <div
              className={`settings-toggle ${settings.localSttOnly ? "settings-toggle--on" : ""}`}
              onClick={() => update("localSttOnly", !settings.localSttOnly)}
            />
          </div>
        </div>
        <div className="setting-row">
          <div>
            <div className="setting-label">Moonshine model</div>
            <div className="setting-desc">Local STT accuracy vs RAM tradeoff</div>
          </div>
          <div className="setting-control">
            <select
              className="settings-input"
              value={settings.moonshineModel ?? "medium_streaming"}
              onChange={(e) => update("moonshineModel", e.target.value)}
            >
              <option value="medium_streaming">Medium v2 (245M, 6.65% WER, ~400MB)</option>
              <option value="small_streaming">Small v2 (123M, 7.84% WER, ~300MB)</option>
              <option value="tiny_streaming">Tiny (34M, 12% WER, ~100MB)</option>
            </select>
          </div>
        </div>
      </div>
    </>
  );
}

// ─── Auth Tab (Phase 3 — OAuth + API keys) ────────────────────────────
function AuthTab({ settings, update }: {
  settings: Settings;
  update: <K extends keyof Settings>(key: K, value: Settings[K]) => void;
}) {
  const [oauthStatus, setOauthStatus] = useState<Record<string, OAuthStatus>>({});
  const [connecting, setConnecting] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);

  // Set the sidecar base URL and fetch OAuth status on mount
  useEffect(() => {
    if (settings.serverUrl) {
      setSidecarBaseUrl(settings.serverUrl);
      if (settings.userId) {
        getOAuthStatus(settings.userId).then(setOauthStatus).catch(() => {});
      }
    }
  }, [settings.serverUrl, settings.userId]);

  const handleConnect = async (provider: "google" | "github") => {
    setError(null);
    setConnecting(provider);
    try {
      if (!settings.serverUrl) throw new Error("Server URL not configured");
      setSidecarBaseUrl(settings.serverUrl);
      const success = await connectOAuth(provider, settings.userId || "local-user");
      if (success) {
        // Refresh status
        const status = await getOAuthStatus(settings.userId || "local-user");
        setOauthStatus(status);
      }
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
    } finally {
      setConnecting(null);
    }
  };

  const handleDisconnect = async (provider: "google" | "github") => {
    setError(null);
    try {
      await disconnectOAuth(settings.userId || "local-user", provider);
      const status = await getOAuthStatus(settings.userId || "local-user");
      setOauthStatus(status);
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
    }
  };

  const googleConnected = oauthStatus.google?.connected ?? false;
  const githubConnected = oauthStatus.github?.connected ?? false;

  return (
    <>
      {error && (
        <div className="auth-card" style={{ borderColor: "rgba(255,80,80,0.3)" }}>
          <span style={{ fontSize: 12, color: "rgba(255,150,150,0.9)" }}>{error}</span>
        </div>
      )}

      <div className="settings-section">
        <div className="settings-section-title">Connected Accounts</div>

        <div className="auth-card">
          <div className="auth-card-header">
            <span className="auth-card-title">Google</span>
            <span className={`status-badge ${googleConnected ? "status-badge--connected" : "status-badge--disconnected"}`}>
              {googleConnected ? "Connected" : "Not connected"}
            </span>
          </div>
          <div className="auth-card-actions">
            {googleConnected ? (
              <button className="settings-btn settings-btn--danger" onClick={() => handleDisconnect("google")}>
                Disconnect
              </button>
            ) : (
              <button
                className="settings-btn settings-btn--primary"
                disabled={connecting === "google"}
                onClick={() => handleConnect("google")}
              >
                {connecting === "google" ? "Waiting..." : "Connect Google"}
              </button>
            )}
          </div>
        </div>

        <div className="auth-card">
          <div className="auth-card-header">
            <span className="auth-card-title">GitHub</span>
            <span className={`status-badge ${githubConnected ? "status-badge--connected" : "status-badge--disconnected"}`}>
              {githubConnected ? "Connected" : "Not connected"}
            </span>
          </div>
          <div className="auth-card-actions">
            {githubConnected ? (
              <button className="settings-btn settings-btn--danger" onClick={() => handleDisconnect("github")}>
                Disconnect
              </button>
            ) : (
              <button
                className="settings-btn settings-btn--primary"
                disabled={connecting === "github"}
                onClick={() => handleConnect("github")}
              >
                {connecting === "github" ? "Waiting..." : "Connect GitHub"}
              </button>
            )}
          </div>
        </div>
      </div>

      <div className="settings-section">
        <div className="settings-section-title">API Keys</div>

        <div className="auth-card">
          <div className="auth-card-header">
            <span className="auth-card-title">Gemini API Key</span>
          </div>
          <input
            type="password"
            className="settings-input"
            placeholder="AIza..."
            value={settings.geminiApiKey ?? ""}
            onChange={(e) => update("geminiApiKey", e.target.value)}
          />
        </div>

        <div className="auth-card">
          <div className="auth-card-header">
            <span className="auth-card-title">Groq API Key</span>
          </div>
          <input
            type="password"
            className="settings-input"
            placeholder="gsk_..."
            value={settings.groqApiKey}
            onChange={(e) => update("groqApiKey", e.target.value)}
          />
        </div>

        <div className="auth-card">
          <div className="auth-card-header">
            <span className="auth-card-title">Cerebras API Key</span>
          </div>
          <input
            type="password"
            className="settings-input"
            placeholder="csk-..."
            value={settings.cerebrasApiKey ?? ""}
            onChange={(e) => update("cerebrasApiKey", e.target.value)}
          />
        </div>
      </div>
    </>
  );
}

// ─── Connections Tab (MCP vault — one login per service group) ──────
// Each card shows vault status (live/expired/missing) with Reconnect +
// Delete. Google reconnects via OAuth (Accounts tab flow writes the token
// into the vault on next MCP call); token services paste a key directly.
// Session bridges (WhatsApp/LinkedIn) show bridge health + instructions.
const VAULT_META: Record<string, { title: string; hint: string; kind: "oauth" | "token" | "session"; tokenUrl?: string }> = {
  google: { title: "Google (Gmail, Calendar, Contacts, Drive, Sheets, Meet)", hint: "One OAuth login covers all six. Reconnect in Accounts tab, then Delete here only clears the local copy.", kind: "oauth" },
  telegram: { title: "Telegram remote (owner-only phone control)", hint: "Create a bot via @BotFather, paste its token below, then put your chat id (from @userinfobot) in the Owner chat id field and press Save + restart.", kind: "token" },
  swiggy: { title: "Swiggy (Food, Instamart, Dineout)", hint: "One OAuth login covers all three. Login opens your browser; NEXUS stores and refreshes the token. A pasted token works too.", kind: "token" },
  spotify: { title: "Spotify", hint: "1. Open the Spotify dashboard (button below). 2. Create a token with user-read scope. 3. Paste it here.", kind: "token", tokenUrl: "https://developer.spotify.com/dashboard" },
  vercel: { title: "Vercel", hint: "1. Open Vercel account settings (button below). 2. Create a token (read-only recommended). 3. Paste it here.", kind: "token", tokenUrl: "https://vercel.com/account/tokens" },
  render: { title: "Render", hint: "1. Open Render API keys (button below). 2. Create a key. 3. Paste it here. OAuth preferred — keys are broadly scoped.", kind: "token", tokenUrl: "https://dashboard.render.com/u/keys" },
};

function ConnectionsTab({ settings, update }: {
  settings: Settings;
  update: <K extends keyof Settings>(key: K, value: Settings[K]) => void;
}) {
  const [vault, setVault] = useState<Record<string, string>>({});
  const [drafts, setDrafts] = useState<Record<string, string>>({});
  const [error, setError] = useState<string | null>(null);
  const [swiggyConnecting, setSwiggyConnecting] = useState(false);

  const refresh = useCallback(async () => {
    try {
      const entries = await invoke<{ service: string; status: string }[]>("vault_status");
      const map: Record<string, string> = {};
      for (const e of entries) map[e.service] = e.status;
      setVault(map);
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
    }
  }, []);

  useEffect(() => { refresh().catch(() => {}); }, [refresh]);

  // Live refresh when the idle vault monitor reports a credential change
  // (expiry/reconnect while the tab is open).
  useEffect(() => {
    let unlisten: (() => void) | null = null;
    (async () => {
      const { listen } = await import("@tauri-apps/api/event");
      unlisten = await listen<string>("vault:changed", () => {
        refresh().catch(() => {});
      });
    })();
    return () => { unlisten?.(); };
  }, [refresh]);

  const handleSave = async (service: string) => {
    setError(null);
    try {
      await invoke("vault_set_token", { service, token: drafts[service] ?? "", expiresInSecs: 0 });
      setDrafts((d) => ({ ...d, [service]: "" }));
      await refresh();
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
    }
  };

  const handleDelete = async (service: string) => {
    setError(null);
    try {
      await invoke("vault_clear_token", { service });
      await refresh();
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
    }
  };

  // Swiggy OAuth: browser login → Worker stores + refreshes server-side;
  // the vault pulls a fresh token on next use (or the user pastes one).
  const handleSwiggyLogin = async () => {
    setError(null);
    setSwiggyConnecting(true);
    try {
      if (!settings.serverUrl) throw new Error("Server URL not configured");
      setSidecarBaseUrl(settings.serverUrl);
      await connectOAuth("swiggy", settings.userId || "local-user");
      await refresh();
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
    } finally {
      setSwiggyConnecting(false);
    }
  };

  const badge = (status?: string) => (
    <span className={`status-badge ${status === "live" ? "status-badge--connected" : "status-badge--disconnected"}`}>
      {status === "live" ? "Live" : status === "expired" ? "Expired" : "Missing"}
    </span>
  );

  return (
    <>
      {error && (
        <div className="auth-card" style={{ borderColor: "rgba(255,80,80,0.3)" }}>
          <span style={{ fontSize: 12, color: "rgba(255,150,150,0.9)" }}>{error}</span>
        </div>
      )}

      <div className="settings-section">
        <div className="settings-section-title">MCP Connections (vault)</div>

        {Object.entries(VAULT_META).map(([service, meta]) => (
          <div className="auth-card" key={service}>
            <div className="auth-card-header">
              <span className="auth-card-title">{meta.title}</span>
              {badge(vault[service])}
            </div>
            <div style={{ fontSize: 12, opacity: 0.65, margin: "4px 0 8px" }}>{meta.hint}</div>
            {meta.kind === "token" && service !== "telegram" && (
              <input
                type="password"
                className="settings-input"
                placeholder="paste token…"
                value={drafts[service] ?? ""}
                onChange={(e) => setDrafts((d) => ({ ...d, [service]: e.target.value }))}
              />
            )}
            {service === "telegram" && (
              <>
                <input
                  type="password"
                  className="settings-input"
                  placeholder="bot token from @BotFather…"
                  value={drafts[service] ?? ""}
                  onChange={(e) => setDrafts((d) => ({ ...d, [service]: e.target.value }))}
                />
                <input
                  type="text"
                  className="settings-input"
                  placeholder="owner chat id (from @userinfobot)…"
                  value={settings.telegramChatId ?? ""}
                  onChange={(e) => update("telegramChatId", e.target.value)}
                />
                <div style={{ fontSize: 12, opacity: 0.65, margin: "4px 0 8px" }}>
                  Save (footer) persists the chat id — restart NEXUS to start the bridge. Only this chat is ever answered.
                </div>
              </>
            )}
            {service === "swiggy" && (
              <button
                className="settings-btn settings-btn--primary"
                disabled={swiggyConnecting || !settings.serverUrl}
                onClick={handleSwiggyLogin}
              >
                {swiggyConnecting ? "Waiting for login…" : "Login with Swiggy"}
              </button>
            )}
            {meta.kind === "token" && meta.tokenUrl && (
              <button
                className="settings-btn"
                onClick={() => { import("@tauri-apps/plugin-shell").then(({ open }) => open(meta.tokenUrl!)).catch(() => window.open(meta.tokenUrl!, "_blank")); }}
              >
                Get token
              </button>
            )}
            <div className="auth-card-actions">
              {meta.kind === "token" && (
                <button
                  className="settings-btn settings-btn--primary"
                  disabled={!(drafts[service] ?? "").trim()}
                  onClick={() => handleSave(service)}
                >
                  Save token
                </button>
              )}
              <button className="settings-btn settings-btn--danger" onClick={() => handleDelete(service)}>
                Delete
              </button>
            </div>
          </div>
        ))}
      </div>

      <div className="settings-section">
        <div className="settings-section-title">Session bridges (local)</div>

        <BridgeCards />
      </div>
    </>
  );
}

function BridgeCards() {
  const [bridges, setBridges] = useState<Record<string, { reachable: boolean; note: string }>>({});
  const [checking, setChecking] = useState(false);

  const refresh = useCallback(async () => {
    setChecking(true);
    try {
      const list = await invoke<{ server: string; url: string; reachable: boolean; latency_ms: number; note: string }[]>("mcp_status");
      const map: Record<string, { reachable: boolean; note: string }> = {};
      for (const b of list) map[b.server] = { reachable: b.reachable, note: b.note };
      setBridges(map);
    } catch {
      // probe failed entirely — leave cards in unknown state
    } finally {
      setChecking(false);
    }
  }, []);

  useEffect(() => { refresh().catch(() => {}); }, [refresh]);

  const card = (key: string, title: string, hint: string) => {
    const st = bridges[key];
    return (
      <div className="auth-card" key={key}>
        <div className="auth-card-header">
          <span className="auth-card-title">{title}</span>
          <span className={`status-badge ${st?.reachable ? "status-badge--connected" : "status-badge--disconnected"}`}>
            {st ? (st.reachable ? "Reachable" : "Down") : "…"}
          </span>
        </div>
        <div style={{ fontSize: 12, opacity: 0.65, margin: "4px 0 8px" }}>{hint}</div>
        {st && (
          <div style={{ fontSize: 12, opacity: 0.65, margin: "0 0 8px" }}>{st.note}</div>
        )}
      </div>
    );
  };

  return (
    <>
      {card("whatsapp", "WhatsApp bridge (:8765)", "Run the bridge binary, then scan the QR once — the session persists. Reads never mark messages read.")}
      {card("amazon", "Amazon bridge (:8766)", "Run the product-search bridge locally. Read-only: search, details, reviews.")}
      <div className="auth-card">
        <div className="auth-card-header">
          <span className="auth-card-title">LinkedIn session</span>
        </div>
        <div style={{ fontSize: 12, opacity: 0.65, margin: "4px 0 8px" }}>
          Uses your logged-in browser session cookie. Re-login in the browser if writes start failing.
        </div>
      </div>
      <div className="auth-card-actions">
        <button className="settings-btn" disabled={checking} onClick={refresh}>
          {checking ? "Checking…" : "Recheck bridges"}
        </button>
      </div>
    </>
  );
}

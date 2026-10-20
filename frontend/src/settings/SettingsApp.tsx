import { useState, useEffect, useCallback } from "react";
import { invoke } from "@tauri-apps/api/core";

import { stopTts, type VoicePersona, ICONIC_PERSONAS } from "../audio/ttsPlayer";
import { initThemeSync, applyThemeMode, resolveThemeMode } from "../sidebar/theme";
import { globalHotkeyHint, globalHotkeyLabel } from "../platform";
import { McpServersPanel } from "../settings-sidebar/McpServersPanel";

/**
 * NEXUS Settings Window
 *
 * Tabbed settings panel with white theme:
 *   - General: autostart, hotkey, theme, auto-hide delay
 *   - Audio: mic/speaker selection, TTS voice, speech rate
 *   - Wake Word: detection toggle, phrase, sensitivity, speaker verification
 *   - Privacy: meeting mode, TTS suppression, local STT, clear history
 *   - Backend: server URL, connection status, credentials
 */

type Tab = "general" | "accounts" | "audio" | "wake" | "privacy" | "backend";

interface Settings {
  autostart: boolean;
  hotkey: string;
  autoHideDelay: number;
  wakeWordEnabled: boolean;
  wakePhrase: string;
  wakeSensitivity: "low" | "medium" | "high";
  speakerVerification: boolean;
  meetingModeAuto: boolean;
  suppressTtsInMeetings: boolean;
  localSttOnly: boolean;
  serverUrl: string;
  userId: string;
  deviceId: string;
  ttsVoice: string;
  speechRate: number;
  ttsVolume?: number;
  ttsProvider?: string;
  elevenlabsApiKey?: string;
  fishAudioApiKey?: string;
  googleCloudApiKey?: string;
  groqApiKey?: string;
  edgeTtsVoice?: string;
  selectedVoice?: string;
  offlineVoiceModel?: string;
  orbPosition?: "top" | "bottom";
  themeMode?: "light" | "dark";
  defaultMusicPlayer?: "spotify" | "apple_music";
}

const DEFAULT_SETTINGS: Settings = {
  autostart: true,
  hotkey: globalHotkeyLabel(),
  autoHideDelay: 8,
  wakeWordEnabled: true,
  wakePhrase: "NEXUS",
  wakeSensitivity: "medium",
  speakerVerification: false,
  meetingModeAuto: true,
  suppressTtsInMeetings: true,
  localSttOnly: true,
  serverUrl: "",
  userId: "local-user",
  deviceId: "local-device",
  ttsVoice: "en-GB-RyanNeural",
  speechRate: 1.15,
  ttsVolume: 75,
  ttsProvider: "edge",
  groqApiKey: "",
  edgeTtsVoice: "en-GB-RyanNeural",
  selectedVoice: "jarvis",
  offlineVoiceModel: "bm_george",
  orbPosition: "top",
  themeMode: "dark",
  defaultMusicPlayer: "spotify",
};

const TABS: { id: Tab; label: string; icon: string }[] = [
  { id: "general", label: "General", icon: "M3 12l2-2m0 0l7-7 7 7M5 10v10a1 1 0 001 1h3m10-11l2 2m-2-2v10a1 1 0 01-1 1h-3m-6 0a1 1 0 001-1v-4a1 1 0 011-1h2a1 1 0 011 1v4a1 1 0 001 1m-6 0h6" },
  { id: "accounts", label: "Accounts & Integrations", icon: "M16 7a4 4 0 11-8 0 4 4 0 018 0zM12 14a7 7 0 00-7 7h14a7 7 0 00-7-7z" },
  { id: "audio", label: "Audio & Voice", icon: "M19 11a7 7 0 01-7 7m0 0a7 7 0 01-7-7m7 7v4m0 0H8m4 0h4m-4-8a3 3 0 01-3-3V5a3 3 0 116 0v6a3 3 0 01-3 3z" },
  { id: "wake", label: "Wake Word", icon: "M15 10.5a3 3 0 11-6 0 3 3 0 016 0z M19 10.5a7 7 0 11-14 0 7 7 0 0114 0z" },
  { id: "privacy", label: "Privacy", icon: "M9 12l2 2 4-4m5.618-4.016A11.955 11.955 0 0112 2.944a11.955 11.955 0 01-8.618 3.04A12.02 12.02 0 003 9c0 5.591 3.824 10.29 9 11.622 5.176-1.332 9-6.03 9-11.622 0-1.042-.133-2.052-.382-3.016z" },
  { id: "backend", label: "Backend", icon: "M5 12h14M5 12a2 2 0 01-2-2V6a2 2 0 012-2h14a2 2 0 012 2v4a2 2 0 01-2 2M5 12a2 2 0 00-2 2v4a2 2 0 002 2h14a2 2 0 002-2v-4a2 2 0 00-2-2m-2-4h.01M17 16h.01" },
];

export function SettingsApp() {
  const [tab, setTab] = useState<Tab>("general");
  const [settings, setSettings] = useState<Settings>(DEFAULT_SETTINGS);
  const [saved, setSaved] = useState(false);
  const [serverConnected, setServerConnected] = useState(false);

  // Initialize theme synchronization
  useEffect(() => {
    initThemeSync();
  }, []);

  // Escape key discards/closes the settings window
  useEffect(() => {
    const handler = (e: KeyboardEvent) => {
      if (e.key === "Escape") {
        invoke("close_settings_window").catch(() => {});
      }
    };
    window.addEventListener("keydown", handler);
    return () => window.removeEventListener("keydown", handler);
  }, []);

  // Load settings from Rust on mount
  useEffect(() => {
    invoke<Partial<Settings>>("get_settings").then((s) => {
      if (s) {
        setSettings({ ...DEFAULT_SETTINGS, ...s });
        if (s.themeMode) {
          applyThemeMode(resolveThemeMode(s.themeMode), false);
        }
      }
    }).catch(() => {
      // Settings command not available yet — use defaults
    });
  }, []);

  // Check server connection when backend tab is opened
  useEffect(() => {
    if (tab === "backend" && settings.serverUrl) {
      fetch(`${settings.serverUrl.replace(/\/+$/, "")}/health`)
        .then(() => setServerConnected(true))
        .catch(() => setServerConnected(false));
    }
  }, [tab, settings.serverUrl]);

  const update = useCallback(<K extends keyof Settings>(key: K, value: Settings[K]) => {
    setSettings((prev) => ({ ...prev, [key]: value }));
    setSaved(false);
  }, []);

  const handleSave = async () => {
    try {
      if (settings.themeMode) {
        applyThemeMode(settings.themeMode);
      }
      await invoke("save_settings", { settings });
      // Sync autostart setting with the OS
      try {
        await invoke("set_autostart", { enabled: settings.autostart });
      } catch (e) {
        console.warn("set_autostart failed:", e);
      }
      setSaved(true);
      setTimeout(() => setSaved(false), 2000);
    } catch {
      // Settings command not available yet — ignore
    }
  };

  const handleReset = () => {
    setSettings(DEFAULT_SETTINGS);
    setSaved(false);
  };

  return (
    <div className="nx-settings">
      {/* Window Drag Titlebar */}
      <div data-tauri-drag-region className="nx-window-titlebar">
        <span style={{ fontSize: "11px", fontWeight: 600, opacity: 0.65, letterSpacing: "0.5px" }}>NEXUS SETTINGS</span>
        <button
          type="button"
          onClick={() => invoke("close_settings_window").catch(() => {})}
          style={{
            background: "none",
            border: "none",
            color: "var(--nx-text-secondary)",
            cursor: "pointer",
            padding: "4px 8px",
            fontSize: "14px",
            lineHeight: 1,
            borderRadius: "6px",
          }}
          title="Close (Esc)"
        >
          ✕
        </button>
      </div>

      {/* Sidebar */}
      <aside className="nx-sidebar">
        <div className="nx-sidebar-header">
          <div className="nx-sidebar-logo">NEXUS</div>
          <div className="nx-sidebar-sub">Settings</div>
        </div>
        {TABS.map((t) => (
          <button
            key={t.id}
            className={`nx-tab ${tab === t.id ? "nx-tab--active" : ""}`}
            onClick={() => setTab(t.id)}
          >
            <svg className="nx-tab-icon" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round">
              <path d={t.icon} />
            </svg>
            {t.label}
          </button>
        ))}
      </aside>

      {/* Main content */}
      <main className="nx-content">
        <div className="nx-content-header">
          <h1 className="nx-content-title">
            {TABS.find((t) => t.id === tab)?.label}
          </h1>
          {saved && <span className="nx-badge nx-badge--ok">Saved</span>}
        </div>

        <div style={{ display: "flex", flexDirection: "column", gap: "var(--nx-space-5)" }}>
          {tab === "general" && (
            <GeneralTab settings={settings} update={update} />
          )}
          {tab === "accounts" && (
            <AccountsTab />
          )}
          {tab === "audio" && (
            <AudioTab settings={settings} update={update} />
          )}
          {tab === "wake" && (
            <WakeTab settings={settings} update={update} />
          )}
          {tab === "privacy" && (
            <PrivacyTab settings={settings} update={update} />
          )}
          {tab === "backend" && (
            <BackendTab
              settings={settings}
              update={update}
              connected={serverConnected}
            />
          )}
        </div>
      </main>

      {/* Footer */}
      <div className="nx-footer" style={{ position: "absolute", bottom: 0, right: 0, left: "200px" }}>
        <button className="nx-btn" onClick={handleReset}>Reset to Defaults</button>
        <button className="nx-btn nx-btn--primary" onClick={handleSave}>Save</button>
      </div>
    </div>
  );
}

// ── Tab Components ──────────────────────────────────────────────

function Toggle({ on, onClick }: { on: boolean; onClick: () => void }) {
  return (
    <button className={`nx-toggle ${on ? "nx-toggle--on" : ""}`} onClick={onClick}>
      <span className="nx-toggle-knob" />
    </button>
  );
}

function GeneralTab({ settings, update }: { settings: Settings; update: <K extends keyof Settings>(k: K, v: Settings[K]) => void }) {
  return (
    <>
      <section className="nx-section">
        <div className="nx-section-title">Startup</div>
        <div className="nx-row">
          <div className="nx-row-label">
            <span className="nx-row-name">Start at login</span>
            <span className="nx-row-hint">Launch NEXUS automatically when you log in</span>
          </div>
          <Toggle on={settings.autostart} onClick={() => update("autostart", !settings.autostart)} />
        </div>
      </section>

      <section className="nx-section">
        <div className="nx-section-title">Interaction</div>
        <div className="nx-row">
          <div className="nx-row-label">
            <span className="nx-row-name">Global hotkey</span>
            <span className="nx-row-hint">{globalHotkeyHint("Press this key combo to wake NEXUS")}</span>
          </div>
          <input className="nx-input" value={settings.hotkey} onChange={(e) => update("hotkey", e.target.value)} />
        </div>
        <div className="nx-row">
          <div className="nx-row-label">
            <span className="nx-row-name">Auto-hide delay</span>
            <span className="nx-row-hint">Seconds before NEXUS hides after silence</span>
          </div>
          <input
            type="number"
            className="nx-input"
            style={{ width: 80 }}
            value={settings.autoHideDelay}
            min={3}
            max={30}
            onChange={(e) => update("autoHideDelay", parseInt(e.target.value) || 8)}
          />
        </div>
      </section>

      <section className="nx-section">
        <div className="nx-section-title">Orb Screen Position</div>
        <div className="nx-row">
          <div className="nx-row-label">
            <span className="nx-row-name">Screen Placement</span>
            <span className="nx-row-hint">Position the black slider capsule at the top or bottom of the screen</span>
          </div>
          <div style={{ display: "flex", gap: "8px" }}>
            <button
              type="button"
              className={`nx-btn ${settings.orbPosition !== "bottom" ? "nx-btn--primary" : ""}`}
              onClick={() => update("orbPosition", "top")}
            >
              Top (Default)
            </button>
            <button
              type="button"
              className={`nx-btn ${settings.orbPosition === "bottom" ? "nx-btn--primary" : ""}`}
              onClick={() => update("orbPosition", "bottom")}
            >
              Bottom
            </button>
          </div>
        </div>
      </section>

      <section className="nx-section">
        <div className="nx-section-title">Theme & Appearance</div>
        <div className="nx-row">
          <div className="nx-row-label">
            <span className="nx-row-name">Window Style</span>
            <span className="nx-row-hint">Liquid frosted glass across all windows, architecture mapper, and information analyses</span>
          </div>
          <div style={{ display: "flex", gap: "8px" }}>
            <button
              type="button"
              className={`nx-btn ${settings.themeMode !== "light" ? "nx-btn--primary" : ""}`}
              onClick={() => {
                update("themeMode", "dark");
                applyThemeMode("dark");
              }}
            >
              🌙 Dark (Liquid Glass)
            </button>
            <button
              type="button"
              className={`nx-btn ${settings.themeMode === "light" ? "nx-btn--primary" : ""}`}
              onClick={() => {
                update("themeMode", "light");
                applyThemeMode("light");
              }}
            >
              ☀️ Light (Frosted White)
            </button>
          </div>
        </div>
      </section>
    </>
  );
}

function AudioTab({ settings, update }: { settings: Settings; update: <K extends keyof Settings>(k: K, v: Settings[K]) => void }) {
  const [personas, setPersonas] = useState<VoicePersona[]>(ICONIC_PERSONAS);
  const [previewing, setPreviewing] = useState<string | null>(null);
  const [equipping, setEquipping] = useState<string | null>(null);
  const [sync, setSync] = useState<Record<string, { state: "ready" | "cloud_only" | "downloading"; progress?: number; phase?: string }>>({});

  useEffect(() => {
    invoke<VoicePersona[]>("list_voice_personas")
      .then((list) => {
        if (Array.isArray(list) && list.length > 0) {
          setPersonas(list);
          for (const p of list) {
            invoke<string>("get_voice_sync_state", { voiceKey: p.key })
              .then((state) => {
                const mapped = state === "ready" ? "ready" : "cloud_only";
                setSync((m) => ({ ...m, [p.key]: { state: mapped } }));
              })
              .catch(() => {});
          }
        }
      })
      .catch(() => {});

    let unlisten: (() => void) | undefined;
    (async () => {
      try {
        const { listen } = await import("@tauri-apps/api/event");
        unlisten = await listen<any>("voice:status", (event) => {
          const payload = event.payload;
          if (!payload || !payload.voice_key) return;
          setSync((m) => ({
            ...m,
            [payload.voice_key]: {
              state: payload.status,
              progress: payload.progress ?? undefined,
              phase: payload.phase,
            },
          }));
        });
      } catch {}
    })();

    return () => {
      unlisten?.();
    };
  }, []);

  const handlePreviewPersona = async (p: VoicePersona, e: React.MouseEvent) => {
    e.stopPropagation();
    if (previewing === p.key) {
      stopTts();
      setPreviewing(null);
      return;
    }
    setPreviewing(p.key);
    try {
      await invoke("preview_voice", { voiceId: p.cloudId, text: p.previewPhrase });
    } catch {}
    setTimeout(() => setPreviewing(null), 3000);
  };

  const handleEquipPersona = async (p: VoicePersona) => {
    if (equipping) return;
    setEquipping(p.key);
    update("selectedVoice", p.key);
    update("edgeTtsVoice", p.cloudId);
    update("ttsVoice", p.cloudId);
    update("offlineVoiceModel", p.kokoroVoice);

    try {
      const res = await invoke<{ voiceKey: string; cloudId: string; syncState: string }>("set_voice_preference", {
        voiceKey: p.key,
      });
      if (res) {
        const st = res.syncState === "ready" ? "ready" : "cloud_only";
        setSync((m) => ({ ...m, [res.voiceKey]: { state: st } }));
      }
    } catch (e) {
      console.warn("set_voice_preference failed:", e);
    } finally {
      setEquipping(null);
    }
  };

  const activePersonaKey =
    settings.selectedVoice ||
    personas.find((p) => p.cloudId === (settings.edgeTtsVoice || "en-GB-RyanNeural"))?.key ||
    "jarvis";

  const renderSyncPill = (key: string) => {
    const s = sync[key];
    if (!s) return null;
    if (s.state === "ready") {
      return <span className="nx-persona-sync nx-persona-sync--ready">✓ Cloud + Offline Ready</span>;
    }
    if (s.state === "downloading") {
      const pct = typeof s.progress === "number" ? ` ${s.progress}%` : "…";
      const label = s.phase === "base" ? "Downloading base engine" : "Syncing twin";
      return <span className="nx-persona-sync nx-persona-sync--busy">⬇ {label}{pct}</span>;
    }
    return <span className="nx-persona-sync nx-persona-sync--pending">⚡ Cloud Only (Twin Pending)</span>;
  };

  return (
    <>
      <section className="nx-section">
        <div className="nx-section-title">Input</div>
        <div className="nx-row">
          <div className="nx-row-label">
            <span className="nx-row-name">Microphone</span>
            <span className="nx-row-hint">Default system microphone is used for wake word and STT</span>
          </div>
          <span className="nx-badge nx-badge--ok">System Default</span>
        </div>
      </section>

      <section className="nx-section">
        <div className="nx-section-title">Voice & Persona Matrix (Top 10 Iconic Personalities)</div>
        <div style={{ fontSize: "var(--nx-text-xs)", color: "var(--nx-text-secondary)", marginBottom: "8px" }}>
          Click any persona card to equip it. Cloud voice switches instantly ($0, 0 MB RAM); matching offline twin downloads automatically.
        </div>

        <div className="nx-persona-grid">
          {personas.map((p) => {
            const isActive = activePersonaKey === p.key;
            const isPlaying = previewing === p.key;
            return (
              <div
                key={p.key}
                className={`nx-persona-card ${isActive ? "nx-persona-card--active" : ""}`}
                onClick={() => handleEquipPersona(p)}
              >
                <div className="nx-persona-card-top">
                  <span className="nx-persona-avatar">{p.avatar}</span>
                  {isActive && <span className="nx-persona-check">✓ Active</span>}
                </div>
                <div className="nx-persona-name">{p.name}</div>
                <div className="nx-persona-accent">{p.accentTag}</div>
                <div className="nx-persona-desc">{p.persona}</div>
                <div className="nx-persona-meta">
                  <span>Cloud: {p.cloudId.replace("Neural", "")}</span>
                  <span>Twin: {p.kokoroVoice}</span>
                </div>
                {renderSyncPill(p.key)}
                <button
                  type="button"
                  className="nx-persona-play-btn"
                  disabled={equipping !== null}
                  onClick={(e) => handlePreviewPersona(p, e)}
                >
                  {isPlaying ? "⏹ Stop Sample" : "▶ Play Sample"}
                </button>
              </div>
            );
          })}
        </div>
      </section>

      <section className="nx-section">
        <div className="nx-section-title">Speech Prosody & Volume</div>
        <div className="nx-row">
          <div className="nx-row-label">
            <span className="nx-row-name">Speech Rate</span>
            <span className="nx-row-hint">{settings.speechRate.toFixed(1)}x speed</span>
          </div>
          <input
            type="range"
            className="nx-slider"
            min={0.5}
            max={2.0}
            step={0.1}
            value={settings.speechRate}
            onChange={(e) => update("speechRate", parseFloat(e.target.value))}
          />
        </div>

        <div className="nx-row">
          <div className="nx-row-label">
            <span className="nx-row-name">TTS Volume</span>
            <span className="nx-row-hint">
              NEXUS sets system volume to {(settings.ttsVolume ?? 75)}% while speaking, then restores it
            </span>
          </div>
          <input
            type="range"
            className="nx-slider"
            min={0}
            max={100}
            step={5}
            value={settings.ttsVolume ?? 75}
            onChange={(e) => update("ttsVolume", parseInt(e.target.value))}
          />
        </div>
      </section>

      <section className="nx-section">
        <div className="nx-section-title">Media & Music Control</div>
        <div className="nx-row">
          <div className="nx-row-label">
            <span className="nx-row-name">Default Music Player</span>
            <span className="nx-row-hint">
              Selected player for "play songs", "play music", or voice media commands
            </span>
          </div>
          <div style={{ display: "flex", gap: "8px" }}>
            <button
              type="button"
              className={`nx-btn ${(!settings.defaultMusicPlayer || settings.defaultMusicPlayer === "spotify") ? "nx-btn--primary" : ""}`}
              onClick={() => update("defaultMusicPlayer", "spotify")}
              style={{ display: "flex", alignItems: "center", gap: "6px" }}
            >
              <span>🟢</span>
              <span>Spotify</span>
            </button>
            <button
              type="button"
              className={`nx-btn ${settings.defaultMusicPlayer === "apple_music" ? "nx-btn--primary" : ""}`}
              onClick={() => update("defaultMusicPlayer", "apple_music")}
              style={{ display: "flex", alignItems: "center", gap: "6px" }}
            >
              <span>🟣</span>
              <span>Apple Music</span>
            </button>
          </div>
        </div>
      </section>
    </>
  );
}

function WakeTab({ settings, update }: { settings: Settings; update: <K extends keyof Settings>(k: K, v: Settings[K]) => void }) {
  return (
    <>
      <section className="nx-section">
        <div className="nx-section-title">Wake Word Detection</div>
        <div className="nx-row">
          <div className="nx-row-label">
            <span className="nx-row-name">Enable wake word</span>
            <span className="nx-row-hint">Listen for the wake word to activate NEXUS hands-free</span>
          </div>
          <Toggle on={settings.wakeWordEnabled} onClick={() => update("wakeWordEnabled", !settings.wakeWordEnabled)} />
        </div>
        <div className="nx-row">
          <div className="nx-row-label">
            <span className="nx-row-name">Wake phrase</span>
            <span className="nx-row-hint">The word NEXUS responds to</span>
          </div>
          <input className="nx-input" value={settings.wakePhrase} onChange={(e) => update("wakePhrase", e.target.value)} />
        </div>
        <div className="nx-row">
          <div className="nx-row-label">
            <span className="nx-row-name">Sensitivity</span>
            <span className="nx-row-hint">Higher = easier to trigger, more false positives</span>
          </div>
          <div className="nx-segmented">
            {(["low", "medium", "high"] as const).map((s) => (
              <button
                key={s}
                className={`nx-segmented-btn ${settings.wakeSensitivity === s ? "nx-segmented-btn--active" : ""}`}
                onClick={() => update("wakeSensitivity", s)}
              >
                {s.charAt(0).toUpperCase() + s.slice(1)}
              </button>
            ))}
          </div>
        </div>
      </section>

      <section className="nx-section">
        <div className="nx-section-title">Speaker Verification</div>
        <div className="nx-row">
          <div className="nx-row-label">
            <span className="nx-row-name">Voice lock</span>
            <span className="nx-row-hint">Only respond to your enrolled voice profile</span>
          </div>
          <Toggle on={settings.speakerVerification} onClick={() => update("speakerVerification", !settings.speakerVerification)} />
        </div>
        <div className="nx-row">
          <div className="nx-row-label">
            <span className="nx-row-name">Voice enrollment</span>
            <span className="nx-row-hint">Record 5 clips of your voice saying "NEXUS"</span>
          </div>
          <button className="nx-btn" onClick={() => invoke("open_setup_window").catch(() => {})}>
            Enroll →
          </button>
        </div>
      </section>
    </>
  );
}

function PrivacyTab({ settings, update }: { settings: Settings; update: <K extends keyof Settings>(k: K, v: Settings[K]) => void }) {
  return (
    <>
      <section className="nx-section">
        <div className="nx-section-title">Meeting Mode</div>
        <div className="nx-row">
          <div className="nx-row-label">
            <span className="nx-row-name">Auto-detect meetings</span>
            <span className="nx-row-hint">Detect when another app is using the microphone</span>
          </div>
          <Toggle on={settings.meetingModeAuto} onClick={() => update("meetingModeAuto", !settings.meetingModeAuto)} />
        </div>
        <div className="nx-row">
          <div className="nx-row-label">
            <span className="nx-row-name">Suppress TTS in meetings</span>
            <span className="nx-row-hint">Don't speak aloud during calls and meetings</span>
          </div>
          <Toggle on={settings.suppressTtsInMeetings} onClick={() => update("suppressTtsInMeetings", !settings.suppressTtsInMeetings)} />
        </div>
      </section>

      <section className="nx-section">
        <div className="nx-section-title">Data Privacy</div>
        <div className="nx-row">
          <div className="nx-row-label">
            <span className="nx-row-name">Local STT only</span>
            <span className="nx-row-hint">Audio never leaves your device — only transcripts are sent</span>
          </div>
          <Toggle on={settings.localSttOnly} onClick={() => update("localSttOnly", !settings.localSttOnly)} />
        </div>
        <div className="nx-row">
          <div className="nx-row-label">
            <span className="nx-row-name">Clear conversation history</span>
            <span className="nx-row-hint">Remove all stored transcripts from this session</span>
          </div>
          <button className="nx-btn nx-btn--danger nx-btn--small" onClick={() => invoke("clear_transcript").catch(() => {})}>
            Clear
          </button>
        </div>
      </section>
    </>
  );
}

function BackendTab({ settings, update, connected }: { settings: Settings; update: <K extends keyof Settings>(k: K, v: Settings[K]) => void; connected: boolean }) {
  return (
    <>
      <section className="nx-section">
        <div className="nx-section-title">Server</div>
        <div className="nx-row">
          <div className="nx-row-label">
            <span className="nx-row-name">Server URL</span>
            <span className="nx-row-hint">Your n8n + Ollama backend server address</span>
          </div>
          <input
            type="url"
            className="nx-input"
            placeholder="https://your-server.com:41098"
            value={settings.serverUrl}
            onChange={(e) => update("serverUrl", e.target.value)}
          />
        </div>
        <div className="nx-row">
          <div className="nx-row-label">
            <span className="nx-row-name">Connection status</span>
          </div>
          <div className="nx-status-indicator">
            <span className={`nx-status-dot ${connected ? "nx-status-dot--ok" : "nx-status-dot--error"}`} />
            {connected ? "Connected" : "Not connected"}
          </div>
        </div>
      </section>

      <section className="nx-section">
        <div className="nx-section-title">Identity</div>
        <div className="nx-row">
          <div className="nx-row-label">
            <span className="nx-row-name">User ID</span>
            <span className="nx-row-hint">Identifies you on the backend</span>
          </div>
          <input className="nx-input" value={settings.userId} onChange={(e) => update("userId", e.target.value)} />
        </div>
        <div className="nx-row">
          <div className="nx-row-label">
            <span className="nx-row-name">Device ID</span>
            <span className="nx-row-hint">Identifies this device</span>
          </div>
          <input className="nx-input" value={settings.deviceId} onChange={(e) => update("deviceId", e.target.value)} />
        </div>
      </section>

      <section className="nx-section">
        <div className="nx-section-title">Credentials</div>
        <div className="nx-row">
          <div className="nx-row-label">
            <span className="nx-row-name">OAuth & API Keys</span>
            <span className="nx-row-hint">Manage Google, GitHub, and API key connections</span>
          </div>
          <button className="nx-btn" onClick={() => invoke("open_setup_window").catch(() => {})}>
            Manage →
          </button>
        </div>
      </section>

      <section className="nx-section">
        <div className="nx-section-title">Cloud STT (Groq)</div>
        <div className="nx-row">
          <div className="nx-row-label">
            <span className="nx-row-name">Groq API Key</span>
            <span className="nx-row-hint">Free at console.groq.com — 2,000 commands/day, forever free</span>
          </div>
          <input
            type="password"
            className="nx-input"
            placeholder="gsk_..."
            value={settings.groqApiKey || ""}
            onChange={(e) => update("groqApiKey", e.target.value)}
          />
        </div>
        <div className="nx-row">
          <div className="nx-row-label">
            <span className="nx-row-name">Don't have a key?</span>
            <span className="nx-row-hint">Sign up free at console.groq.com (Google/GitHub login, no credit card)</span>
          </div>
          <button className="nx-btn" onClick={() => window.open("https://console.groq.com/keys", "_blank")}>
            Get Free Key →
          </button>
        </div>
      </section>

      <section className="nx-section">
        <div className="nx-section-title">TTS Engine Status</div>
        <div className="nx-row">
          <div className="nx-row-label">
            <span className="nx-row-name">Primary Engine</span>
            <span className="nx-row-hint">Edge TTS (Microsoft Neural, cloud, free)</span>
          </div>
          <span className="nx-status-indicator">
            <span className="nx-status-dot nx-status-dot--ok" />
            Active
          </span>
        </div>
        <div className="nx-row">
          <div className="nx-row-label">
            <span className="nx-row-name">Fallback Engine</span>
            <span className="nx-row-hint">Kokoro (local ONNX, ~260-370 MB RAM while loaded) — used only when the network is down, unloaded after 10 min of recovery</span>
          </div>
          <span className="nx-status-indicator">
            <span className="nx-status-dot nx-status-dot--ok" />
            Standby
          </span>
        </div>
        <div className="nx-row">
          <div className="nx-row-label">
            <span className="nx-row-name">Cost</span>
            <span className="nx-row-hint">Free forever — no API key, no account, no limit</span>
          </div>
          <span className="nx-status-indicator">
            <span className="nx-status-dot nx-status-dot--ok" />
            Free
          </span>
        </div>
      </section>
    </>
  );
}

// ── Accounts & Integrations Tab ──────────────────────────────────────

interface GoogleAccountProfile {
  email: string;
  name: string;
  picture?: string | null;
  is_primary: boolean;
  added_at_ms?: number;
}

function AccountsTab() {
  const [googleAccounts, setGoogleAccounts] = useState<GoogleAccountProfile[]>([]);
  const [googleNeedsSignin, setGoogleNeedsSignin] = useState(false);
  const [credStatus, setCredStatus] = useState<{ configured: boolean; has_client_id: boolean; has_secret: boolean; message?: string | null } | null>(null);
  const [connecting, setConnecting] = useState(false);
  const [showCustomCreds, setShowCustomCreds] = useState(false);
  const [clientId, setClientId] = useState("");
  const [clientSecret, setClientSecret] = useState("");
  const [confirmDelete, setConfirmDelete] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [success, setSuccess] = useState<string | null>(null);

  const loadData = useCallback(async () => {
    try {
      const accs = await invoke<GoogleAccountProfile[]>("google_get_accounts");
      setGoogleAccounts(accs);
    } catch (e) {
      console.warn("Failed to load google accounts", e);
    }
    try {
      const ms = await invoke<any>("memcore_status");
      if (ms && typeof ms.google_needs_signin === "boolean") {
        setGoogleNeedsSignin(ms.google_needs_signin);
      }
    } catch {
      /* advisory */
    }
    try {
      const cs = await invoke<any>("google_credentials_status");
      setCredStatus(cs);
      if (!cs.configured) {
        setShowCustomCreds(true);
      }
    } catch {
      /* advisory */
    }
  }, []);

  useEffect(() => {
    void loadData();
  }, [loadData]);

  const handleConnectGoogle = async () => {
    setError(null);
    setSuccess(null);
    setConnecting(true);
    try {
      await invoke("google_connect_account");
      await loadData();
      setSuccess("Google account authenticated and synced successfully!");
      setTimeout(() => setSuccess(null), 4000);
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
    } finally {
      setConnecting(false);
    }
  };

  const handleSetPrimary = async (email: string) => {
    setError(null);
    try {
      await invoke("google_set_primary_account", { email });
      await loadData();
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
    }
  };

  const handleDisconnect = async (email: string) => {
    setError(null);
    try {
      await invoke("google_disconnect_account", { email });
      setConfirmDelete(null);
      await loadData();
      setSuccess(`Disconnected ${email}`);
      setTimeout(() => setSuccess(null), 3000);
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
    }
  };

  const handleSaveCustomCreds = async () => {
    setError(null);
    try {
      await invoke("google_save_custom_credentials", {
        clientId,
        clientSecret: clientSecret || null,
      });
      setClientSecret("");
      await loadData();
      setSuccess("Developer credentials saved successfully!");
      setTimeout(() => setSuccess(null), 3000);
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
    }
  };

  return (
    <>
      {error && (
        <div style={{ padding: "12px 16px", borderRadius: "8px", background: "var(--nx-error-bg)", border: "1px solid rgba(239,68,68,0.3)", color: "var(--nx-error)", fontSize: "var(--nx-text-sm)" }}>
          {error}
        </div>
      )}

      {success && (
        <div style={{ padding: "12px 16px", borderRadius: "8px", background: "var(--nx-success-bg)", border: "1px solid rgba(34,197,94,0.3)", color: "var(--nx-success)", fontSize: "var(--nx-text-sm)" }}>
          {success}
        </div>
      )}

      {/* Google Accounts Section */}
      <section className="nx-section">
        <div className="nx-section-title">Google Accounts (Gmail & Calendar)</div>
        <p style={{ fontSize: "var(--nx-text-xs)", color: "var(--nx-text-tertiary)", marginTop: "-8px" }}>
          Connect your Google account for proactive email alerts, morning briefing, and leave-by timetable checks.
        </p>

        {googleAccounts.length === 0 ? (
          <div style={{ padding: "20px", textAlign: "center", border: "1px dashed var(--nx-border)", borderRadius: "8px", background: "var(--nx-bg)" }}>
            <p style={{ fontSize: "var(--nx-text-sm)", color: "var(--nx-text-secondary)", marginBottom: "14px" }}>
              No Google account connected. Connect your Google account to get started.
            </p>
            <button
              type="button"
              className="nx-btn nx-btn--primary"
              disabled={connecting}
              onClick={handleConnectGoogle}
            >
              {connecting ? "Opening browser for sign-in..." : "+ Connect Google Account"}
            </button>
          </div>
        ) : (
          <div style={{ display: "flex", flexDirection: "column", gap: "12px" }}>
            {googleAccounts.map((acc) => {
              const isExpired = googleNeedsSignin;
              return (
                <div
                  key={acc.email}
                  style={{
                    display: "flex",
                    flexDirection: "column",
                    gap: "10px",
                    padding: "16px",
                    border: isExpired ? "1px solid rgba(245,158,11,0.5)" : "1px solid var(--nx-border)",
                    borderRadius: "8px",
                    background: isExpired ? "rgba(245,158,11,0.04)" : "var(--nx-bg)",
                  }}
                >
                  <div style={{ display: "flex", alignItems: "center", justifyContent: "space-between", gap: "12px" }}>
                    <div style={{ display: "flex", alignItems: "center", gap: "12px" }}>
                      {acc.picture ? (
                        <img
                          src={acc.picture}
                          alt=""
                          style={{ width: 42, height: 42, borderRadius: "50%", objectFit: "cover", border: "1px solid var(--nx-border)" }}
                        />
                      ) : (
                        <div
                          style={{
                            width: 42,
                            height: 42,
                            borderRadius: "50%",
                            background: "var(--nx-accent-blue)",
                            color: "white",
                            display: "flex",
                            alignItems: "center",
                            justifyContent: "center",
                            fontSize: "16px",
                            fontWeight: 600,
                          }}
                        >
                          {acc.name?.charAt(0) || acc.email.charAt(0).toUpperCase()}
                        </div>
                      )}
                      <div>
                        <div style={{ display: "flex", alignItems: "center", gap: "8px" }}>
                          <span style={{ fontWeight: 600, fontSize: "var(--nx-text-base)" }}>{acc.name || acc.email}</span>
                          {acc.is_primary && <span className="nx-badge nx-badge--ok">Primary</span>}
                          {isExpired ? (
                            <span className="nx-badge nx-badge--warn">Sign-In Expired</span>
                          ) : (
                            <span className="nx-badge nx-badge--ok">Active</span>
                          )}
                        </div>
                        <div style={{ fontSize: "var(--nx-text-xs)", color: "var(--nx-text-tertiary)", marginTop: "2px" }}>
                          {acc.email}
                        </div>
                      </div>
                    </div>

                    <div style={{ display: "flex", alignItems: "center", gap: "8px" }}>
                      {/* Re-authenticate Button */}
                      <button
                        type="button"
                        className={`nx-btn ${isExpired ? "nx-btn--primary" : ""}`}
                        disabled={connecting}
                        onClick={handleConnectGoogle}
                        title="Re-authenticates token via Google OAuth loopback (127.0.0.1:49152)"
                      >
                        {connecting ? "Signing in..." : isExpired ? "Re-authenticate Now →" : "Re-authenticate"}
                      </button>

                      {/* Primary toggle */}
                      {!acc.is_primary && (
                        <button
                          type="button"
                          className="nx-btn nx-btn--small"
                          onClick={() => handleSetPrimary(acc.email)}
                        >
                          Make Primary
                        </button>
                      )}

                      {/* Disconnect button */}
                      {confirmDelete === acc.email ? (
                        <div style={{ display: "flex", gap: "6px" }}>
                          <button
                            type="button"
                            className="nx-btn nx-btn--small"
                            onClick={() => setConfirmDelete(null)}
                          >
                            Keep
                          </button>
                          <button
                            type="button"
                            className="nx-btn nx-btn--small nx-btn--danger"
                            onClick={() => handleDisconnect(acc.email)}
                          >
                            Confirm Disconnect
                          </button>
                        </div>
                      ) : (
                        <button
                          type="button"
                          className="nx-btn nx-btn--small nx-btn--danger"
                          onClick={() => setConfirmDelete(acc.email)}
                        >
                          Disconnect
                        </button>
                      )}
                    </div>
                  </div>

                  {isExpired && (
                    <div style={{ padding: "8px 12px", borderRadius: "6px", background: "rgba(245,158,11,0.12)", border: "1px solid rgba(245,158,11,0.3)", fontSize: "var(--nx-text-xs)", color: "#b45309" }}>
                      ⚠️ <strong>Token Expired:</strong> Google Cloud OAuth apps in Testing mode expire refresh tokens after 7 days. Click <strong>Re-authenticate Now</strong> to refresh the connection in one click.
                    </div>
                  )}
                </div>
              );
            })}

            <div style={{ marginTop: "8px" }}>
              <button
                type="button"
                className="nx-btn"
                disabled={connecting}
                onClick={handleConnectGoogle}
              >
                + Connect Another Google Account
              </button>
            </div>
          </div>
        )}
      </section>

      {/* Developer OAuth Credentials Section */}
      <section className="nx-section">
        <div style={{ display: "flex", alignItems: "center", justifyContent: "space-between" }}>
          <div>
            <div className="nx-section-title">Developer OAuth Credentials</div>
            <div style={{ fontSize: "var(--nx-text-xs)", color: "var(--nx-text-tertiary)", marginTop: "2px" }}>
              Custom Google Cloud Console Client ID & Secret for desktop OAuth
            </div>
          </div>
          <button
            type="button"
            className="nx-btn nx-btn--small"
            onClick={() => setShowCustomCreds(!showCustomCreds)}
          >
            {showCustomCreds ? "Hide Setup" : "Configure Credentials"}
          </button>
        </div>

        <div className="nx-row">
          <div className="nx-row-label">
            <span className="nx-row-name">Client Configuration Status</span>
            <span className="nx-row-hint">{credStatus?.message || (credStatus?.configured ? "Client ID and Client Secret configured." : "Not configured.")}</span>
          </div>
          <span className="nx-status-indicator">
            <span className={`nx-status-dot ${credStatus?.configured ? "nx-status-dot--ok" : "nx-status-dot--error"}`} />
            {credStatus?.configured ? "Ready" : "Setup Required"}
          </span>
        </div>

        {showCustomCreds && (
          <div style={{ display: "flex", flexDirection: "column", gap: "12px", padding: "16px", borderRadius: "8px", background: "var(--nx-bg)", border: "1px solid var(--nx-border)" }}>
            <div style={{ fontSize: "var(--nx-text-xs)", color: "var(--nx-text-secondary)", lineHeight: 1.5 }}>
              Create an OAuth client of type <strong>Desktop App</strong> in Google Cloud Console with the <strong>Gmail API</strong> and <strong>Google Calendar API</strong> enabled, then paste the Client ID and Secret below:
            </div>
            <div style={{ display: "flex", flexDirection: "column", gap: "6px" }}>
              <label style={{ fontSize: "var(--nx-text-xs)", fontWeight: 600 }}>Client ID</label>
              <input
                className="nx-input"
                placeholder="123456789012-abcdef....apps.googleusercontent.com"
                value={clientId}
                onChange={(e) => setClientId(e.target.value)}
              />
            </div>
            <div style={{ display: "flex", flexDirection: "column", gap: "6px" }}>
              <label style={{ fontSize: "var(--nx-text-xs)", fontWeight: 600 }}>Client Secret</label>
              <input
                type="password"
                className="nx-input"
                placeholder="GOCSPX-..."
                value={clientSecret}
                onChange={(e) => setClientSecret(e.target.value)}
              />
            </div>
            <div style={{ display: "flex", gap: "8px", marginTop: "4px" }}>
              <button
                type="button"
                className="nx-btn nx-btn--primary"
                onClick={handleSaveCustomCreds}
              >
                Save Credentials
              </button>
              <button
                type="button"
                className="nx-btn"
                onClick={() => window.open("https://console.cloud.google.com/apis/credentials", "_blank")}
              >
                Open Google Cloud Console →
              </button>
            </div>
          </div>
        )}
      </section>

      {/* Cloud Integrations Section */}
      <section className="nx-section">
        <div className="nx-section-title">Cloud Integrations</div>
        <div className="nx-row">
          <div className="nx-row-label">
            <span className="nx-row-name">GitHub Integration</span>
            <span className="nx-row-hint">PR list, issue reviewer, and code search capabilities</span>
          </div>
          <button
            type="button"
            className="nx-btn"
            onClick={() => invoke("open_setup_window").catch(() => {})}
          >
            Manage via Setup →
          </button>
        </div>
      </section>

      {/* MCP Servers Connections & Control */}
      <section className="nx-section" style={{ minHeight: "560px" }}>
        <McpServersPanel style={{ height: "580px" }} />
      </section>
    </>
  );
}


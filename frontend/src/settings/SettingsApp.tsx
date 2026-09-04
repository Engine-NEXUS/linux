import { useState, useEffect, useCallback } from "react";
import { invoke } from "@tauri-apps/api/core";

import { CURATED_VOICES, previewVoice, stopTts } from "../audio/ttsPlayer";

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

type Tab = "general" | "audio" | "wake" | "privacy" | "backend";

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
}

const DEFAULT_SETTINGS: Settings = {
  autostart: true,
  hotkey: "Super+Space",
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
  ttsVoice: "af_sky",
  speechRate: 1.15,
};

const TABS: { id: Tab; label: string; icon: string }[] = [
  { id: "general", label: "General", icon: "M3 12l2-2m0 0l7-7 7 7M5 10v10a1 1 0 001 1h3m10-11l2 2m-2-2v10a1 1 0 01-1 1h-3m-6 0a1 1 0 001-1v-4a1 1 0 011-1h2a1 1 0 011 1v4a1 1 0 001 1m-6 0h6" },
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

  // Load settings from Rust on mount
  useEffect(() => {
    invoke<Partial<Settings>>("get_settings").then((s) => {
      if (s) setSettings({ ...DEFAULT_SETTINGS, ...s });
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
            <span className="nx-row-hint">Super+Space — DE keybind to nexus --wake (fixed)</span>
          </div>
          <input className="nx-input" value={settings.hotkey} disabled readOnly />
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
    </>
  );
}

function AudioTab({ settings, update }: { settings: Settings; update: <K extends keyof Settings>(k: K, v: Settings[K]) => void }) {
  const [playingVoice, setPlayingVoice] = useState<string | null>(null);

  const handlePreview = async (voiceId: string) => {
    if (playingVoice === voiceId) {
      stopTts();
      setPlayingVoice(null);
      return;
    }
    const voice = CURATED_VOICES.find((v) => v.id === voiceId) || {
      id: voiceId,
      name: voiceId,
      provider: "system" as const,
      accent: "Default",
      description: "Default voice",
      locale: "en-US",
      gender: "male" as const,
      sampleText: "Hello sir, NEXUS is ready to assist you.",
    };
    setPlayingVoice(voiceId);
    await previewVoice(voice, undefined, () => {
      setPlayingVoice(null);
    }, settings.speechRate);
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
        <div className="nx-section-title">Persona & Voice Engine</div>
        <div className="nx-row">
          <div className="nx-row-label">
            <span className="nx-row-name">Assistant Voice</span>
            <span className="nx-row-hint">Curated AI persona voice for spoken answers</span>
          </div>
          <div style={{ display: "flex", alignItems: "center", gap: "8px" }}>
            <select
              className="nx-select"
              value={settings.ttsVoice}
              onChange={(e) => update("ttsVoice", e.target.value)}
            >
              {CURATED_VOICES.map((v) => (
                <option key={v.id} value={v.id}>
                  {v.name} ({v.accent})
                </option>
              ))}
              <option value="default">System Default (Local)</option>
            </select>
            <button
              type="button"
              className="nx-btn"
              style={{ padding: "6px 12px", fontSize: "var(--nx-text-xs)" }}
              onClick={() => handlePreview(settings.ttsVoice)}
            >
              {playingVoice === settings.ttsVoice ? "⏹ Stop" : "▶ Play Sample"}
            </button>
          </div>
        </div>

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
            <span className="nx-row-hint">Fixed backend (hardcoded, not configurable)</span>
          </div>
          <input
            type="url"
            className="nx-input"
            value={settings.serverUrl}
            disabled
            readOnly
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
    </>
  );
}

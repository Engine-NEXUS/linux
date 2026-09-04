import { useState, useEffect, useCallback, useRef } from "react";
import { invoke } from "@tauri-apps/api/core";

import {
  setSidecarBaseUrl,
  connectOAuth,
  getOAuthStatus,
  type OAuthStatus,
} from "./oauth";
import { VoiceEnrollment } from "./VoiceEnrollment";
import { CURATED_VOICES, previewVoice, stopTts, type VoiceOption } from "../audio/ttsPlayer";

type Step = 0 | 1 | 2 | 3;
const STEP_LABELS = ["Persona & Voice", "Permissions", "Preferences", "Accounts"];

export function SetupApp() {
  const [step, setStep] = useState<Step>(0);
  const [serverUrl, setServerUrl] = useState("");
  const [userId, setUserId] = useState("");
  const [selectedVoice, setSelectedVoice] = useState<string>("af_sky");
  const [playingVoice, setPlayingVoice] = useState<string | null>(null);

  // Settings
  const [hotkey] = useState("Super+Space");
  const [wakeWordEnabled, setWakeWordEnabled] = useState(true);
  const [autostart, setAutostart] = useState(true);

  // Mic permission
  const [micStatus, setMicStatus] = useState<"checking" | "granted" | "denied" | "no_device">("checking");
  const [micTesting, setMicTesting] = useState(false);
  const [micRetryCount, setMicRetryCount] = useState(0);
  const [micSkipped, setMicSkipped] = useState(false);
  const micPollRef = useRef<ReturnType<typeof setInterval> | null>(null);

  // Accounts
  const [oauthStatus, setOauthStatus] = useState<Record<string, OAuthStatus>>({});
  const [connecting, setConnecting] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [saved, setSaved] = useState(false);

  // Load current settings and server config
  useEffect(() => {
    invoke<{ serverUrl: string; userId: string; deviceId: string }>("get_server_config")
      .then((cfg) => {
        setServerUrl(cfg.serverUrl);
        setUserId(cfg.userId);
      })
      .catch(() => {});

    invoke<any>("get_settings")
      .then((s) => {
        if (s) {
          if (s.ttsVoice) setSelectedVoice(s.ttsVoice);
          if (typeof s.wakeWordEnabled === "boolean") setWakeWordEnabled(s.wakeWordEnabled);
          if (typeof s.autostart === "boolean") setAutostart(s.autostart);
        }
      })
      .catch(() => {});
  }, []);

  // Auto-check mic permission when entering the Permissions step
  useEffect(() => {
    if (step === 1) {
      runMicCheck();
    }
  }, [step]);

  const handlePreview = async (voice: VoiceOption, e: React.MouseEvent) => {
    e.stopPropagation();
    if (playingVoice === voice.id) {
      stopTts();
      setPlayingVoice(null);
      return;
    }
    setPlayingVoice(voice.id);
    await previewVoice(voice, undefined, () => {
      setPlayingVoice(null);
    });
  };

  const checkServer = useCallback(async () => {
    if (!serverUrl || !userId) return;
    setSidecarBaseUrl(serverUrl);
    try {
      const status = await getOAuthStatus(userId);
      setOauthStatus(status);
      setError(null);
    } catch {
      // Server unreachable
    }
  }, [serverUrl, userId]);

  useEffect(() => {
    if (step === 3) checkServer();
  }, [step, checkServer]);

  const handleConnect = async (provider: "google" | "github") => {
    // Fallback: if serverUrl hasn't loaded yet, try loading it now
    let url = serverUrl;
    if (!url) {
      try {
        const cfg = await invoke<{ serverUrl: string; userId: string; deviceId: string }>("get_server_config");
        url = cfg.serverUrl;
        setServerUrl(cfg.serverUrl);
        setUserId(cfg.userId);
      } catch {
        setError("Server not configured");
        return;
      }
    }
    if (!url) {
      setError("Server not configured");
      return;
    }
    setConnecting(provider);
    setError(null);
    try {
      setSidecarBaseUrl(url);
      await connectOAuth(provider, userId);
      await checkServer();
    } catch (err) {
      setError(`${provider} connection failed: ${err instanceof Error ? err.message : String(err)}`);
    } finally {
      setConnecting(null);
    }
  };

  // ─── Mic permission check with auto-retry ──────────────────────
  //
  // Two-stage check:
  //   1. Rust-side: check_mic_permission() probes cpal for the default input
  //      device. Detects if Windows mic privacy is globally off.
  //   2. Frontend-side: getUserMedia() to trigger the WebView2/browser mic
  //      permission prompt and verify the user actually grants it.
  //
  // If permission is denied, we start a background poll that re-checks every
  // 3 seconds. This way, when the user fixes Windows Settings → Privacy →
  // Microphone, the setup auto-detects the change without requiring them
  // to manually click "Check Again".

  const stopMicPoll = useCallback(() => {
    if (micPollRef.current) {
      clearInterval(micPollRef.current);
      micPollRef.current = null;
    }
  }, []);

  // Use a ref to hold runMicCheck so startMicPoll can call it without
  // creating a circular dependency.
  const runMicCheckRef = useRef<(isAutoRetry?: boolean) => Promise<void>>(async () => {});

  const startMicPoll = useCallback(() => {
    if (micPollRef.current) return; // already polling
    setMicRetryCount(0);
    micPollRef.current = setInterval(async () => {
      setMicRetryCount((c) => c + 1);
      console.log("[NEXUS] setup: auto-retrying mic permission check...");
      await runMicCheckRef.current(true);
    }, 3000);
  }, []);

  const runMicCheck = async (isAutoRetry = false) => {
    if (!isAutoRetry) {
      setMicStatus("checking");
    }
    setMicSkipped(false);

    // Stage 1: Rust probe (checks OS-level mic privacy)
    try {
      const result = await invoke<string>("check_mic_permission");
      if (result === "no_device") {
        setMicStatus("no_device");
        stopMicPoll();
        return;
      }
      if (result === "denied") {
        setMicStatus("denied");
        // Start auto-retry polling — user likely needs to fix Windows Settings
        startMicPoll();
        return;
      }
    } catch (e) {
      console.warn("check_mic_permission failed:", e);
      // Continue to getUserMedia check anyway
    }

    // Stage 2: Frontend getUserMedia (triggers WebView2 permission prompt)
    if (!isAutoRetry) setMicTesting(true);
    try {
      const stream = await navigator.mediaDevices.getUserMedia({
        audio: {
          channelCount: 1,
          echoCancellation: true,
          noiseSuppression: true,
        },
      });
      // Success — stop the tracks immediately, we just wanted permission
      stream.getTracks().forEach((t) => t.stop());
      setMicStatus("granted");
      setMicRetryCount(0);
      stopMicPoll();
      console.log("[NEXUS] setup: mic permission granted");
    } catch (err) {
      console.error("[NEXUS] setup: mic permission denied:", err);
      const name = (err as Error)?.name;
      if (name === "NotAllowedError" || name === "PermissionDeniedError") {
        setMicStatus("denied");
      } else if (name === "NotFoundError" || name === "DevicesNotFoundError") {
        setMicStatus("no_device");
        stopMicPoll();
        return;
      } else {
        setMicStatus("denied");
      }
      // Start auto-retry polling if not already running
      startMicPoll();
    } finally {
      setMicTesting(false);
    }
  };

  // Keep the ref in sync with the latest runMicCheck closure
  useEffect(() => {
    runMicCheckRef.current = runMicCheck;
  });

  // Stop polling when leaving the Permissions step or on unmount
  useEffect(() => {
    if (step !== 1) {
      stopMicPoll();
    }
    return () => {
      if (step === 1) stopMicPoll();
    };
  }, [step, stopMicPoll]);

  // Cleanup on unmount
  useEffect(() => {
    return () => stopMicPoll();
  }, [stopMicPoll]);

  const handleOpenMicSettings = async () => {
    try {
      await invoke("open_mic_settings");
    } catch (e) {
      console.error("open_mic_settings failed:", e);
    }
  };

  const saveAllSettings = async () => {
    try {
      const current = (await invoke<any>("get_settings").catch(() => ({}))) || {};
      const updated = {
        ...current,
        ttsVoice: selectedVoice,
        hotkey,
        wakeWordEnabled,
        autostart,
      };
      await invoke("save_settings", { settings: updated });
    } catch (e) {
      console.warn("Failed to persist settings:", e);
    }
  };

  const handleFinish = async () => {
    try {
      await saveAllSettings();
      // Sync autostart with the OS before closing setup
      try {
        await invoke("set_autostart", { enabled: autostart });
      } catch (e) {
        console.warn("set_autostart failed:", e);
      }
      await invoke("close_setup_window", { firstRun: true });
      setSaved(true);
    } catch (err) {
      setError(`Failed to finish: ${err instanceof Error ? err.message : String(err)}`);
    }
  };

  // Prevent advancing past Permissions step if mic is not granted.
  // User can skip with a warning, but only via the explicit "Skip" button.
  const canAdvanceFromPermissions = micStatus === "granted" || micSkipped;

  return (
    <div className="setup-root">
      {error && <div className="setup-error">{error}</div>}

      <div style={{ flex: 1 }}>
        {/* ── Step 0: Voice & Persona ── */}
          {step === 0 && (
            <div>
              <div style={{ textAlign: "center", marginBottom: "var(--nx-space-5)" }}>
                <h1 style={{ fontSize: "var(--nx-text-xl)", fontWeight: "bold", color: "var(--nx-text-primary)" }}>
                  Choose Your Assistant Persona
                </h1>
                <p style={{ color: "var(--nx-text-secondary)", fontSize: "var(--nx-text-sm)", marginTop: "4px" }}>
                  Select the voice & tone for NEXUS. You can change this anytime.
                </p>
              </div>

              <div className="setup-voice-grid">
                {CURATED_VOICES.map((voice) => {
                  const isSelected = selectedVoice === voice.id;
                  const isPlaying = playingVoice === voice.id;
                  return (
                    <div
                      key={voice.id}
                      className={`setup-voice-card ${isSelected ? "setup-voice-card--active" : ""}`}
                      onClick={() => setSelectedVoice(voice.id)}
                    >
                      <div className="setup-voice-card-header">
                        <span className="setup-voice-name">{voice.name}</span>
                        <span className="setup-voice-accent">{voice.accent}</span>
                      </div>
                      <p className="setup-voice-desc">{voice.description}</p>
                      <button
                        type="button"
                        className="setup-voice-play-btn"
                        onClick={(e) => handlePreview(voice, e)}
                      >
                        {isPlaying ? "⏹ Stop" : "▶ Play Sample"}
                      </button>
                    </div>
                  );
                })}
              </div>


            </div>
          )}

          {/* ── Step 1: Microphone Permission ── */}
          {step === 1 && (
            <>
              <StepHeader step={step} />
              <section className="setup-section">
                <h2>Microphone Access</h2>
                <p style={{ marginBottom: "var(--nx-space-4)", color: "var(--nx-text-secondary)", fontSize: "var(--nx-text-sm)" }}>
                  NEXUS needs microphone access to hear your wake word and voice commands.
                  Your audio never leaves this device — all speech recognition runs locally.
                </p>

                {/* Permission status card */}
                <div style={{
                  display: "flex",
                  flexDirection: "column",
                  alignItems: "center",
                  gap: "var(--nx-space-4)",
                  padding: "var(--nx-space-6)",
                  border: "1px solid var(--nx-border)",
                  borderRadius: "12px",
                  background: "var(--nx-bg-secondary)",
                }}>
                  {/* Status icon */}
                  <div style={{
                    width: "64px",
                    height: "64px",
                    borderRadius: "50%",
                    display: "flex",
                    alignItems: "center",
                    justifyContent: "center",
                    fontSize: "28px",
                    background: micStatus === "granted" ? "rgba(34,197,94,0.15)" :
                               micStatus === "denied" ? "rgba(239,68,68,0.15)" :
                               micStatus === "no_device" ? "rgba(234,179,8,0.15)" :
                               "rgba(59,130,246,0.15)",
                  }}>
                    {micStatus === "granted" && "✓"}
                    {micStatus === "denied" && "✕"}
                    {micStatus === "no_device" && "!"}
                    {(micStatus === "checking" || micTesting || (micStatus === "denied" && micRetryCount > 0)) && (
                      <div style={{
                        width: "28px",
                        height: "28px",
                        border: "3px solid rgba(59,130,246,0.3)",
                        borderTopColor: "rgba(59,130,246,0.8)",
                        borderRadius: "50%",
                        animation: "spin 0.8s linear infinite",
                      }} />
                    )}
                  </div>

                  {/* Status text */}
                  <div style={{ textAlign: "center" }}>
                    {micStatus === "checking" && (
                      <p style={{ fontSize: "var(--nx-text-sm)", color: "var(--nx-text-secondary)" }}>
                        Checking microphone access...
                      </p>
                    )}
                    {micTesting && (
                      <p style={{ fontSize: "var(--nx-text-sm)", color: "var(--nx-text-secondary)" }}>
                        Requesting permission — please click "Allow" if prompted...
                      </p>
                    )}
                    {micStatus === "granted" && (
                      <>
                        <p style={{ fontSize: "var(--nx-text-sm)", fontWeight: 600, color: "var(--nx-text-primary)" }}>
                          Microphone Ready
                        </p>
                        <p style={{ fontSize: "var(--nx-text-xs)", color: "var(--nx-text-secondary)", marginTop: "4px" }}>
                          NEXUS can hear you. Audio stays on this device.
                        </p>
                      </>
                    )}
                    {micStatus === "denied" && (
                      <>
                        <p style={{ fontSize: "var(--nx-text-sm)", fontWeight: 600, color: "#ef4444" }}>
                          Microphone Access Denied
                        </p>
                        <p style={{ fontSize: "var(--nx-text-xs)", color: "var(--nx-text-secondary)", marginTop: "4px", maxWidth: "340px" }}>
                          NEXUS needs microphone access to function. Enable it in Windows Settings →
                          Privacy → Microphone. NEXUS will auto-detect when you've enabled it.
                        </p>
                        {micRetryCount > 0 && (
                          <p style={{ fontSize: "var(--nx-text-xs)", color: "rgba(59,130,246,0.8)", marginTop: "8px" }}>
                            Auto-checking every 3s... (attempt {micRetryCount})
                          </p>
                        )}
                      </>
                    )}
                    {micStatus === "no_device" && (
                      <>
                        <p style={{ fontSize: "var(--nx-text-sm)", fontWeight: 600, color: "#eab308" }}>
                          No Microphone Found
                        </p>
                        <p style={{ fontSize: "var(--nx-text-xs)", color: "var(--nx-text-secondary)", marginTop: "4px" }}>
                          No input devices detected. Connect a microphone and click "Try Again".
                        </p>
                      </>
                    )}
                  </div>

                  {/* Action buttons */}
                  <div style={{ display: "flex", gap: "var(--nx-space-3)", flexWrap: "wrap", justifyContent: "center" }}>
                    {/* Manual retry button — always available when not granted */}
                    {micStatus !== "granted" && micStatus !== "checking" && !micTesting && (
                      <button
                        className="setup-btn setup-btn--primary"
                        onClick={() => runMicCheck(false)}
                      >
                        Try Again
                      </button>
                    )}
                    {/* Open Windows mic settings — shown when denied */}
                    {micStatus === "denied" && (
                      <button
                        className="setup-btn"
                        onClick={handleOpenMicSettings}
                      >
                        Open Windows Settings
                      </button>
                    )}
                  </div>

                  {/* Skip option — allows proceeding without mic, with warning */}
                  {micStatus !== "granted" && micStatus !== "checking" && !micTesting && !micSkipped && (
                    <button
                      style={{
                        background: "none",
                        border: "none",
                        color: "var(--nx-text-secondary)",
                        fontSize: "var(--nx-text-xs)",
                        cursor: "pointer",
                        textDecoration: "underline",
                        marginTop: "var(--nx-space-2)",
                      }}
                      onClick={() => {
                        stopMicPoll();
                        setMicSkipped(true);
                      }}
                    >
                      Skip for now (NEXUS won't hear you until mic is enabled)
                    </button>
                  )}
                  {micSkipped && micStatus !== "granted" && (
                    <p style={{ fontSize: "var(--nx-text-xs)", color: "#eab308", marginTop: "var(--nx-space-2)" }}>
                      ⚠ Skipped — NEXUS will start but voice commands won't work.
                      You can enable the mic later in Settings.
                    </p>
                  )}
                </div>

                {/* Privacy note */}
                <div style={{
                  marginTop: "var(--nx-space-4)",
                  padding: "12px 16px",
                  borderRadius: "8px",
                  background: "rgba(59,130,246,0.08)",
                  border: "1px solid rgba(59,130,246,0.2)",
                  fontSize: "var(--nx-text-xs)",
                  color: "var(--nx-text-secondary)",
                }}>
                  <strong style={{ color: "var(--nx-text-primary)" }}>Privacy:</strong> All speech recognition
                  runs locally via faster-whisper. Audio is never sent to the cloud. Only the transcribed
                  text is sent to the NEXUS Worker for intent processing.
                </div>
              </section>
            </>
          )}

          {/* ── Step 2: Preferences ── */}
          {step === 2 && (
            <>
              <StepHeader step={step} />
              <section className="setup-section">
                <h2>Interaction Controls</h2>
                <p style={{ marginBottom: "var(--nx-space-4)", color: "var(--nx-text-secondary)", fontSize: "var(--nx-text-sm)" }}>
                  Configure your primary wake triggers and startup settings.
                </p>

                <div style={{ display: "flex", flexDirection: "column", gap: "var(--nx-space-3)" }}>
                  <div style={{ display: "flex", justifyContent: "space-between", alignItems: "center", padding: "12px", border: "1px solid var(--nx-border)", borderRadius: "8px" }}>
                    <div>
                      <div style={{ fontWeight: 600, fontSize: "var(--nx-text-sm)" }}>Wake Word ("NEXUS")</div>
                      <div style={{ fontSize: "var(--nx-text-xs)", color: "var(--nx-text-secondary)" }}>Local neural keyword spotter (openWakeWord)</div>
                    </div>
                    <input
                      type="checkbox"
                      checked={wakeWordEnabled}
                      onChange={(e) => setWakeWordEnabled(e.target.checked)}
                      style={{ width: "18px", height: "18px", accentColor: "var(--nx-accent-blue)" }}
                    />
                  </div>

                  <div style={{ display: "flex", justifyContent: "space-between", alignItems: "center", padding: "12px", border: "1px solid var(--nx-border)", borderRadius: "8px" }}>
                    <div>
                      <div style={{ fontWeight: 600, fontSize: "var(--nx-text-sm)" }}>Global Hotkey</div>
                      <div style={{ fontSize: "var(--nx-text-xs)", color: "var(--nx-text-secondary)" }}>Super+Space — instantly wake/toggle assistant</div>
                    </div>
                    <div style={{ padding: "6px 10px", fontSize: "var(--nx-text-xs)", border: "1px solid var(--nx-border)", borderRadius: "6px", width: "140px", textAlign: "center", color: "var(--nx-text-secondary)", background: "var(--nx-surface-2)" }}>
                      Super+Space
                    </div>
                  </div>

                  <div style={{ display: "flex", justifyContent: "space-between", alignItems: "center", padding: "12px", border: "1px solid var(--nx-border)", borderRadius: "8px" }}>
                    <div>
                      <div style={{ fontWeight: 600, fontSize: "var(--nx-text-sm)" }}>Start at Login</div>
                      <div style={{ fontSize: "var(--nx-text-xs)", color: "var(--nx-text-secondary)" }}>Launch in system background on boot</div>
                    </div>
                    <input
                      type="checkbox"
                      checked={autostart}
                      onChange={(e) => setAutostart(e.target.checked)}
                      style={{ width: "18px", height: "18px", accentColor: "var(--nx-accent-blue)" }}
                    />
                  </div>
                </div>

                <div style={{ marginTop: "var(--nx-space-5)" }}>
                  <h3 style={{ fontSize: "var(--nx-text-sm)", marginBottom: "var(--nx-space-2)" }}>Voice Lock (Optional)</h3>
                  <VoiceEnrollment />
                </div>
              </section>
            </>
          )}

          {/* ── Step 3: Accounts ── */}
          {step === 3 && (
            <>
              <StepHeader step={step} />
              <section className="setup-section">
                <h2>Connect Integrations</h2>
                <p style={{ marginBottom: "var(--nx-space-4)", color: "var(--nx-text-secondary)", fontSize: "var(--nx-text-sm)" }}>
                  Connect Google and GitHub to let NEXUS manage your emails, calendar, and GitHub repos. (You can also skip and connect later).
                </p>

                {/* Google card */}
                <div className="setup-provider setup-provider--large">
                  <div className="setup-provider-icon setup-provider-icon--google">
                    <svg width="24" height="24" viewBox="0 0 24 24" fill="none">
                      <path d="M22.56 12.25c0-.78-.07-1.53-.2-2.25H12v4.26h5.92c-.26 1.37-1.04 2.53-2.21 3.31v2.77h3.57c2.08-1.92 3.28-4.74 3.28-8.09z" fill="#4285F4"/>
                      <path d="M12 23c2.97 0 5.46-.98 7.28-2.66l-3.57-2.77c-.98.66-2.23 1.06-3.71 1.06-2.86 0-5.29-1.93-6.16-4.53H2.18v2.84C3.99 20.53 7.7 23 12 23z" fill="#34A853"/>
                      <path d="M5.84 14.09c-.22-.66-.35-1.36-.35-2.09s.13-1.43.35-2.09V7.07H2.18C1.43 8.55 1 10.22 1 12s.43 3.45 1.18 4.93l2.85-2.22.81-.62z" fill="#FBBC05"/>
                      <path d="M12 5.38c1.62 0 3.06.56 4.21 1.64l3.15-3.15C17.45 2.09 14.97 1 12 1 7.7 1 3.99 3.47 2.18 7.07l3.66 2.84C6.71 7.31 9.14 5.38 12 5.38z" fill="#EA4335"/>
                    </svg>
                  </div>
                  <div className="setup-provider-info">
                    <h3>Google</h3>
                    <p>Gmail · Calendar · Meet</p>
                  </div>
                  {oauthStatus.google?.connected ? (
                    <span className="setup-badge setup-badge--ok">Connected</span>
                  ) : (
                    <button className="setup-btn setup-btn--primary setup-btn--small" disabled={connecting !== null} onClick={() => handleConnect("google")}>
                      {connecting === "google" ? "Connecting..." : "Connect"}
                    </button>
                  )}
                </div>

                {/* GitHub card */}
                <div className="setup-provider setup-provider--large">
                  <div className="setup-provider-icon setup-provider-icon--github">
                    <svg width="24" height="24" viewBox="0 0 24 24" fill="currentColor">
                      <path d="M12 0C5.37 0 0 5.37 0 12c0 5.31 3.435 9.795 8.205 11.385.6.105.825-.255.825-.57 0-.285-.015-1.23-.015-2.235-3.015.555-3.795-.735-4.035-1.41-.135-.345-.72-1.41-1.23-1.695-.42-.225-1.02-.78-.015-.795.945-.015 1.62.87 1.845 1.23 1.08 1.815 2.805 1.305 3.495.99.105-.78.42-1.305.765-1.605-2.67-.3-5.46-1.335-5.46-5.925 0-1.305.465-2.385 1.23-3.225-.12-.3-.54-1.53.12-3.18 0 0 1.005-.315 3.3 1.23.96-.27 1.98-.405 3-.405s2.04.135 3 .405c2.295-1.56 3.3-1.23 3.3-1.23.66 1.65.24 2.88.12 3.18.765.84 1.23 1.905 1.23 3.225 0 4.605-2.805 5.625-5.475 5.925.435.375.81 1.095.81 2.22 0 1.605-.015 2.895-.015 3.3 0 .315.225.69.825.57A12.02 12.02 0 0024 12c0-6.63-5.37-12-12-12z"/>
                    </svg>
                  </div>
                  <div className="setup-provider-info">
                    <h3>GitHub</h3>
                    <p>Repos · Pull Requests</p>
                  </div>
                  {oauthStatus.github?.connected ? (
                    <span className="setup-badge setup-badge--ok">Connected</span>
                  ) : (
                    <button className="setup-btn setup-btn--primary setup-btn--small" disabled={connecting !== null} onClick={() => handleConnect("github")}>
                      {connecting === "github" ? "Connecting..." : "Connect"}
                    </button>
                  )}
                </div>
              </section>
            </>
          )}
        </div>

      {/* ── Footer navigation ── */}
      <div className="setup-footer">
        {step > 0 ? (
          <button className="setup-btn" onClick={() => setStep((step - 1) as Step)}>
            ← Back
          </button>
        ) : (
          <div />
        )}
        <div style={{ display: "flex", alignItems: "center", gap: "var(--nx-space-3)" }}>
          {saved && <span className="setup-saved">Ready!</span>}
          {step < 3 ? (
            <button
              className="setup-btn setup-btn--primary"
              disabled={step === 1 && !canAdvanceFromPermissions}
              onClick={async () => {
                await saveAllSettings();
                setStep((step + 1) as Step);
              }}
            >
              Continue →
            </button>
          ) : (
            <button className="setup-btn setup-btn--primary" style={{ padding: "10px 24px" }} onClick={handleFinish}>
              🚀 Launch Assistant
            </button>
          )}
        </div>
      </div>
    </div>
  );
}

function StepHeader({ step }: { step: Step }) {
  return (
    <div className="setup-step-header">
      <div className="setup-step-indicator">
        {STEP_LABELS.map((label, i) => (
          <div key={label} style={{ display: "flex", alignItems: "center", gap: "var(--nx-space-2)", flex: i < 3 ? 1 : undefined }}>
            <div className={`setup-step-dot ${i === step ? "setup-step-dot--active" : ""} ${i < step ? "setup-step-dot--completed" : ""}`} />
            {i < 3 && <div className={`setup-step-bar ${i < step ? "setup-step-bar--completed" : ""}`} />}
          </div>
        ))}
      </div>
      <div className="setup-step-label">Step {step + 1} of 4</div>
      <div className="setup-step-title">{STEP_LABELS[step]}</div>
    </div>
  );
}

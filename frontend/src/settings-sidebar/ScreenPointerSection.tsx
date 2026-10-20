import { useEffect, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import {
  DEFAULT_POINTER_SETTINGS,
  clampDwell,
  formatExcludedApps,
  normalizePointerSettings,
  parseExcludedApps,
  type PointerSettings,
} from "./screenPointerModel";

type SaveStatus = "idle" | "saved" | "error";

/**
 * Command Hub card for the on-screen "screen pointer" marker.
 * Toggles save immediately; the dwell time and the excluded-app list save on
 * blur or Enter. The backend clamps values, so the saved reply is what shows.
 */
export function ScreenPointerSection() {
  const [settings, setSettings] = useState<PointerSettings>(
    DEFAULT_POINTER_SETTINGS,
  );
  const [dwellText, setDwellText] = useState(
    String(DEFAULT_POINTER_SETTINGS.dwellSeconds),
  );
  const [excludedText, setExcludedText] = useState("");
  const [disabled, setDisabled] = useState(false);
  const [status, setStatus] = useState<SaveStatus>("idle");
  const settingsRef = useRef<PointerSettings>(DEFAULT_POINTER_SETTINGS);
  const statusTimer = useRef<ReturnType<typeof setTimeout> | null>(null);
  const saveSeq = useRef(0);

  useEffect(() => {
    let alive = true;
    invoke<PointerSettings>("pointer_settings_get")
      .then((raw) => {
        if (!alive) return;
        // A toggle may already have saved while this load was in flight;
        // never let the older read overwrite the newer save.
        if (saveSeq.current > 0) return;
        const loaded = normalizePointerSettings(raw);
        settingsRef.current = loaded;
        setSettings(loaded);
        setDwellText(String(loaded.dwellSeconds));
        setExcludedText(formatExcludedApps(loaded.excludedApps));
      })
      .catch(() => {
        if (!alive) return;
        // No Tauri (or an older build without the command): keep the
        // defaults visible, but read-only.
        setDisabled(true);
      });
    return () => {
      alive = false;
    };
  }, []);

  useEffect(() => {
    return () => {
      if (statusTimer.current) clearTimeout(statusTimer.current);
    };
  }, []);

  const flash = (next: Exclude<SaveStatus, "idle">) => {
    setStatus(next);
    if (statusTimer.current) clearTimeout(statusTimer.current);
    statusTimer.current = setTimeout(() => {
      statusTimer.current = null;
      setStatus("idle");
    }, 1500);
  };

  const save = async (next: PointerSettings) => {
    if (disabled) return;
    const seq = ++saveSeq.current;
    try {
      const saved = normalizePointerSettings(
        await invoke<PointerSettings>("pointer_settings_save", {
          settings: next,
        }),
      );
      if (seq !== saveSeq.current) return;
      settingsRef.current = saved;
      setSettings(saved);
      setDwellText(String(saved.dwellSeconds));
      setExcludedText(formatExcludedApps(saved.excludedApps));
      flash("saved");
    } catch {
      if (seq !== saveSeq.current) return;
      flash("error");
    }
  };

  const toggle = (key: "enabled" | "suppressFullscreen", value: boolean) => {
    if (disabled) return;
    const next = { ...settingsRef.current, [key]: value };
    settingsRef.current = next;
    setSettings(next);
    void save(next);
  };

  const commitDwell = () => {
    if (disabled) return;
    const value = dwellText.trim() === "" ? Number.NaN : Number(dwellText);
    const dwell = clampDwell(value);
    setDwellText(String(dwell));
    void save({ ...settingsRef.current, dwellSeconds: dwell });
  };

  const commitExcludedApps = () => {
    if (disabled) return;
    const list = parseExcludedApps(excludedText);
    setExcludedText(formatExcludedApps(list));
    void save({ ...settingsRef.current, excludedApps: list });
  };

  return (
    <div className="settings-section">
      <div className="settings-section-title">Screen pointer</div>
      <div className="setting-desc" style={{ marginBottom: 8 }}>
        When you ask NEXUS to point at something, a marker appears on screen
        for a few seconds.
      </div>

      <div className="setting-row">
        <div>
          <div className="setting-label">Show marker</div>
          <div className="setting-desc">
            The on-screen marker when NEXUS points at something.
          </div>
        </div>
        <div className="setting-control">
          <button
            type="button"
            className={`settings-btn ${settings.enabled ? "settings-btn--primary" : "settings-btn--glass"}`}
            disabled={disabled}
            onClick={() => toggle("enabled", !settingsRef.current.enabled)}
          >
            {settings.enabled ? "On" : "Off"}
          </button>
        </div>
      </div>

      <div className="setting-row">
        <div>
          <div className="setting-label">Marker duration</div>
          <div className="setting-desc">
            Seconds the marker stays on screen (3 to 15).
          </div>
        </div>
        <div className="setting-control">
          <input
            type="number"
            className="settings-input"
            style={{ width: 80 }}
            min={3}
            max={15}
            step={1}
            value={dwellText}
            disabled={disabled}
            aria-label="Marker duration in seconds"
            onChange={(e) => setDwellText(e.target.value)}
            onBlur={commitDwell}
            onKeyDown={(e) => {
              if (e.key === "Enter") commitDwell();
            }}
          />
        </div>
      </div>

      <div className="setting-row">
        <div>
          <div className="setting-label">Hide over full-screen apps</div>
          <div className="setting-desc">
            Keeps the marker out of the way while an app is full screen.
          </div>
        </div>
        <div className="setting-control">
          <button
            type="button"
            className={`settings-btn ${settings.suppressFullscreen ? "settings-btn--primary" : "settings-btn--glass"}`}
            disabled={disabled}
            onClick={() =>
              toggle(
                "suppressFullscreen",
                !settingsRef.current.suppressFullscreen,
              )
            }
          >
            {settings.suppressFullscreen ? "On" : "Off"}
          </button>
        </div>
      </div>

      <div
        className="setting-row"
        style={{ flexDirection: "column", alignItems: "stretch", gap: 6 }}
      >
        <div>
          <div className="setting-label">Excluded apps</div>
          <div className="setting-desc">
            Screen capture and the marker stay off while a window title
            contains one of these.
          </div>
        </div>
        <input
          type="text"
          className="settings-input"
          value={excludedText}
          disabled={disabled}
          placeholder="bank, password manager"
          aria-label="Excluded app title fragments, comma separated"
          onChange={(e) => setExcludedText(e.target.value)}
          onBlur={commitExcludedApps}
          onKeyDown={(e) => {
            if (e.key === "Enter") commitExcludedApps();
          }}
        />
      </div>

      {status !== "idle" && (
        <div className="setting-desc" role="status" aria-live="polite">
          {status === "saved" ? "Saved" : "Could not save"}
        </div>
      )}
      {disabled && (
        <div className="setting-desc">
          Read-only: the desktop backend is not available here.
        </div>
      )}
    </div>
  );
}

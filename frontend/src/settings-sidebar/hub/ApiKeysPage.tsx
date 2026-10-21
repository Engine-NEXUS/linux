import { useCallback, useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";

interface ApiKeyInfo {
  service: string;
  label: string;
  has_key: boolean;
  masked: string;
}

const HINTS: Record<string, string> = {
  groq: "Speech-to-text and fast chat · console.groq.com",
  gemini: "Screen vision and the screen tour · aistudio.google.com",
  cerebras: "Fastest free chat model · cloud.cerebras.ai",
};

/**
 * API Keys page: add, replace and delete keys. Keys go straight to the OS
 * keychain via Rust; the full key is never shown again — only a masked tail.
 */
export function ApiKeysPage({ onChanged }: { onChanged: () => void }) {
  const [keys, setKeys] = useState<ApiKeyInfo[]>([]);
  const [editing, setEditing] = useState<string | null>(null);
  const [draft, setDraft] = useState("");
  const [confirmDelete, setConfirmDelete] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);

  const load = useCallback(async () => {
    try {
      setKeys(await invoke<ApiKeyInfo[]>("api_keys_list"));
    } catch (e) {
      setError(String(e));
    }
  }, []);
  useEffect(() => {
    void load();
  }, [load]);

  const save = async (service: string) => {
    setError(null);
    setBusy(true);
    try {
      await invoke("api_key_set", { service, key: draft });
      setDraft("");
      setEditing(null);
      await load();
      onChanged();
    } catch (e) {
      setError(typeof e === "string" ? e : String(e));
    } finally {
      setBusy(false);
    }
  };

  const del = async (service: string) => {
    setError(null);
    setBusy(true);
    try {
      await invoke("api_key_delete", { service });
      setConfirmDelete(null);
      await load();
      onChanged();
    } catch (e) {
      setError(String(e));
    } finally {
      setBusy(false);
    }
  };

  return (
    <div className="hx-page">
      <div className="hx-note">
        Keys are stored in your Windows keychain. They are never shown in full — replace or delete them here.
      </div>
      {error && <div className="hx-error">{error}</div>}
      {keys.map((k) => (
        <div className="hx-card hx-key" key={k.service}>
          <div className="hx-key-head">
            <div>
              <div className="hx-key-title">{k.label}</div>
              <div className="hx-key-sub">{HINTS[k.service] ?? ""}</div>
            </div>
            <span className={`hx-pill ${k.has_key ? "hx-pill--on" : ""}`}>{k.has_key ? "Added" : "Not set"}</span>
          </div>
          {k.has_key && <div className="hx-key-mask">{k.masked}</div>}

          {editing === k.service ? (
            <div className="hx-key-edit">
              <input
                className="hx-input"
                type="password"
                autoFocus
                placeholder={`Paste ${k.label} API key`}
                value={draft}
                onChange={(e) => setDraft(e.target.value)}
                onKeyDown={(e) => {
                  if (e.key === "Enter") void save(k.service);
                }}
              />
              <div className="hx-row-actions">
                <button
                  className="hx-btn"
                  onClick={() => {
                    setEditing(null);
                    setDraft("");
                    setError(null);
                  }}
                >
                  Cancel
                </button>
                <button
                  className="hx-btn hx-btn--primary"
                  disabled={busy || !draft.trim()}
                  onClick={() => save(k.service)}
                >
                  {k.has_key ? "Replace" : "Save"}
                </button>
              </div>
            </div>
          ) : confirmDelete === k.service ? (
            <div className="hx-row-actions">
              <span className="hx-confirm">Delete this key?</span>
              <button className="hx-btn" onClick={() => setConfirmDelete(null)}>
                No
              </button>
              <button className="hx-btn hx-btn--danger" disabled={busy} onClick={() => del(k.service)}>
                Delete
              </button>
            </div>
          ) : (
            <div className="hx-row-actions">
              {k.has_key && (
                <button className="hx-btn hx-btn--danger-ghost" onClick={() => setConfirmDelete(k.service)}>
                  Delete
                </button>
              )}
              <button
                className="hx-btn hx-btn--primary"
                onClick={() => {
                  setEditing(k.service);
                  setError(null);
                }}
              >
                {k.has_key ? "Replace" : "Add key"}
              </button>
            </div>
          )}
        </div>
      ))}
    </div>
  );
}

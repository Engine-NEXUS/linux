import { useCallback, useEffect, useMemo, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import {
  ageLabel,
  groupRows,
  keyLabel,
  sourceLabel,
  type EgressEntry,
  activitySummary,
  ageLabel as ageLabelFn,
  categoryLabel,
  fmtDays,
  fmtSlotTime,
  leaveBySummary,
  mailSummary,
  selftestSummary,
  whatsappSummary,
  type MailRow,
  type PersonRow,
  type WhatsappSelfTest,
  type MemoryFlag,
  type MemoryRow,
  type MemoryStatus,
  type TimetableSlot,
} from "./memoryModel";

/**
 * Memory page: what NEXUS remembers (with where each item came from), what
 * was sent to cloud models in the last 7 days, and the name-redaction switch.
 * All data lives on this PC; nothing here talks to the network.
 */
export function MemoryPage({ onFlagChanged }: { onFlagChanged?: (flag: MemoryFlag, on: boolean) => void }) {
  const [status, setStatus] = useState<MemoryStatus | null>(null);
  const [rows, setRows] = useState<MemoryRow[]>([]);
  const [egress, setEgress] = useState<EgressEntry[]>([]);
  const [slots, setSlots] = useState<TimetableSlot[]>([]);
  const [mail, setMail] = useState<MailRow[]>([]);
  const [people, setPeople] = useState<PersonRow[]>([]);
  const [selftest, setSelftest] = useState<WhatsappSelfTest | null>(null);
  const [hasRoutesKey, setHasRoutesKey] = useState(false);
  const [showEgress, setShowEgress] = useState(false);
  const [confirm, setConfirm] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);

  const load = useCallback(async () => {
    try {
      const [s, r, e, t, m, ppl, keys] = await Promise.all([
        invoke<MemoryStatus>("memcore_status"),
        invoke<MemoryRow[]>("memcore_list"),
        invoke<EgressEntry[]>("memcore_egress_log", { limit: 50 }),
        invoke<TimetableSlot[]>("memcore_timetable"),
        invoke<MailRow[]>("memcore_mail_list"),
        invoke<PersonRow[]>("memcore_people"),
        invoke<{ service: string; has_key: boolean }[]>("api_keys_list").catch(() => [] as { service: string; has_key: boolean }[]),
      ]);
      setStatus(s);
      setRows(r);
      setEgress(e);
      setSlots(t);
      setMail(m);
      setPeople(ppl);
      setHasRoutesKey(keys.some((k) => k.service === "google_maps" && k.has_key));
    } catch (e) {
      setError(String(e));
    }
  }, []);
  useEffect(() => {
    void load();
  }, [load]);

  const now = Math.floor(Date.now() / 1000);
  const groups = useMemo(() => groupRows(rows), [rows]);

  const forget = async (row: MemoryRow) => {
    setBusy(true);
    setError(null);
    try {
      await invoke("memory_forget", { key: row.key });
      setConfirm(null);
      await load();
    } catch (e) {
      setError(String(e));
    } finally {
      setBusy(false);
    }
  };

  // Same read-modify-write the rest of the hub uses: the form never holds
  // API keys, so saving blank keys neither wipes nor resurrects them.
  const setFlag = async (flag: MemoryFlag, on: boolean) => {
    setBusy(true);
    setError(null);
    try {
      const s = await invoke<Record<string, unknown>>("get_settings");
      await invoke("save_settings", {
        settings: { ...s, [flag]: on, groqApiKey: "", geminiApiKey: "", cerebrasApiKey: "" },
      });
      // Keep the hub's own settings form in step so its Save cannot revert this.
      onFlagChanged?.(flag, on);
      await load();
    } catch (e) {
      setError(String(e));
    } finally {
      setBusy(false);
    }
  };

  const muteSender = async (email: string) => {
    setBusy(true);
    setError(null);
    try {
      await invoke("memcore_mail_mute", { email });
      setConfirm(null);
      await load();
    } catch (e) {
      setError(String(e));
    } finally {
      setBusy(false);
    }
  };

  const flagPerson = async (jid: string, kind: "vip" | "mute", on: boolean) => {
    setBusy(true);
    setError(null);
    try {
      await invoke("memcore_person_flag", { jid, kind, on });
      await load();
    } catch (e) {
      setError(String(e));
    } finally {
      setBusy(false);
    }
  };

  const runSelftest = async () => {
    setBusy(true);
    setError(null);
    try {
      setSelftest(await invoke<WhatsappSelfTest>("whatsapp_selftest"));
    } catch (e) {
      setError(String(e));
    } finally {
      setBusy(false);
    }
  };

  const removeSlot = async (id: string) => {
    setBusy(true);
    setError(null);
    try {
      await invoke("memcore_timetable_delete", { id });
      setConfirm(null);
      await load();
    } catch (e) {
      setError(String(e));
    } finally {
      setBusy(false);
    }
  };

  const clearActivity = async () => {
    setBusy(true);
    setError(null);
    try {
      await invoke("memcore_clear_activity");
      await load();
    } catch (e) {
      setError(String(e));
    } finally {
      setBusy(false);
    }
  };

  const renderRow = (r: MemoryRow) => (
    <div className="hx-card hx-key" key={r.id}>
      <div className="hx-key-head">
        <div>
          <div className="hx-key-title">{keyLabel(r)}</div>
          <div className="hx-key-sub">
            {sourceLabel(r.source)} · {ageLabel(now, r.created)}
            {r.tier === "fact" && r.trust !== "user_said" ? ` · last used ${ageLabel(now, r.last_seen)}` : ""}
          </div>
        </div>
        {r.pinned && <span className="hx-pill hx-pill--on">Pinned</span>}
      </div>
      <div className="hx-key-mask">{r.value}</div>
      {confirm === `${r.id}` ? (
        <div className="hx-row-actions">
          <span className="hx-confirm">Forget this?</span>
          <button className="hx-btn" onClick={() => setConfirm(null)}>
            No
          </button>
          <button className="hx-btn hx-btn--danger" disabled={busy} onClick={() => forget(r)}>
            Forget
          </button>
        </div>
      ) : (
        <div className="hx-row-actions">
          <button className="hx-btn hx-btn--danger-ghost" onClick={() => setConfirm(`${r.id}`)}>
            Forget
          </button>
        </div>
      )}
    </div>
  );

  return (
    <div className="hx-page">
      <div className="hx-note">
        Everything below is stored only on this PC
        {status?.encrypted ? ", encrypted with a key held in Windows Credential Manager." : ", but NOT encrypted: the Windows key store could not hold the encryption key."}{" "}
        Say “forget everything” to erase it all.
      </div>
      {error && <div className="hx-error">{error}</div>}

      {status && !status.enabled && (
        <div className="hx-card hx-key">
          <div className="hx-key-title">Memory Core is off</div>
          <div className="hx-key-sub">Set memcore to true in settings.json to turn it on. Nothing is stored in the new store while it is off.</div>
        </div>
      )}

      {status && (
        <div className="hx-card hx-key">
          <div className="hx-key-head">
            <div>
              <div className="hx-key-title">Summary</div>
              <div className="hx-key-sub">
                {status.facts} facts · {status.episodes} conversations · {status.egress_7d} cloud sends in 7 days
              </div>
            </div>
            <span className={`hx-pill ${status.encrypted ? "hx-pill--on" : ""}`}>
              {status.encrypted ? "Encrypted" : "Plain file"}
            </span>
          </div>
          <label className="hx-row-actions" style={{ justifyContent: "space-between" }}>
            <span className="hx-key-sub">Hide people's names from cloud models (shown to them as “Person A”)</span>
            <input
              type="checkbox"
              checked={status.redact_names}
              disabled={busy}
              onChange={(e) => setFlag("memcoreRedactNames", e.target.checked)}
            />
          </label>
          <label className="hx-row-actions" style={{ justifyContent: "space-between" }}>
            <span className="hx-key-sub">Remember which window I was working in, so I can say where you left off</span>
            <input
              type="checkbox"
              checked={status.activity}
              disabled={busy}
              onChange={(e) => setFlag("memcoreActivity", e.target.checked)}
            />
          </label>
          <div className="hx-key-sub">{activitySummary(status)}</div>
          {status.resume_points > 0 && (
            <div className="hx-row-actions">
              <button className="hx-btn hx-btn--danger-ghost" disabled={busy} onClick={clearActivity}>
                Clear activity history
              </button>
            </div>
          )}
          <label className="hx-row-actions" style={{ justifyContent: "space-between" }}>
            <span className="hx-key-sub">Watch my inbox for important mail (exams, hackathons, GitHub, database notices) and read my calendar</span>
            <input
              type="checkbox"
              checked={status.mail}
              disabled={busy}
              onChange={(e) => setFlag("memcoreMail", e.target.checked)}
            />
          </label>
          <div className="hx-key-sub">{mailSummary(status)}</div>
          <label className="hx-row-actions" style={{ justifyContent: "space-between" }}>
            <span className="hx-key-sub">Tell me when a priority person messages me on WhatsApp (read-only; never marks chats read)</span>
            <input
              type="checkbox"
              checked={!!status.whatsapp}
              disabled={busy}
              onChange={(e) => setFlag("memcoreWhatsapp", e.target.checked)}
            />
          </label>
          <div className="hx-key-sub">{whatsappSummary(status)}</div>
          <label className="hx-row-actions" style={{ justifyContent: "space-between" }}>
            <span className="hx-key-sub">Speak message text aloud (the text goes to the cloud voice service; the sidebar card stays on this PC)</span>
            <input
              type="checkbox"
              checked={!!status.whatsapp_speak}
              disabled={busy}
              onChange={(e) => setFlag("memcoreWhatsappSpeak", e.target.checked)}
            />
          </label>
          <div className="hx-row-actions">
            <button className="hx-btn" disabled={busy} onClick={runSelftest}>
              Check WhatsApp connection
            </button>
          </div>
          {selftest && <div className="hx-key-sub">{selftestSummary(selftest)}</div>}
          <label className="hx-row-actions" style={{ justifyContent: "space-between" }}>
            <span className="hx-key-sub">Tell me when to leave for calendar events that have an address (uses live traffic)</span>
            <input
              type="checkbox"
              checked={status.leave_by !== false}
              disabled={busy}
              onChange={(e) => setFlag("memcoreLeaveBy", e.target.checked)}
            />
          </label>
          <div className="hx-key-sub">{leaveBySummary(status, hasRoutesKey)}</div>
          <label className="hx-row-actions" style={{ justifyContent: "space-between" }}>
            <span className="hx-key-sub">Remind me at my timetable times (“It's time for DSA. Shall I start?”)</span>
            <input
              type="checkbox"
              checked={status.timetable}
              disabled={busy}
              onChange={(e) => setFlag("memcoreTimetable", e.target.checked)}
            />
          </label>
          <label className="hx-row-actions" style={{ justifyContent: "space-between" }}>
            <span className="hx-key-sub">Say a short “where you left off” once a day, a little after startup</span>
            <input
              type="checkbox"
              checked={status.briefing}
              disabled={busy}
              onChange={(e) => setFlag("memcoreBriefing", e.target.checked)}
            />
          </label>
          <label className="hx-row-actions" style={{ justifyContent: "space-between" }}>
            <span className="hx-key-sub">
              Remember which apps, folders and browser windows are open, and offer to reopen them after a restart
            </span>
            <input
              type="checkbox"
              checked={status.session !== false}
              disabled={busy}
              onChange={(e) => setFlag("memcoreSession", e.target.checked)}
            />
          </label>
          <label className="hx-row-actions" style={{ justifyContent: "space-between" }}>
            <span className="hx-key-sub">Check my connections at startup and mention anything that is broken</span>
            <input
              type="checkbox"
              checked={status.health !== false}
              disabled={busy}
              onChange={(e) => setFlag("memcoreHealth", e.target.checked)}
            />
          </label>
        </div>
      )}

      <div className="hx-section">Important mail ({mail.length})</div>
      {mail.length === 0 && (
        <div className="hx-key-sub">
          Nothing filed. Only sender, subject and Gmail's own short preview are ever read — never the full message.
        </div>
      )}
      {mail.map((m) => (
        <div className="hx-card hx-key" key={m.key}>
          <div className="hx-key-head">
            <div>
              <div className="hx-key-title">{m.subject || "(no subject)"}</div>
              <div className="hx-key-sub">
                {m.sender} · {ageLabelFn(Math.floor(Date.now() / 1000), Math.floor(m.ts_ms / 1000))}
              </div>
            </div>
            <span className={`hx-pill ${m.urgency === "high" ? "hx-pill--on" : ""}`}>{categoryLabel(m.category)}</span>
          </div>
          <div className="hx-key-sub">Why: {m.reason}</div>
          {confirm === `mail:${m.key}` ? (
            <div className="hx-row-actions">
              <span className="hx-confirm">Stop alerts from {m.email}?</span>
              <button className="hx-btn" onClick={() => setConfirm(null)}>
                No
              </button>
              <button className="hx-btn hx-btn--danger" disabled={busy} onClick={() => muteSender(m.email)}>
                Mute
              </button>
            </div>
          ) : (
            <div className="hx-row-actions">
              <button className="hx-btn hx-btn--danger-ghost" onClick={() => setConfirm(`mail:${m.key}`)}>
                Not important
              </button>
            </div>
          )}
        </div>
      ))}

      <div className="hx-section">WhatsApp priority people ({people.length})</div>
      {people.length === 0 && (
        <div className="hx-key-sub">
          Nobody learned yet. NEXUS learns from how often you message each other and how fast you reply — never from what is said.
        </div>
      )}
      {people.slice(0, 15).map((p) => (
        <div className="hx-card hx-key" key={p.jid}>
          <div className="hx-key-head">
            <div>
              <div className="hx-key-title">{p.name}</div>
              <div className="hx-key-sub">
                Score {p.score} · {p.why}
              </div>
            </div>
            {p.vip && <span className="hx-pill hx-pill--on">VIP</span>}
            {p.muted && <span className="hx-pill">Muted</span>}
          </div>
          <div className="hx-row-actions">
            <button className="hx-btn" disabled={busy} onClick={() => flagPerson(p.jid, "vip", !p.vip)}>
              {p.vip ? "Remove VIP" : "Make VIP"}
            </button>
            <button className="hx-btn hx-btn--danger-ghost" disabled={busy} onClick={() => flagPerson(p.jid, "mute", !p.muted)}>
              {p.muted ? "Unmute" : "Mute"}
            </button>
          </div>
        </div>
      ))}

      <div className="hx-section">Timetable ({slots.length})</div>
      {slots.length === 0 && (
        <div className="hx-key-sub">
          Empty. Show me a timetable picture on screen and say “add this to my timetable”, or copy it and say “add the
          copied picture to my timetable”. You confirm before anything is saved.
        </div>
      )}
      {slots.map((s) => (
        <div className="hx-card hx-key" key={s.id}>
          <div className="hx-key-head">
            <div>
              <div className="hx-key-title">{s.title}</div>
              <div className="hx-key-sub">
                {fmtDays(s.days)} · {fmtSlotTime(s)}
                {s.section ? ` · ${s.section}` : ""}
              </div>
            </div>
          </div>
          {confirm === `slot:${s.id}` ? (
            <div className="hx-row-actions">
              <span className="hx-confirm">Remove this slot?</span>
              <button className="hx-btn" onClick={() => setConfirm(null)}>
                No
              </button>
              <button className="hx-btn hx-btn--danger" disabled={busy} onClick={() => removeSlot(s.id)}>
                Remove
              </button>
            </div>
          ) : (
            <div className="hx-row-actions">
              <button className="hx-btn hx-btn--danger-ghost" onClick={() => setConfirm(`slot:${s.id}`)}>
                Remove
              </button>
            </div>
          )}
        </div>
      ))}

      <div className="hx-section">You told me ({groups.told.length})</div>
      {groups.told.length === 0 && <div className="hx-key-sub">Nothing yet. Say “remember that my dog is Bruno”.</div>}
      {groups.told.map(renderRow)}

      <div className="hx-section">I picked up ({groups.learned.length})</div>
      {groups.learned.length === 0 && <div className="hx-key-sub">Nothing learned yet. Unused items fade away on their own.</div>}
      {groups.learned.map(renderRow)}

      <div className="hx-section">Recent conversations ({groups.episodes.length})</div>
      {groups.episodes.slice(0, 10).map(renderRow)}

      <div className="hx-section">Sent to cloud models (last 7 days)</div>
      <div className="hx-row-actions">
        <button className="hx-btn" onClick={() => setShowEgress((v) => !v)}>
          {showEgress ? "Hide" : `Show ${egress.length} sends`}
        </button>
      </div>
      {showEgress &&
        egress.map((e, i) => (
          <div className="hx-card hx-key" key={`${e.ts}-${i}`}>
            <div className="hx-key-sub">
              {new Date(e.ts * 1000).toLocaleString()} · {e.route}
            </div>
            <div className="hx-key-title">You said</div>
            <div className="hx-key-mask">{e.request}</div>
            <div className="hx-key-title">Memory sent with it</div>
            <div className="hx-key-mask" style={{ whiteSpace: "pre-wrap" }}>
              {e.memory || "(nothing)"}
            </div>
          </div>
        ))}
    </div>
  );
}

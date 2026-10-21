// Pure helpers for the Memory page (Command Hub). Mirrors the Rust
// `memcore::store::Row` / `EgressEntry` shapes returned by the IPC commands.

export interface MemoryRow {
  id: number;
  tier: string; // "fact" | "episode"
  key: string;
  value: string;
  source: string;
  trust: string; // "user_said" | "user_owned" | "derived" | "untrusted"
  created: number;
  last_seen: number;
  pinned: boolean;
  uses: number;
}

export interface EgressEntry {
  ts: number;
  route: string;
  request: string;
  memory: string;
}

export interface MemoryStatus {
  enabled: boolean;
  encrypted: boolean;
  facts: number;
  episodes: number;
  egress_7d: number;
  redact_names: boolean;
  /** Foreground-window recording (where-you-left-off) is on. */
  activity: boolean;
  /** Resume points currently stored. */
  resume_points: number;
  /** Once-a-day boot briefing is on. */
  briefing: boolean;
  /** Timetable reminders are on. */
  timetable: boolean;
  /** Saved timetable slots. */
  slots: number;
  /** Inbox watching + calendar are on. */
  mail: boolean;
  /** A Google account with mail access is connected. */
  google_connected: boolean;
  /** Important emails currently filed. */
  mail_items: number;
  /** Google rejected the stored sign-in (e.g. the 7-day Testing-mode expiry). */
  google_needs_signin?: boolean;
  /** WhatsApp priority watcher is switched on (default off). */
  whatsapp?: boolean;
  /** Message text may be spoken aloud (off: sidebar only). */
  whatsapp_speak?: boolean;
  /** Leave-by reminders are on. */
  leave_by?: boolean;
  /** Remember what is open and offer to reopen it after a restart. */
  session?: boolean;
  /** Mention broken connections in the startup greeting. */
  health?: boolean;
  /** Learned WhatsApp people. */
  people?: number;
}

/** A person NEXUS learned from WhatsApp patterns (counts and timing only). */
export interface PersonRow {
  jid: string;
  name: string;
  score: number;
  why: string;
  vip: boolean;
  muted: boolean;
}

/** Result of the WhatsApp bridge check (`whatsapp_selftest`). */
export interface WhatsappSelfTest {
  reachable: boolean;
  paired: boolean;
  tools: number;
  has_read_tools: boolean;
  guarded_tools_present: string[];
  chats_parsed: number;
  format_ok: boolean;
  note: string;
}

/** What leave-by reminders still need before they can speak. */
export function leaveBySummary(
  st: { leave_by?: boolean; google_connected?: boolean },
  hasRoutesKey: boolean,
): string {
  if (st.leave_by === false) return "Off - NEXUS will not tell you when to leave.";
  const missing: string[] = [];
  if (!st.google_connected) missing.push("a connected Google account (calendar)");
  if (!hasRoutesKey) missing.push("a Google Maps & Routes API key (API Keys)");
  missing.push('your home address (say "remember that my home is ...")');
  return `On. Needs: ${missing.join(", ")}. Trips are measured from home, only for events with a street address.`;
}

/** One-line status of the WhatsApp watcher for the Memory page. */
export function whatsappSummary(st: Pick<MemoryStatus, "whatsapp" | "people">): string {
  if (!st.whatsapp) {
    return "Off — NEXUS is not reading WhatsApp. Turn it on only after the two-phone read-receipt check (docs/testing/whatsapp-read-receipt-test.md).";
  }
  const n = st.people ?? 0;
  if (n === 0) return "On — still learning who matters to you. Nothing is announced for a chat until NEXUS has seen it once.";
  return `On — ${n} ${n === 1 ? "person" : "people"} learned from how you message. Message text is never used to decide who matters.`;
}

/** Plain-language result of the bridge check. A pass does NOT prove "no read receipts". */
export function selftestSummary(t: WhatsappSelfTest): string {
  if (!t.reachable || !t.paired) return t.note;
  if (!t.has_read_tools) return "The bridge is paired but does not offer the chat-reading tools NEXUS needs.";
  if (!t.format_ok) return `Connected, but NEXUS could not understand the chat list: ${t.note}`;
  const g = t.guarded_tools_present.length;
  return `Bridge OK — ${t.chats_parsed} chats understood. It also offers ${g} receipt/presence tool${g === 1 ? "" : "s"}; NEXUS refuses to call ${g === 1 ? "it" : "them"}. This does not prove that reading is invisible - do the two-phone check before relying on it.`;
}

/** An important email the inbox watcher filed (sender/subject/Gmail snippet only). */
export interface MailRow {
  key: string;
  sender: string;
  email: string;
  subject: string;
  snippet: string;
  category: string;
  urgency: string;
  reason: string;
  ts_ms: number;
  account: string;
}

const CATEGORY_LABELS: Record<string, string> = {
  exam: "Exam",
  hackathon: "Hackathon",
  github: "GitHub",
  database: "Database",
  deadline: "Deadline",
  internship: "Internship",
};

export function categoryLabel(c: string): string {
  return CATEGORY_LABELS[c] ?? c;
}

/** One-line status of the inbox watcher for the Memory page. */
export function mailSummary(
  st: Pick<MemoryStatus, "mail" | "google_connected" | "mail_items"> & { google_needs_signin?: boolean },
): string {
  if (!st.mail) return "Off — NEXUS is not watching your inbox or calendar.";
  if (st.google_needs_signin) {
    return "Paused — Google needs you to sign in again (Google expires sign-ins after 7 days while your OAuth app is in Testing). Click Add Google Account.";
  }
  if (!st.google_connected) {
    return "On, but no Google account is connected. Add your own Google OAuth Client ID and Secret under Command Hub → Advanced, then click Add Google Account.";
  }
  if (st.mail_items === 0) return "On — nothing important right now.";
  return `On — ${st.mail_items} important email${st.mail_items === 1 ? "" : "s"} filed for 7 days.`;
}

/** A timetable slot as stored by the Memory Core (days: 0 = Monday … 6 = Sunday). */
export interface TimetableSlot {
  id: string;
  title: string;
  days: number[];
  start_min: number;
  end_min?: number | null;
  section?: string | null;
}

/** Settings keys the Memory page can switch (camelCase, as in settings.json). */
export type MemoryFlag =
  | "memcoreRedactNames"
  | "memcoreActivity"
  | "memcoreBriefing"
  | "memcoreTimetable"
  | "memcoreMail"
  | "memcoreWhatsapp"
  | "memcoreWhatsappSpeak"
  | "memcoreLeaveBy"
  | "memcoreSession"
  | "memcoreHealth";

const DAY_NAMES = ["Mon", "Tue", "Wed", "Thu", "Fri", "Sat", "Sun"];

/** 1080 → "6 PM", 1110 → "6:30 PM". */
export function fmtTime(min: number): string {
  const h = Math.floor(min / 60) % 24;
  const m = min % 60;
  const ap = h < 12 ? "AM" : "PM";
  const h12 = h % 12 === 0 ? 12 : h % 12;
  return m === 0 ? `${h12} ${ap}` : `${h12}:${String(m).padStart(2, "0")} ${ap}`;
}

/** [0,1,2,3,4] → "weekdays", [0,2] → "Mon, Wed". */
export function fmtDays(days: number[]): string {
  const d = [...days].sort((a, b) => a - b);
  if (d.length === 7) return "every day";
  if (d.join() === "0,1,2,3,4") return "weekdays";
  if (d.join() === "5,6") return "weekends";
  return d.map((x) => DAY_NAMES[Math.min(Math.max(x, 0), 6)]).join(", ");
}

/** "6 PM – 7:30 PM" or "6 PM". */
export function fmtSlotTime(s: Pick<TimetableSlot, "start_min" | "end_min">): string {
  return s.end_min ? `${fmtTime(s.start_min)} – ${fmtTime(s.end_min)}` : fmtTime(s.start_min);
}

/** One-line description of the activity recorder's state for the Memory page. */
export function activitySummary(st: Pick<MemoryStatus, "activity" | "resume_points">): string {
  if (!st.activity) return "Off — NEXUS is not recording which window you work in.";
  if (st.resume_points === 0) return "On — nothing recorded yet.";
  return `On — ${st.resume_points} point${st.resume_points === 1 ? "" : "s"} kept for 7 days, only on this PC.`;
}

export function ageLabel(nowSec: number, ts: number): string {
  const days = Math.max(0, Math.floor((nowSec - ts) / 86400));
  if (days === 0) return "today";
  if (days === 1) return "yesterday";
  return `${days}d ago`;
}

export interface MemoryGroups {
  told: MemoryRow[];
  learned: MemoryRow[];
  episodes: MemoryRow[];
}

/** Facts you told NEXUS, facts it picked up on its own, and conversation lines. */
export function groupRows(rows: MemoryRow[]): MemoryGroups {
  return {
    told: rows.filter((r) => r.tier === "fact" && r.trust === "user_said"),
    learned: rows.filter((r) => r.tier === "fact" && r.trust !== "user_said"),
    episodes: rows.filter((r) => r.tier === "episode"),
  };
}

/** Human label for a record's origin ("miner:episode" → "picked up from a conversation"). */
export function sourceLabel(source: string): string {
  if (source.startsWith("user:")) return "you told me";
  if (source.startsWith("miner:")) return "picked up from a conversation";
  if (source.startsWith("legacy:")) return "imported from earlier memory";
  if (source === "turn") return "conversation";
  return source;
}

/** Display name for a stored key ("dog_name" → "dog name"; episode keys are hidden). */
export function keyLabel(row: MemoryRow): string {
  return row.tier === "episode" ? "conversation" : row.key.replace(/_/g, " ");
}

import { describe, expect, it } from "vitest";
import { leaveBySummary, activitySummary, ageLabel, categoryLabel, fmtDays, fmtSlotTime, fmtTime, groupRows, keyLabel, mailSummary, selftestSummary, sourceLabel, whatsappSummary, type MemoryRow, type WhatsappSelfTest } from "./memoryModel";

const row = (over: Partial<MemoryRow>): MemoryRow => ({
  id: 1,
  tier: "fact",
  key: "dog_name",
  value: "Bruno",
  source: "user:remember",
  trust: "user_said",
  created: 0,
  last_seen: 0,
  pinned: true,
  uses: 0,
  ...over,
});

describe("memoryModel", () => {
  it("lists what leave-by reminders still need", () => {
    expect(leaveBySummary({ leave_by: false, google_connected: true }, true)).toMatch(/^Off/);
    const all = leaveBySummary({ leave_by: true, google_connected: false }, false);
    expect(all).toContain("Google account");
    expect(all).toContain("Routes API key");
    expect(all).toContain("home address");
    const some = leaveBySummary({ leave_by: true, google_connected: true }, true);
    expect(some).not.toContain("Google account");
    expect(some).not.toContain("Routes API key");
    expect(some).toContain("from home");
  });

  it("summarises the WhatsApp watcher honestly", () => {
    expect(whatsappSummary({ whatsapp: false, people: 0 })).toMatch(/^Off.*two-phone/);
    expect(whatsappSummary({ whatsapp: undefined, people: undefined })).toMatch(/^Off/);
    expect(whatsappSummary({ whatsapp: true, people: 0 })).toContain("still learning");
    expect(whatsappSummary({ whatsapp: true, people: 1 })).toContain("1 person learned");
    expect(whatsappSummary({ whatsapp: true, people: 7 })).toContain("7 people learned");
  });

  it("never claims a passing self-test proves reads are invisible", () => {
    const base: WhatsappSelfTest = { reachable: true, paired: true, tools: 5, has_read_tools: true, guarded_tools_present: ["mark_read", "send_presence"], chats_parsed: 12, format_ok: true, note: "ok" };
    expect(selftestSummary({ ...base, reachable: false, paired: false, note: "The WhatsApp bridge is not running on 127.0.0.1:8765." })).toContain("not running");
    expect(selftestSummary({ ...base, paired: false, note: "not paired" })).toBe("not paired");
    expect(selftestSummary({ ...base, has_read_tools: false })).toContain("does not offer");
    expect(selftestSummary({ ...base, format_ok: false, note: "bad shape" })).toContain("could not understand the chat list: bad shape");
    const ok = selftestSummary(base);
    expect(ok).toContain("12 chats understood");
    expect(ok).toContain("refuses to call them");
    expect(ok).toContain("does not prove");
  });

  it("labels ages", () => {
    const now = 10 * 86400;
    expect(ageLabel(now, now)).toBe("today");
    expect(ageLabel(now, now - 86400)).toBe("yesterday");
    expect(ageLabel(now, now - 3 * 86400)).toBe("3d ago");
    expect(ageLabel(now, now + 500)).toBe("today"); // clock skew never goes negative
  });

  it("groups facts you told, facts it learned, and conversation lines", () => {
    const g = groupRows([
      row({ id: 1 }),
      row({ id: 2, key: "employer", trust: "derived", source: "miner:episode", pinned: false }),
      row({ id: 3, tier: "episode", key: "ep:1:0", trust: "derived", source: "turn" }),
    ]);
    expect(g.told.map((r) => r.id)).toEqual([1]);
    expect(g.learned.map((r) => r.id)).toEqual([2]);
    expect(g.episodes.map((r) => r.id)).toEqual([3]);
  });

  it("explains where a record came from", () => {
    expect(sourceLabel("user:remember")).toBe("you told me");
    expect(sourceLabel("miner:episode")).toBe("picked up from a conversation");
    expect(sourceLabel("legacy:core")).toBe("imported from earlier memory");
    expect(sourceLabel("turn")).toBe("conversation");
    expect(sourceLabel("mail:gmail")).toBe("mail:gmail");
  });

  it("hides internal episode keys", () => {
    expect(keyLabel(row({}))).toBe("dog name");
    expect(keyLabel(row({ tier: "episode", key: "ep:1700000000:3" }))).toBe("conversation");
  });

  it("formats timetable times and days like the voice does", () => {
    expect(fmtTime(0)).toBe("12 AM");
    expect(fmtTime(720)).toBe("12 PM");
    expect(fmtTime(1080)).toBe("6 PM");
    expect(fmtTime(1110)).toBe("6:30 PM");
    expect(fmtSlotTime({ start_min: 1080, end_min: 1170 })).toBe("6 PM – 7:30 PM");
    expect(fmtSlotTime({ start_min: 390, end_min: null })).toBe("6:30 AM");
    expect(fmtDays([4, 0, 3, 2, 1])).toBe("weekdays");
    expect(fmtDays([5, 6])).toBe("weekends");
    expect(fmtDays([2, 0])).toBe("Mon, Wed");
    expect(fmtDays([0, 1, 2, 3, 4, 5, 6])).toBe("every day");
  });

  it("describes the inbox watcher honestly", () => {
    expect(mailSummary({ mail: false, google_connected: true, mail_items: 3 })).toMatch(/^Off/);
    expect(mailSummary({ mail: true, google_connected: false, mail_items: 0 })).toContain("no Google account");
    expect(mailSummary({ mail: true, google_connected: true, mail_items: 0 })).toBe("On — nothing important right now.");
    expect(mailSummary({ mail: true, google_connected: true, mail_items: 1 })).toContain("1 important email filed");
    expect(mailSummary({ mail: true, google_connected: true, mail_items: 4 })).toContain("4 important emails");
    expect(mailSummary({ mail: true, google_connected: true, mail_items: 2, google_needs_signin: true })).toMatch(/^Paused.*sign in again/);
    expect(categoryLabel("database")).toBe("Database");
    expect(categoryLabel("internship")).toBe("Internship");
    expect(categoryLabel("other")).toBe("other");
  });

  it("describes the activity recorder honestly", () => {
    expect(activitySummary({ activity: false, resume_points: 40 })).toMatch(/^Off/);
    expect(activitySummary({ activity: true, resume_points: 0 })).toBe("On — nothing recorded yet.");
    expect(activitySummary({ activity: true, resume_points: 1 })).toContain("1 point kept");
    expect(activitySummary({ activity: true, resume_points: 12 })).toContain("12 points kept");
  });
});

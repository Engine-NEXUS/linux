import { beforeEach, describe, expect, it, vi } from "vitest";

vi.mock("@tauri-apps/api/core", () => ({
  invoke: vi.fn(),
}));

vi.mock("@tauri-apps/api/event", () => ({
  listen: vi.fn(async () => () => {}),
}));

import {
  initSentinelAlertListener,
  useSentinel,
  type SentinelAlertPayload,
} from "./sentinel";

function payload(over: Partial<SentinelAlertPayload> = {}): SentinelAlertPayload {
  return {
    alert_id: "sentinel_deadline_1790701281",
    source: "Gmail",
    account_email: "official.lakshya.chitkul@gmail.com",
    organization: {
      name: "Acme",
      domain: "acme.edu",
      avatar_fallback_initials: "AC",
      brand_color: "#4285F4",
    },
    context: {
      thread_id: "screen_thread_1790701281",
      subject: "Final Submission Guidelines & Deadline Extension",
      sender: "Dr. Henderson <henderson@acme.edu>",
      snippet: "The submission portal will remain open until Monday 9:00 AM.",
    },
    deadline: {
      is_extended: true,
      previous_deadline: "Friday 5:00 PM",
      new_deadline: "Monday 9:00 AM",
      urgency: "High",
    },
    landing_animation: {
      initial_state: "incoming_pulse",
      docked_state: "side_pill_pill",
      auto_collapse_after_ms: 7000,
    },
    spoken_notification: "Sir, update on Final Submission Guidelines: the deadline has been updated to Monday 9:00 AM.",
    ...over,
  };
}

describe("sentinel tracking store (Orb landing spec, tracking-first)", () => {
  beforeEach(() => {
    useSentinel.getState().clearAlerts();
  });

  it("tracks an incoming alert with the spec wire shape intact", () => {
    useSentinel.getState().pushAlert(payload());
    const all = useSentinel.getState().alerts;
    expect(all).toHaveLength(1);
    expect(all[0].status).toBe("incoming");
    expect(all[0].context.thread_id).toBe("screen_thread_1790701281");
    expect(all[0].deadline?.new_deadline).toBe("Monday 9:00 AM");
    expect(all[0].landing_animation.auto_collapse_after_ms).toBe(7000);
    expect(typeof all[0].receivedAt).toBe("number");
  });

  it("dedupes by alert_id (poll loop may re-emit)", () => {
    useSentinel.getState().pushAlert(payload());
    useSentinel.getState().pushAlert(payload());
    expect(useSentinel.getState().alerts).toHaveLength(1);
  });

  it("drives the status lifecycle without touching payload data", () => {
    useSentinel.getState().pushAlert(payload());
    useSentinel.getState().setStatus("sentinel_deadline_1790701281", "docked");
    useSentinel.getState().setStatus("sentinel_deadline_1790701281", "collapsed");
    const a = useSentinel.getState().alerts[0];
    expect(a.status).toBe("collapsed");
    expect(a.spoken_notification).toContain("Monday 9:00 AM");
    useSentinel.getState().dismissAlert("sentinel_deadline_1790701281");
    expect(useSentinel.getState().alerts[0].status).toBe("dismissed");
    useSentinel.getState().clearAlerts();
    expect(useSentinel.getState().alerts).toHaveLength(0);
  });

  it("registers the orchestrator:sentinel-alert listener", async () => {
    const { listen } = await import("@tauri-apps/api/event");
    const unlisten = await initSentinelAlertListener();
    expect(listen).toHaveBeenCalledWith("orchestrator:sentinel-alert", expect.any(Function));
    expect(typeof unlisten).toBe("function");
  });
});

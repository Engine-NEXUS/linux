import { create } from "zustand";

/**
 * Sentinel alert tracking store (tracking-first contract for the Orb UI &
 * Landing Specification). The Rust sentinel emits
 * `orchestrator:sentinel-alert` per detected change; this store is the
 * observation deck: every alert lands here with a lifecycle status so
 * tracking/debugging (and later the landing animation) can follow it.
 *
 * Lifecycle: incoming → docked → collapsed | dismissed. The landing UI
 * owns transitions; the listener only ever pushes `incoming`.
 * Spoken notifications stay Rust-side (`speak_proactive_alert`) — this
 * module never speaks (no double TTS).
 */

export interface SentinelOrganization {
  name: string;
  domain: string;
  avatar_fallback_initials: string;
  brand_color: string;
}

export interface SentinelContext {
  thread_id: string;
  subject: string;
  sender: string;
  snippet: string;
}

export interface SentinelDeadline {
  is_extended: boolean;
  previous_deadline: string;
  new_deadline: string;
  urgency: string;
}

export interface SentinelLanding {
  initial_state: string;
  docked_state: string;
  auto_collapse_after_ms: number;
}

/** Exact mirror of the Rust `SentinelAlertPayload` wire shape. */
export interface SentinelAlertPayload {
  alert_id: string;
  source: string;
  account_email?: string | null;
  organization: SentinelOrganization;
  context: SentinelContext;
  deadline?: SentinelDeadline | null;
  landing_animation: SentinelLanding;
  spoken_notification: string;
}

export type SentinelAlertStatus = "incoming" | "docked" | "collapsed" | "dismissed";

export interface TrackedSentinelAlert extends SentinelAlertPayload {
  status: SentinelAlertStatus;
  receivedAt: number;
}

interface SentinelState {
  alerts: TrackedSentinelAlert[];
  pushAlert: (payload: SentinelAlertPayload) => void;
  setStatus: (alertId: string, status: SentinelAlertStatus) => void;
  dismissAlert: (alertId: string) => void;
  clearAlerts: () => void;
}

export const useSentinel = create<SentinelState>()((set) => ({
  alerts: [],
  pushAlert: (payload) =>
    set((st) => {
      if (st.alerts.some((a) => a.alert_id === payload.alert_id)) return st;
      const tracked: TrackedSentinelAlert = {
        ...payload,
        status: "incoming",
        receivedAt: Date.now(),
      };
      return { alerts: [...st.alerts, tracked] };
    }),
  setStatus: (alertId, status) =>
    set((st) => ({
      alerts: st.alerts.map((a) => (a.alert_id === alertId ? { ...a, status } : a)),
    })),
  dismissAlert: (alertId) =>
    set((st) => ({
      alerts: st.alerts.map((a) =>
        a.alert_id === alertId ? { ...a, status: "dismissed" } : a,
      ),
    })),
  clearAlerts: () => set({ alerts: [] }),
}));

/**
 * Register the `orchestrator:sentinel-alert` listener (call once at
 * startup). Pure tracking: appends to the store + console trace. Never
 * speaks, never touches the orb — the landing phase owns presentation.
 */
export async function initSentinelAlertListener(): Promise<() => void> {
  try {
    const { listen } = await import("@tauri-apps/api/event");
    return await listen<SentinelAlertPayload>("orchestrator:sentinel-alert", (event) => {
      console.log("[NEXUS] sentinel-alert tracked:", event.payload.alert_id, event.payload.source);
      useSentinel.getState().pushAlert(event.payload);
    });
  } catch {
    return () => {};
  }
}

#![allow(dead_code)]

use super::types::{CalendarEvent, EventReceipt, GoogleError, NewEvent};
use chrono::DateTime;
use serde_json::Value;

/// Google Calendar domain service.
pub struct CalendarService;

impl CalendarService {
    /// Parse Google Calendar API events list response into structured `CalendarEvent` items.
    pub fn parse_events_json(json: &Value) -> Result<Vec<CalendarEvent>, GoogleError> {
        let items = json
            .get("items")
            .and_then(|v| v.as_array())
            .ok_or_else(|| GoogleError::SerializationError("Missing items array in calendar response".into()))?;

        let mut events = Vec::new();

        for item in items {
            let id = item.get("id").and_then(|v| v.as_str()).unwrap_or_default().to_string();
            let summary = item.get("summary").and_then(|v| v.as_str()).unwrap_or("(No title)").to_string();
            let description = item.get("description").and_then(|v| v.as_str()).map(|s| s.to_string());
            let location = item.get("location").and_then(|v| v.as_str()).map(|s| s.to_string());
            let status = item.get("status").and_then(|v| v.as_str()).unwrap_or("confirmed").to_string();

            // Start time can be dateTime or date (all-day event)
            let start_iso = item
                .pointer("/start/dateTime")
                .and_then(|v| v.as_str())
                .or_else(|| item.pointer("/start/date").and_then(|v| v.as_str()))
                .unwrap_or_default()
                .to_string();

            // End time can be dateTime or date
            let end_iso = item
                .pointer("/end/dateTime")
                .and_then(|v| v.as_str())
                .or_else(|| item.pointer("/end/date").and_then(|v| v.as_str()))
                .unwrap_or_default()
                .to_string();

            events.push(CalendarEvent {
                id,
                summary,
                description,
                start_iso,
                end_iso,
                location,
                status,
            });
        }

        Ok(events)
    }

    /// Build JSON payload for Google Calendar event insertion.
    pub fn create_event_payload(event: &NewEvent) -> Value {
        let mut payload = serde_json::json!({
            "summary": event.summary,
            "start": {
                "dateTime": event.start_iso
            },
            "end": {
                "dateTime": event.end_iso
            }
        });

        if let Some(ref desc) = event.description {
            payload["description"] = Value::String(desc.clone());
        }

        if let Some(ref loc) = event.location {
            payload["location"] = Value::String(loc.clone());
        }

        payload
    }

    /// Parse single event receipt after creation.
    pub fn parse_event_receipt_json(json: &Value) -> Result<EventReceipt, GoogleError> {
        let event_id = json.get("id").and_then(|v| v.as_str()).unwrap_or_default().to_string();
        let summary = json.get("summary").and_then(|v| v.as_str()).unwrap_or_default().to_string();
        let start_iso = json
            .pointer("/start/dateTime")
            .and_then(|v| v.as_str())
            .or_else(|| json.pointer("/start/date").and_then(|v| v.as_str()))
            .unwrap_or_default()
            .to_string();

        Ok(EventReceipt {
            event_id,
            summary,
            start_iso,
        })
    }

    /// Parses an ISO 8601 string into Unix epoch timestamp in seconds.
    pub fn parse_iso_to_epoch(iso: &str) -> Option<i64> {
        if let Ok(dt) = DateTime::parse_from_rfc3339(iso) {
            return Some(dt.timestamp());
        }
        // Handle date-only YYYY-MM-DD
        if let Ok(ndt) = chrono::NaiveDate::parse_from_str(iso, "%Y-%m-%d") {
            return Some(ndt.and_hms_opt(0, 0, 0)?.and_utc().timestamp());
        }
        None
    }

    /// Checks for schedule conflicts within a proposed time range.
    /// Returns any events that overlap with [start_sec, end_sec].
    pub fn check_schedule_conflict<'a>(
        events: &'a [CalendarEvent],
        start_sec: i64,
        end_sec: i64,
    ) -> Vec<&'a CalendarEvent> {
        events
            .iter()
            .filter(|ev| {
                if ev.status == "cancelled" {
                    return false;
                }
                let ev_start = match Self::parse_iso_to_epoch(&ev.start_iso) {
                    Some(s) => s,
                    None => return false,
                };
                let ev_end = match Self::parse_iso_to_epoch(&ev.end_iso) {
                    Some(e) => e,
                    None => return false,
                };

                // Overlap condition: max(start_sec, ev_start) < min(end_sec, ev_end)
                start_sec < ev_end && end_sec > ev_start
            })
            .collect()
    }

    /// Computes available free time slots of at least `slot_duration_sec`
    /// between `window_start_sec` and `window_end_sec`.
    pub fn calculate_free_busy_buffer(
        events: &[CalendarEvent],
        window_start_sec: i64,
        window_end_sec: i64,
        slot_duration_sec: i64,
    ) -> Vec<(i64, i64)> {
        let mut busy_ranges: Vec<(i64, i64)> = events
            .iter()
            .filter(|e| e.status != "cancelled")
            .filter_map(|e| {
                let s = Self::parse_iso_to_epoch(&e.start_iso)?;
                let end = Self::parse_iso_to_epoch(&e.end_iso)?;
                let clamped_start = s.max(window_start_sec);
                let clamped_end = end.min(window_end_sec);
                if clamped_start < clamped_end {
                    Some((clamped_start, clamped_end))
                } else {
                    None
                }
            })
            .collect();

        busy_ranges.sort_by_key(|&(s, _)| s);

        // Merge overlapping busy ranges
        let mut merged_busy: Vec<(i64, i64)> = Vec::new();
        for (s, e) in busy_ranges {
            if let Some(last) = merged_busy.last_mut() {
                if s <= last.1 {
                    last.1 = last.1.max(e);
                    continue;
                }
            }
            merged_busy.push((s, e));
        }

        // Find gaps
        let mut free_slots = Vec::new();
        let mut current_cursor = window_start_sec;

        for (b_start, b_end) in merged_busy {
            if b_start - current_cursor >= slot_duration_sec {
                free_slots.push((current_cursor, b_start));
            }
            current_cursor = current_cursor.max(b_end);
        }

        if window_end_sec - current_cursor >= slot_duration_sec {
            free_slots.push((current_cursor, window_end_sec));
        }

        free_slots
    }
}

// ─── Unit Tests ──────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_events_json() {
        let raw = serde_json::json!({
            "items": [
                {
                    "id": "event_1",
                    "summary": "Team Sync",
                    "description": "Weekly engineering check-in",
                    "start": { "dateTime": "2026-09-30T10:00:00Z" },
                    "end": { "dateTime": "2026-09-30T10:30:00Z" },
                    "location": "Meet room 4",
                    "status": "confirmed"
                },
                {
                    "id": "event_2",
                    "summary": "Doctor Appointment",
                    "start": { "date": "2026-10-01" },
                    "end": { "date": "2026-10-02" },
                    "status": "confirmed"
                }
            ]
        });

        let events = CalendarService::parse_events_json(&raw).unwrap();
        assert_eq!(events.len(), 2);
        assert_eq!(events[0].summary, "Team Sync");
        assert_eq!(events[0].start_iso, "2026-09-30T10:00:00Z");
        assert_eq!(events[1].start_iso, "2026-10-01");
    }

    #[test]
    fn test_create_event_payload() {
        let new_ev = NewEvent {
            summary: "Sprint Planning".to_string(),
            description: Some("Q4 Roadmap kickoff".to_string()),
            start_iso: "2026-09-30T14:00:00Z".to_string(),
            end_iso: "2026-09-30T15:00:00Z".to_string(),
            location: Some("Conference Room A".to_string()),
        };

        let payload = CalendarService::create_event_payload(&new_ev);
        assert_eq!(payload["summary"], "Sprint Planning");
        assert_eq!(payload["start"]["dateTime"], "2026-09-30T14:00:00Z");
        assert_eq!(payload["end"]["dateTime"], "2026-09-30T15:00:00Z");
        assert_eq!(payload["description"], "Q4 Roadmap kickoff");
        assert_eq!(payload["location"], "Conference Room A");
    }

    #[test]
    fn test_parse_event_receipt_json() {
        let raw = serde_json::json!({
            "id": "ev_abc123",
            "summary": "1:1 with Manager",
            "start": { "dateTime": "2026-09-30T16:00:00Z" }
        });
        let receipt = CalendarService::parse_event_receipt_json(&raw).unwrap();
        assert_eq!(receipt.event_id, "ev_abc123");
        assert_eq!(receipt.summary, "1:1 with Manager");
        assert_eq!(receipt.start_iso, "2026-09-30T16:00:00Z");
    }

    #[test]
    fn test_schedule_conflict_detection() {
        let events = vec![
            CalendarEvent {
                id: "1".into(),
                summary: "Sync".into(),
                description: None,
                start_iso: "2026-09-30T10:00:00Z".into(), // 1790762400
                end_iso: "2026-09-30T11:00:00Z".into(),   // 1790766000
                location: None,
                status: "confirmed".into(),
            },
            CalendarEvent {
                id: "2".into(),
                summary: "Lunch".into(),
                description: None,
                start_iso: "2026-09-30T12:00:00Z".into(),
                end_iso: "2026-09-30T13:00:00Z".into(),
                location: None,
                status: "cancelled".into(), // Should be ignored
            },
        ];

        let start_epoch = CalendarService::parse_iso_to_epoch("2026-09-30T10:30:00Z").unwrap();
        let end_epoch = CalendarService::parse_iso_to_epoch("2026-09-30T11:30:00Z").unwrap();

        let conflicts = CalendarService::check_schedule_conflict(&events, start_epoch, end_epoch);
        assert_eq!(conflicts.len(), 1);
        assert_eq!(conflicts[0].id, "1");

        // No conflict test
        let clear_start = CalendarService::parse_iso_to_epoch("2026-09-30T11:00:00Z").unwrap();
        let clear_end = CalendarService::parse_iso_to_epoch("2026-09-30T12:00:00Z").unwrap();
        let clear_conflicts = CalendarService::check_schedule_conflict(&events, clear_start, clear_end);
        assert!(clear_conflicts.is_empty());
    }

    #[test]
    fn test_free_busy_buffer_calculation() {
        let events = vec![
            CalendarEvent {
                id: "1".into(),
                summary: "Meeting A".into(),
                description: None,
                start_iso: "2026-09-30T09:30:00Z".into(),
                end_iso: "2026-09-30T10:30:00Z".into(),
                location: None,
                status: "confirmed".into(),
            },
            CalendarEvent {
                id: "2".into(),
                summary: "Meeting B".into(),
                description: None,
                start_iso: "2026-09-30T11:00:00Z".into(),
                end_iso: "2026-09-30T12:00:00Z".into(),
                location: None,
                status: "confirmed".into(),
            },
        ];

        let window_start = CalendarService::parse_iso_to_epoch("2026-09-30T09:00:00Z").unwrap();
        let window_end = CalendarService::parse_iso_to_epoch("2026-09-30T13:00:00Z").unwrap();
        let slot_30m = 1800; // 30 minutes

        let slots = CalendarService::calculate_free_busy_buffer(&events, window_start, window_end, slot_30m);
        // Expect:
        // [09:00, 09:30] -> 30 min (free)
        // [10:30, 11:00] -> 30 min (free)
        // [12:00, 13:00] -> 60 min (free)
        assert_eq!(slots.len(), 3);
        assert_eq!(slots[0].1 - slots[0].0, 1800);
        assert_eq!(slots[1].1 - slots[1].0, 1800);
        assert_eq!(slots[2].1 - slots[2].0, 3600);
    }
}

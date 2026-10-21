#![allow(dead_code)]

use super::types::{CommuteDelay, GoogleError, PlaceItem, RouteSummary, TravelMode};
use chrono::{DateTime, Utc};
use serde_json::Value;

/// Google Maps domain service.
pub struct MapsService;

impl MapsService {
    /// Parse Directions API JSON response into `RouteSummary`.
    pub fn parse_directions_json(
        json: &Value,
        origin: &str,
        destination: &str,
    ) -> Result<RouteSummary, GoogleError> {
        let route = json
            .pointer("/routes/0")
            .ok_or_else(|| GoogleError::NotFound("No route found for specified points".into()))?;

        let leg = route
            .pointer("/legs/0")
            .ok_or_else(|| GoogleError::SerializationError("Malformed route leg structure".into()))?;

        let distance_text = leg
            .pointer("/distance/text")
            .and_then(|v| v.as_str())
            .unwrap_or("Unknown distance")
            .to_string();

        let duration_text = leg
            .pointer("/duration/text")
            .and_then(|v| v.as_str())
            .unwrap_or("Unknown duration")
            .to_string();

        let duration_seconds = leg
            .pointer("/duration/value")
            .and_then(|v| v.as_u64())
            .unwrap_or(0) as u32;

        let duration_in_traffic_sec = leg
            .pointer("/duration_in_traffic/value")
            .and_then(|v| v.as_u64())
            .map(|v| v as u32);

        let traffic_delay_minutes = if let Some(traffic_sec) = duration_in_traffic_sec {
            if traffic_sec > duration_seconds {
                (traffic_sec - duration_seconds) / 60
            } else {
                0
            }
        } else {
            0
        };

        let mut summary_steps = Vec::new();
        if let Some(steps) = leg.get("steps").and_then(|s| s.as_array()) {
            for step in steps.iter().take(5) {
                if let Some(instruction) = step.get("html_instructions").and_then(|v| v.as_str()) {
                    let stripped = instruction
                        .replace("<b>", "")
                        .replace("</b>", "")
                        .replace("<div style=\"font-size:0.9em\">", " (")
                        .replace("</div>", ")");
                    summary_steps.push(stripped);
                }
            }
        }

        Ok(RouteSummary {
            origin: origin.to_string(),
            destination: destination.to_string(),
            distance_text,
            duration_text,
            duration_seconds,
            traffic_delay_minutes,
            summary_steps,
        })
    }

    /// Detect commute delays and compute suggested departure time based on an arrival deadline.
    pub fn detect_commute_delay(
        normal_duration_min: u32,
        current_duration_min: u32,
        destination: &str,
        arrival_deadline_epoch: i64,
    ) -> CommuteDelay {
        let delay_minutes = if current_duration_min > normal_duration_min {
            current_duration_min - normal_duration_min
        } else {
            0
        };

        let is_congested = delay_minutes >= 10 || (current_duration_min as f32 > normal_duration_min as f32 * 1.25);

        // Required departure = arrival_deadline - current_duration_min (with 5 min safety buffer)
        let departure_epoch = arrival_deadline_epoch - ((current_duration_min as i64 + 5) * 60);
        let rec_dt = DateTime::<Utc>::from_timestamp(departure_epoch, 0).unwrap_or_else(Utc::now);

        CommuteDelay {
            destination: destination.to_string(),
            normal_duration_min,
            current_duration_min,
            delay_minutes,
            is_congested,
            recommended_departure_iso: rec_dt.to_rfc3339(),
        }
    }

    /// Parse Google Places API JSON response into `Vec<PlaceItem>`.
    pub fn parse_places_json(json: &Value) -> Result<Vec<PlaceItem>, GoogleError> {
        let results = json
            .get("results")
            .and_then(|v| v.as_array())
            .ok_or_else(|| GoogleError::SerializationError("Missing results array in places response".into()))?;

        let mut places = Vec::new();

        for res in results {
            let name = res.get("name").and_then(|v| v.as_str()).unwrap_or_default().to_string();
            let address = res
                .get("vicinity")
                .or_else(|| res.get("formatted_address"))
                .and_then(|v| v.as_str())
                .unwrap_or_default()
                .to_string();

            let rating = res.get("rating").and_then(|v| v.as_f64()).map(|f| f as f32);
            let open_now = res.pointer("/opening_hours/open_now").and_then(|v| v.as_bool());

            places.push(PlaceItem {
                name,
                address,
                rating,
                open_now,
            });
        }

        Ok(places)
    }

    /// Construct a Google Maps navigation deep link URL.
    pub fn build_maps_nav_url(origin: &str, destination: &str, mode: TravelMode) -> String {
        let mode_str = match mode {
            TravelMode::Driving => "driving",
            TravelMode::Walking => "walking",
            TravelMode::Bicycling => "bicycling",
            TravelMode::Transit => "transit",
        };
        format!(
            "https://www.google.com/maps/dir/?api=1&origin={}&destination={}&travelmode={}",
            urlencoding_simple(origin),
            urlencoding_simple(destination),
            mode_str
        )
    }

    /// Official Google Maps "directions" URL. Unlike `build_maps_nav_url` this
    /// omits an empty origin (Maps then uses "your location") and fully
    /// percent-encodes every parameter.
    pub fn build_directions_url(origin: Option<&str>, destination: &str, mode: TravelMode) -> String {
        let mode_str = match mode {
            TravelMode::Driving => "driving",
            TravelMode::Walking => "walking",
            TravelMode::Bicycling => "bicycling",
            TravelMode::Transit => "transit",
        };
        let mut url = format!("https://www.google.com/maps/dir/?api=1&destination={}", percent_encode(destination));
        if let Some(o) = origin.map(str::trim).filter(|o| !o.is_empty()) {
            url.push_str(&format!("&origin={}", percent_encode(o)));
        }
        url.push_str(&format!("&travelmode={mode_str}"));
        url
    }

    /// JSON body for Routes API v2 `computeRoutes` (address-based endpoints).
    pub fn routes_request_body(origin: &str, destination: &str, mode: TravelMode) -> Value {
        let travel = match mode {
            TravelMode::Driving => "DRIVE",
            TravelMode::Walking => "WALK",
            TravelMode::Bicycling => "BICYCLE",
            TravelMode::Transit => "TRANSIT",
        };
        let mut body = serde_json::json!({
            "origin": { "address": origin },
            "destination": { "address": destination },
            "travelMode": travel,
        });
        // Traffic-aware routing is only valid for driving.
        if mode == TravelMode::Driving {
            body["routingPreference"] = serde_json::json!("TRAFFIC_AWARE");
        }
        body
    }

    /// Parse a Routes API v2 response (field mask: duration, distanceMeters, description).
    pub fn parse_routes_json(json: &Value) -> Result<RouteEta, GoogleError> {
        let route = json
            .pointer("/routes/0")
            .ok_or_else(|| GoogleError::NotFound("No route found for specified points".into()))?;
        let secs = route
            .get("duration")
            .and_then(|d| d.as_str())
            .and_then(|d| d.strip_suffix('s'))
            .and_then(|d| d.trim().parse::<f64>().ok())
            .ok_or_else(|| GoogleError::SerializationError("route has no duration".into()))?;
        let meters = route.get("distanceMeters").and_then(|m| m.as_u64()).unwrap_or(0);
        let via = route
            .get("description")
            .and_then(|d| d.as_str())
            .map(str::trim)
            .filter(|d| !d.is_empty())
            .map(String::from);
        Ok(RouteEta { minutes: ((secs / 60.0).round() as u32).max(1), meters, via })
    }

    /// Call Routes API v2. The key travels in a header (never in a URL or log).
    pub async fn compute_route(
        client: &reqwest::Client,
        api_key: &str,
        origin: &str,
        destination: &str,
        mode: TravelMode,
    ) -> Result<RouteEta, GoogleError> {
        let resp = client
            .post("https://routes.googleapis.com/directions/v2:computeRoutes")
            .header("X-Goog-Api-Key", api_key)
            .header("X-Goog-FieldMask", "routes.duration,routes.distanceMeters,routes.description")
            .json(&Self::routes_request_body(origin, destination, mode))
            .send()
            .await
            .map_err(|e| GoogleError::NetworkError(e.to_string()))?;
        let status = resp.status();
        if !status.is_success() {
            return Err(GoogleError::ApiError { code: status.as_u16(), message: "Routes API request failed".into() });
        }
        let json: Value = resp.json().await.map_err(|e| GoogleError::SerializationError(e.to_string()))?;
        Self::parse_routes_json(&json)
    }

    /// Places API (New) `places:searchText` body. `text_query` already contains the
    /// area ("pharmacy near 12 High St"), so no coordinates are needed.
    pub fn places_request_body(text_query: &str) -> Value {
        serde_json::json!({ "textQuery": text_query, "maxResultCount": 3, "languageCode": "en" })
    }

    /// Parse a Places API (New) response (field mask: displayName, formattedAddress, rating, openNow).
    pub fn parse_places_new_json(json: &Value) -> Vec<PlaceItem> {
        json.get("places")
            .and_then(|p| p.as_array())
            .map(|arr| {
                arr.iter()
                    .filter_map(|p| {
                        let name = p.pointer("/displayName/text").and_then(|v| v.as_str())?.trim().to_string();
                        if name.is_empty() {
                            return None;
                        }
                        Some(PlaceItem {
                            name,
                            address: p.get("formattedAddress").and_then(|v| v.as_str()).unwrap_or_default().to_string(),
                            rating: p.get("rating").and_then(|v| v.as_f64()).map(|f| f as f32),
                            open_now: p.pointer("/currentOpeningHours/openNow").and_then(|v| v.as_bool()),
                        })
                    })
                    .collect()
            })
            .unwrap_or_default()
    }

    /// Call Places API (New) text search. Key in a header only.
    pub async fn search_places(
        client: &reqwest::Client,
        api_key: &str,
        text_query: &str,
    ) -> Result<Vec<PlaceItem>, GoogleError> {
        let resp = client
            .post("https://places.googleapis.com/v1/places:searchText")
            .header("X-Goog-Api-Key", api_key)
            .header(
                "X-Goog-FieldMask",
                "places.displayName,places.formattedAddress,places.rating,places.currentOpeningHours.openNow",
            )
            .json(&Self::places_request_body(text_query))
            .send()
            .await
            .map_err(|e| GoogleError::NetworkError(e.to_string()))?;
        let status = resp.status();
        if !status.is_success() {
            return Err(GoogleError::ApiError { code: status.as_u16(), message: "Places API request failed".into() });
        }
        let json: Value = resp.json().await.map_err(|e| GoogleError::SerializationError(e.to_string()))?;
        Ok(Self::parse_places_new_json(&json))
    }

    /// Construct a Google Maps search URL.
    pub fn build_place_search_url(query: &str) -> String {
        format!(
            "https://www.google.com/maps/search/?api=1&query={}",
            urlencoding_simple(query)
        )
    }
}

/// ETA summary from the Routes API.
#[derive(Debug, Clone, PartialEq)]
pub struct RouteEta {
    pub minutes: u32,
    pub meters: u64,
    pub via: Option<String>,
}

impl RouteEta {
    /// "42 minutes via FDR Drive" / "1 hour 5 minutes".
    pub fn spoken(&self) -> String {
        let t = if self.minutes >= 60 {
            let (h, m) = (self.minutes / 60, self.minutes % 60);
            let hs = format!("{h} hour{}", if h == 1 { "" } else { "s" });
            if m == 0 { hs } else { format!("{hs} {m} minute{}", if m == 1 { "" } else { "s" }) }
        } else {
            format!("{} minute{}", self.minutes, if self.minutes == 1 { "" } else { "s" })
        };
        match &self.via {
            Some(v) => format!("{t} via {v}"),
            None => t,
        }
    }
}

/// Percent-encode everything except RFC 3986 unreserved characters.
fn percent_encode(s: &str) -> String {
    let mut out = String::with_capacity(s.len() * 2);
    for b in s.trim().bytes() {
        if b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.' | b'~') {
            out.push(b as char);
        } else {
            out.push_str(&format!("%{b:02X}"));
        }
    }
    out
}

fn urlencoding_simple(s: &str) -> String {
    s.replace(' ', "+")
        .replace('&', "%26")
        .replace(',', "%2C")
}

// ─── Unit Tests ──────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_directions_json() {
        let raw = serde_json::json!({
            "routes": [{
                "legs": [{
                    "distance": { "text": "14.2 mi", "value": 22852 },
                    "duration": { "text": "25 mins", "value": 1500 },
                    "duration_in_traffic": { "text": "40 mins", "value": 2400 },
                    "steps": [
                        { "html_instructions": "Head <b>north</b> on 1st Ave" },
                        { "html_instructions": "Turn <b>right</b> onto I-90 E" }
                    ]
                }]
            }]
        });

        let summary = MapsService::parse_directions_json(&raw, "Home", "Office").unwrap();
        assert_eq!(summary.origin, "Home");
        assert_eq!(summary.destination, "Office");
        assert_eq!(summary.distance_text, "14.2 mi");
        assert_eq!(summary.duration_text, "25 mins");
        assert_eq!(summary.duration_seconds, 1500);
        assert_eq!(summary.traffic_delay_minutes, 15);
        assert_eq!(summary.summary_steps.len(), 2);
        assert_eq!(summary.summary_steps[0], "Head north on 1st Ave");
    }

    #[test]
    fn test_commute_delay_detection() {
        // Target arrival: 1727690400 (e.g. 10:00 AM)
        let arrival_epoch = 1727690400;
        let delay = MapsService::detect_commute_delay(20, 35, "Office HQ", arrival_epoch);

        assert_eq!(delay.destination, "Office HQ");
        assert_eq!(delay.normal_duration_min, 20);
        assert_eq!(delay.current_duration_min, 35);
        assert_eq!(delay.delay_minutes, 15);
        assert!(delay.is_congested);

        // Check recommended departure time (arrival - (35 + 5) min = arrival - 2400 sec)
        let expected_departure_epoch = arrival_epoch - (40 * 60);
        let expected_iso = DateTime::<Utc>::from_timestamp(expected_departure_epoch, 0)
            .unwrap()
            .to_rfc3339();
        assert_eq!(delay.recommended_departure_iso, expected_iso);
    }

    #[test]
    fn test_parse_places_json() {
        let raw = serde_json::json!({
            "results": [
                {
                    "name": "Artisan Coffee",
                    "vicinity": "123 Main St, Springfield",
                    "rating": 4.8,
                    "opening_hours": { "open_now": true }
                },
                {
                    "name": "Quiet Library Cafe",
                    "vicinity": "456 Oak Ave",
                    "rating": 4.2,
                    "opening_hours": { "open_now": false }
                }
            ]
        });

        let places = MapsService::parse_places_json(&raw).unwrap();
        assert_eq!(places.len(), 2);
        assert_eq!(places[0].name, "Artisan Coffee");
        assert_eq!(places[0].rating, Some(4.8));
        assert_eq!(places[0].open_now, Some(true));
        assert_eq!(places[1].open_now, Some(false));
    }

    #[test]
    fn test_build_maps_urls() {
        let nav = MapsService::build_maps_nav_url("Central Park", "Times Square", TravelMode::Walking);
        assert_eq!(
            nav,
            "https://www.google.com/maps/dir/?api=1&origin=Central+Park&destination=Times+Square&travelmode=walking"
        );

        let search = MapsService::build_place_search_url("best pizza nearby");
        assert_eq!(
            search,
            "https://www.google.com/maps/search/?api=1&query=best+pizza+nearby"
        );
    }

    #[test]
    fn directions_url_encodes_and_omits_empty_origin() {
        let u = MapsService::build_directions_url(None, "Central Park, NY & Co #1?", TravelMode::Walking);
        assert_eq!(u, "https://www.google.com/maps/dir/?api=1&destination=Central%20Park%2C%20NY%20%26%20Co%20%231%3F&travelmode=walking");
        assert!(!MapsService::build_directions_url(Some("  "), "X", TravelMode::Driving).contains("origin="));
        let u = MapsService::build_directions_url(Some("London Bridge"), "Heathrow", TravelMode::Transit);
        assert!(u.contains("&origin=London%20Bridge") && u.ends_with("travelmode=transit"), "{u}");
    }

    #[test]
    fn places_new_response_parses_and_skips_nameless() {
        let j = serde_json::json!({"places":[
            {"displayName":{"text":"Boots Pharmacy"},"formattedAddress":"12 High St, London","rating":4.3,"currentOpeningHours":{"openNow":true}},
            {"displayName":{"text":" "},"formattedAddress":"x"},
            {"formattedAddress":"no name"},
            {"displayName":{"text":"Lloyds"}}
        ]});
        let p = MapsService::parse_places_new_json(&j);
        assert_eq!(p.len(), 2);
        assert_eq!((p[0].name.as_str(), p[0].open_now, p[0].rating), ("Boots Pharmacy", Some(true), Some(4.3)));
        assert_eq!((p[1].name.as_str(), p[1].open_now, p[1].address.as_str()), ("Lloyds", None, ""));
        assert!(MapsService::parse_places_new_json(&serde_json::json!({})).is_empty());
        assert_eq!(MapsService::places_request_body("pharmacy near X")["textQuery"], "pharmacy near X");
    }

    #[test]
    fn routes_body_is_traffic_aware_only_for_driving() {
        let d = MapsService::routes_request_body("A", "B", TravelMode::Driving);
        assert_eq!(d["travelMode"], "DRIVE");
        assert_eq!(d["routingPreference"], "TRAFFIC_AWARE");
        let w = MapsService::routes_request_body("A", "B", TravelMode::Walking);
        assert_eq!(w["travelMode"], "WALK");
        assert!(w.get("routingPreference").is_none());
    }

    #[test]
    fn routes_response_parses_duration_distance_and_road() {
        let j = serde_json::json!({"routes":[{"duration":"1680s","distanceMeters":13520,"description":"FDR Drive"}]});
        let eta = MapsService::parse_routes_json(&j).unwrap();
        assert_eq!((eta.minutes, eta.meters, eta.via.as_deref()), (28, 13520, Some("FDR Drive")));
        assert_eq!(eta.spoken(), "28 minutes via FDR Drive");
        let j = serde_json::json!({"routes":[{"duration":"3900s"}]});
        assert_eq!(MapsService::parse_routes_json(&j).unwrap().spoken(), "1 hour 5 minutes");
        let j = serde_json::json!({"routes":[{"duration":"20s"}]});
        assert_eq!(MapsService::parse_routes_json(&j).unwrap().spoken(), "1 minute");
        assert!(MapsService::parse_routes_json(&serde_json::json!({})).is_err());
        assert!(MapsService::parse_routes_json(&serde_json::json!({"routes":[{"distanceMeters":5}]})).is_err());
    }
}

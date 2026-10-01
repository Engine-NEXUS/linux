//! OCR client — Phase-3b text grounding tier for the screen agent.
//!
//! Sends screenshot JPEGs to the lazy RapidOCR sidecar (`lazy_ocr.rs` +
//! `server/ocr_server.py`) and parses axis-aligned text boxes in original
//! image pixels. The snap-to-text consumer lands in Phase-3c.

use std::time::Duration;

/// One OCR text box in original image pixels.
#[derive(Debug, Clone, PartialEq)]
pub struct OcrBox {
    pub text: String,
    pub x: i32,
    pub y: i32,
    pub w: i32,
    pub h: i32,
    pub conf: f32,
}

impl OcrBox {
    pub fn center(&self) -> (f64, f64) {
        (self.x as f64 + self.w as f64 / 2.0, self.y as f64 + self.h as f64 / 2.0)
    }
}

/// Merge OCR word boxes into text lines: group boxes with vertically
/// overlapping bands, then split runs separated by a gap wider than 3x
/// the line height. Fixes phrases OCR splits ("Sign" + "in") without
/// merging distant controls ("Save ... Cancel" stay separate).
/// Merged text joins with spaces; conf is the min (weakest word governs).
pub fn merge_lines(mut boxes: Vec<OcrBox>) -> Vec<OcrBox> {
    boxes.sort_by_key(|b| (b.y, b.x));
    let mut lines: Vec<Vec<OcrBox>> = Vec::new();
    for b in boxes {
        let mut placed = false;
        for line in lines.iter_mut() {
            let top = line.iter().map(|m| m.y).min().unwrap_or(b.y);
            let bot = line.iter().map(|m| m.y + m.h).max().unwrap_or(b.y + b.h);
            // Tight vertical overlap: same-row words share a baseline
            // within a few px. Loose tolerance chains adjacent rows
            // into mega-boxes (two "Play video" rows fused → both miss).
            let slack = (b.h / 4).max(2);
            if b.y < bot + slack && b.y + b.h > top - slack {
                line.push(b.clone());
                placed = true;
                break;
            }
        }
        if !placed {
            lines.push(vec![b]);
        }
    }
    let mut out = Vec::new();
    for mut line in lines {
        line.sort_by_key(|b| b.x);
        let line_h = line.iter().map(|b| b.h).max().unwrap_or(1).max(1);
        // Within-phrase word gaps are ~1 cap-height; anything wider is a
        // separate control (3x fused adjacent "Play video" rows — see
        // vision_accuracy harness 2026-09-27).
        let gap_limit = line_h.max(8);
        let mut run: Vec<OcrBox> = Vec::new();
        let flush = |run: &mut Vec<OcrBox>, out: &mut Vec<OcrBox>| {
            if run.is_empty() {
                return;
            }
            let x0 = run.iter().map(|b| b.x).min().unwrap();
            let y0 = run.iter().map(|b| b.y).min().unwrap();
            let x1 = run.iter().map(|b| b.x + b.w).max().unwrap();
            let y1 = run.iter().map(|b| b.y + b.h).max().unwrap();
            out.push(OcrBox {
                text: run.iter().map(|b| b.text.clone()).collect::<Vec<_>>().join(" "),
                x: x0,
                y: y0,
                w: x1 - x0,
                h: y1 - y0,
                conf: run.iter().map(|b| b.conf).fold(1.0f32, f32::min),
            });
            run.clear();
        };
        let mut prev_right = i32::MIN;
        for b in line {
            if !run.is_empty() && b.x - prev_right > gap_limit {
                flush(&mut run, &mut out);
            }
            prev_right = b.x + b.w;
            run.push(b);
        }
        flush(&mut run, &mut out);
    }
    out
}

/// Snap a VLM point to the best-matching OCR text box.
///
/// Candidates score `screen::score_match(query, box.text) >= 0.5`; the
/// highest score wins, ties break by distance to the VLM point. Rejects
/// when the winner is farther than `max_dist` (px, same space as boxes)
/// — a far snap is worse than raw VLM coords. Returns the box center.
pub fn snap_to_text(
    boxes: &[OcrBox],
    query: &str,
    px: f64,
    py: f64,
    max_dist: f64,
) -> Option<(f64, f64)> {
    let mut best: Option<(f32, f64, (f64, f64))> = None;
    for b in boxes {
        let score = crate::screen::score_match(query, &b.text);
        if score < 0.5 {
            continue;
        }
        let (cx, cy) = b.center();
        let dist = ((cx - px).powi(2) + (cy - py).powi(2)).sqrt();
        let replace = match &best {
            None => true,
            Some((s, d, _)) => score > *s || (score == *s && dist < *d),
        };
        if replace {
            best = Some((score, dist, (cx, cy)));
        }
    }
    match best {
        Some((_, dist, center)) if dist <= max_dist => Some(center),
        _ => None,
    }
}

/// Lenient parse of the `/ocr` response: skips malformed entries and
/// boxes with empty text or non-positive size, never fails outright.
pub fn parse_response(json: &serde_json::Value) -> Vec<OcrBox> {
    let mut out = Vec::new();
    let boxes = match json.get("boxes").and_then(|b| b.as_array()) {
        Some(b) => b,
        None => return out,
    };
    for b in boxes {
        let text = b.get("text").and_then(|t| t.as_str()).unwrap_or("").trim();
        if text.is_empty() {
            continue;
        }
        let num = |k: &str| b.get(k).and_then(|v| v.as_i64()).unwrap_or(-1);
        let (x, y, w, h) = (num("x"), num("y"), num("w"), num("h"));
        if w <= 0 || h <= 0 {
            continue;
        }
        out.push(OcrBox {
            text: text.chars().take(120).collect(),
            x: x as i32,
            y: y as i32,
            w: w as i32,
            h: h as i32,
            conf: b.get("conf").and_then(|c| c.as_f64()).unwrap_or(0.0) as f32,
        });
    }
    out
}

/// OCR a JPEG: ensure the sidecar is up, POST raw bytes, parse boxes.
/// Total budget 40s (cold model load happens server-side on first use).
pub async fn ocr_image(jpeg: &[u8]) -> Result<Vec<OcrBox>, String> {
    if jpeg.is_empty() {
        return Err("empty image".into());
    }
    crate::lazy_ocr::ensure_ocr_running();
    crate::lazy_ocr::mark_ocr_request();
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(40))
        .build()
        .map_err(|e| format!("http client: {e}"))?;
    let url = format!("http://127.0.0.1:{}/ocr", crate::lazy_ocr::ocr_port());
    let resp = client
        .post(&url)
        .header("Content-Type", "image/jpeg")
        .body(jpeg.to_vec())
        .send()
        .await
        .map_err(|e| format!("ocr request failed: {e}"))?;
    if !resp.status().is_success() {
        return Err(format!("ocr HTTP {}", resp.status()));
    }
    let json: serde_json::Value = resp
        .json()
        .await
        .map_err(|e| format!("ocr response parse error: {e}"))?;
    if let Some(err) = json.get("error").and_then(|e| e.as_str()) {
        return Err(format!("ocr server: {err}"));
    }
    Ok(parse_response(&json))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> serde_json::Value {
        serde_json::json!({
            "boxes": [
                {"text": "Save", "x": 59, "y": 60, "w": 76, "h": 13, "conf": 0.982},
                {"text": "   ", "x": 1, "y": 1, "w": 5, "h": 5, "conf": 0.5},
                {"text": "Flat", "x": 1, "y": 1, "w": 0, "h": 5, "conf": 0.5},
                {"text": "NoNumbers"},
                {"oops": true},
            ],
            "latency_ms": 244
        })
    }

    #[test]
    fn parse_keeps_good_boxes_skips_bad() {
        let boxes = parse_response(&sample());
        assert_eq!(boxes.len(), 1);
        assert_eq!(boxes[0].text, "Save");
        assert_eq!((boxes[0].x, boxes[0].y, boxes[0].w, boxes[0].h), (59, 60, 76, 13));
        assert!((boxes[0].conf - 0.982).abs() < 1e-6);
    }

    #[test]
    fn parse_empty_and_missing_boxes() {
        assert!(parse_response(&serde_json::json!({"boxes": []})).is_empty());
        assert!(parse_response(&serde_json::json!({})).is_empty());
        assert!(parse_response(&serde_json::json!({"boxes": "nope"})).is_empty());
    }

    #[test]
    fn box_center_is_middle() {
        let b = OcrBox { text: "x".into(), x: 10, y: 20, w: 30, h: 40, conf: 1.0 };
        assert_eq!(b.center(), (25.0, 40.0));
    }

    fn word(text: &str, x: i32, y: i32, w: i32) -> OcrBox {
        OcrBox { text: text.into(), x, y, w, h: 20, conf: 0.9 }
    }

    #[test]
    fn merge_lines_joins_split_phrases() {
        let boxes = vec![word("tab", 60, 100, 30), word("New", 10, 100, 40)];
        let merged = merge_lines(boxes);
        assert_eq!(merged.len(), 1);
        assert_eq!(merged[0].text, "New tab");
        assert_eq!((merged[0].x, merged[0].w), (10, 80));
    }

    #[test]
    fn merge_lines_keeps_distant_controls_apart() {
        let boxes = vec![word("Save", 10, 100, 50), word("Cancel", 900, 100, 60)];
        let merged = merge_lines(boxes);
        assert_eq!(merged.len(), 2);
    }

    #[test]
    fn merge_lines_keeps_adjacent_rows_apart() {
        // Two "Play video" rows 28px apart with a 46px x-gap: same line
        // band, but the gap exceeds 1x line height → must NOT fuse.
        let boxes = vec![
            OcrBox { text: "Play video".into(), x: 335, y: 368, w: 123, h: 36, conf: 0.97 },
            OcrBox { text: "Play video".into(), x: 504, y: 340, w: 120, h: 34, conf: 0.99 },
        ];
        let merged = merge_lines(boxes);
        assert_eq!(merged.len(), 2);
    }

    #[test]
    fn merge_lines_separates_rows() {
        let boxes = vec![word("Top", 10, 10, 40), word("Bottom", 10, 200, 60)];
        let merged = merge_lines(boxes);
        assert_eq!(merged.len(), 2);
    }

    fn snap_box(text: &str, x: i32, y: i32) -> OcrBox {
        OcrBox { text: text.into(), x, y, w: 60, h: 20, conf: 0.9 }
    }

    #[test]
    fn snap_picks_best_text_match() {
        let boxes = vec![snap_box("Cancel", 500, 500), snap_box("Save Document", 100, 100)];
        // VLM point is near Cancel, but text match wins for "save".
        let hit = snap_to_text(&boxes, "save", 490.0, 490.0, 1000.0).unwrap();
        assert_eq!(hit, (130.0, 110.0));
    }

    #[test]
    fn snap_tie_breaks_by_distance() {
        let boxes = vec![snap_box("Save", 0, 0), snap_box("Save", 1000, 1000)];
        let hit = snap_to_text(&boxes, "save", 990.0, 990.0, 2000.0).unwrap();
        assert_eq!(hit, (1030.0, 1010.0));
    }

    #[test]
    fn snap_rejects_far_and_weak_matches() {
        let boxes = vec![snap_box("Save", 0, 0)];
        // Right text, too far → keep raw VLM coords (None).
        assert!(snap_to_text(&boxes, "save", 900.0, 900.0, 120.0).is_none());
        // Wrong text → None even when close.
        assert!(snap_to_text(&boxes, "eiffel tower", 30.0, 10.0, 120.0).is_none());
        assert!(snap_to_text(&[], "save", 0.0, 0.0, 120.0).is_none());
    }
}

//! Screen OCR — zero-RAM local text extraction.
//!
//! Two implementations, one per platform family:
//!
//! * **Windows**: `Windows.Media.Ocr` — OS-native, no model download, no GPU,
//!   no cloud, ~15-50ms per pass. Used by the screen-analysis fallback when no
//!   Gemini key is configured: the user still gets a real "what's on my
//!   screen" answer (extracted text in the sidebar + a spoken summary line)
//!   with zero API cost, fully offline.
//!
//!   Pipeline: GDI capture (existing sidebar_backdrop helper) → PNG bytes →
//!   InMemoryRandomAccessStream → BitmapDecoder → SoftwareBitmap →
//!   OcrEngine.RecognizeAsync → joined line text + line rects.
//!
//! * **Linux**: a local RapidOCR sidecar (`lazy_ocr.rs` +
//!   `server/ocr_server.py`), which returns axis-aligned text boxes in
//!   original image pixels. Those boxes are what a vision model's answer is
//!   snapped to, so a click lands on the text the user meant rather than on
//!   the model's guess.
//!
//! Fail-open on the Windows side (no engine / no language pack / decode fail
//! → `None`, the caller falls back to the UIA count). The Linux side reports
//! its failures instead, because "no text" and "the OCR server is not
//! installed" are different answers.

// ─── Shared text-box plumbing ─────────────────────────────────────────────
//
// The box shape and the snapping rule are platform-independent: the Linux
// RapidOCR sidecar produces the boxes today, and the Windows OCR engine
// produces the same shape (one rect per line). Keeping them HERE means both
// platforms feed the same rule, and the rule is unit-tested on every
// platform.
//
// `score_match` is a private helper on purpose. The fuzzy-match rule lives
// in `screen.rs`, which belongs to another team, so it is carried here
// rather than borrowed across the boundary.

/// One OCR text box, in screen pixels.
#[derive(Debug, Clone, PartialEq)]
pub struct OcrBox {
    pub text: String,
    pub x: i32,
    pub y: i32,
    pub w: i32,
    pub h: i32,
    /// The engine's 0..1 confidence for this text. The Windows line engine
    /// exposes no score, so its boxes carry a fixed `1.0`; a value outside
    /// 0..1 is a malformed reply and is never used as a snap target.
    pub conf: f32,
}

impl OcrBox {
    /// The centre of the box, in the same pixel space as the box.
    pub fn center(&self) -> (f64, f64) {
        (
            self.x as f64 + self.w as f64 / 2.0,
            self.y as f64 + self.h as f64 / 2.0,
        )
    }
}

/// Pure: fuzzy match score 0.0-1.0 of a locate query against a UI name.
/// Exact > substring > all-words > any-word. Locate hits need >= 0.5.
fn score_match(query: &str, name: &str) -> f32 {
    let norm = |s: &str| {
        s.to_lowercase()
            .split(|c: char| !c.is_alphanumeric())
            .filter(|w| !w.is_empty())
            .collect::<Vec<_>>()
            .join(" ")
    };
    let q = norm(query);
    let n = norm(name);
    if q.is_empty() || n.is_empty() {
        return 0.0;
    }
    if q == n {
        return 1.0;
    }
    if n.contains(&q) || q.contains(&n) {
        return 0.8;
    }
    // Space-insensitive containment: OCR routinely drops inter-word
    // spaces ("Closewindow", "Signin"). Scores just below substring.
    let nospace = |s: &str| s.split_whitespace().collect::<String>();
    let (nq, nn) = (nospace(&q), nospace(&n));
    if nn.contains(&nq) || nq.contains(&nn) {
        return 0.75;
    }
    let qw: Vec<&str> = q.split(' ').collect();
    let nw: Vec<&str> = n.split(' ').collect();
    let hits = qw.iter().filter(|w| nw.contains(w)).count();
    if hits == qw.len() {
        return 0.7;
    }
    if hits > 0 {
        return 0.4;
    }
    0.0
}

/// Snap a vision point onto the best-matching OCR text box.
///
/// Candidates score `score_match(query, box.text) >= 0.5`; the highest score
/// wins, ties break by distance to the vision point. Rejects when the winner
/// is farther than `max_dist` (px) — a far snap is worse than the model's own
/// coordinates. Boxes whose confidence is not a 0..1 engine score are ignored
/// as a malformed reply. Returns the box centre.
///
/// `boxes` and the point MUST be in the same pixel space (screen pixels),
/// otherwise the distance comparison compares two different pictures.
pub fn snap_to_text(
    boxes: &[OcrBox],
    query: &str,
    px: f64,
    py: f64,
    max_dist: f64,
) -> Option<(f64, f64)> {
    let mut best: Option<(f32, f64, (f64, f64))> = None;
    for b in boxes {
        // Confidence is the one field the two engines do not share: the
        // Linux sidecar reports the model's 0..1 score, the Windows line
        // engine exposes no score and carries a fixed 1.0. Anything outside
        // that range is a malformed reply, so the box cannot be trusted.
        // This is an input check, not a ranking rule: valid boxes are ranked
        // exactly as before.
        if !(0.0..=1.0).contains(&b.conf) {
            continue;
        }
        let score = score_match(query, &b.text);
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

#[cfg(target_os = "windows")]
mod windows_ocr {
    //! Windows.Media.Ocr — zero-RAM local screen text extraction (Feature 86).
    //!
    //! Fail-open everywhere: no engine / no language pack / decode fail →
    //! `None` (caller falls back to the UIA count).

    use windows::Globalization::Language;
    use windows::Graphics::Imaging::{BitmapDecoder, SoftwareBitmap};
    use windows::Media::Ocr::OcrEngine;
    use windows::Storage::Streams::{DataWriter, InMemoryRandomAccessStream};

    /// True when an OcrEngine can be created on this machine (language pack
    /// present). Cheap; call once per screen-analysis turn.
    #[allow(dead_code)]
    pub fn is_available() -> bool {
        engine().is_some()
    }

    fn engine() -> Option<OcrEngine> {
        // Try user-profile languages first, then explicit en-us.
        if let Ok(e) = OcrEngine::TryCreateFromUserProfileLanguages() {
            return Some(e);
        }
        let lang = Language::CreateLanguage(&windows::core::HSTRING::from("en-US")).ok()?;
        OcrEngine::TryCreateFromLanguage(&lang).ok()
    }

    /// Capture the primary monitor and OCR all visible text.
    /// Returns (full_text, line_rects) — rects are PHYSICAL px (x, y, w, h).
    /// None on any failure (capture, engine, decode).
    pub fn capture_screen_text() -> Option<(String, Vec<(i32, i32, i32, i32)>)> {
        let (mw, mh) = crate::screen::primary_monitor_size()?;
        capture_region_text_rects(0, 0, mw, mh)
    }

    /// OCR one screen region (physical px). Rects are relative to the region's
    /// top-left corner.
    pub fn capture_region_text_rects(
        x: i32,
        y: i32,
        mw: i32,
        mh: i32,
    ) -> Option<(String, Vec<(i32, i32, i32, i32)>)> {
        if mw <= 0 || mh <= 0 {
            return None;
        }
        let bgra = crate::sidebar_backdrop::capture_region_bgra_public(x, y, mw, mh)?;
        // BGRA (BitBlt, bottom-up rows? capture_region_bgra is top-down DIB)
        // → RGBA PNG in memory (image crate, already a dependency).
        let mut rgba: Vec<u8> = Vec::with_capacity(bgra.len());
        for px in bgra.chunks_exact(4) {
            rgba.push(px[2]); // R
            rgba.push(px[1]); // G
            rgba.push(px[0]); // B
            rgba.push(255);
        }
        let img = image::RgbaImage::from_raw(mw as u32, mh as u32, rgba)?;
        let mut png: Vec<u8> = Vec::new();
        image::DynamicImage::ImageRgba8(img)
            .write_to(&mut std::io::Cursor::new(&mut png), image::ImageFormat::Png)
            .ok()?;

        // PNG bytes → SoftwareBitmap via BitmapDecoder over an in-memory stream.
        let stream = InMemoryRandomAccessStream::new().ok()?;
        let writer = DataWriter::CreateDataWriter(&stream).ok()?;
        writer.WriteBytes(&png).ok()?;
        writer.StoreAsync().ok()?.get().ok()?;
        writer.FlushAsync().ok()?.get().ok()?;
        writer.DetachStream().ok()?;
        stream.Seek(0).ok()?;

        let decoder = BitmapDecoder::CreateAsync(&stream).ok()?.get().ok()?;
        let bitmap: SoftwareBitmap = decoder.GetSoftwareBitmapAsync().ok()?.get().ok()?;

        // OCR.
        let ocr_engine = engine()?;
        let result = ocr_engine.RecognizeAsync(&bitmap).ok()?.get().ok()?;

        let lines = result.Lines().ok()?;
        let line_count = lines.Size().ok()?;
        let mut lines_out: Vec<String> = Vec::new();
        let mut rects: Vec<(i32, i32, i32, i32)> = Vec::new();
        for i in 0..line_count {
            let line = match lines.GetAt(i) {
                Ok(l) => l,
                Err(_) => continue,
            };
            let text = match line.Text() {
                Ok(t) => t.to_string(),
                Err(_) => continue,
            };
            if text.trim().is_empty() {
                continue;
            }
            // Line bounding rect: union of word rects (physical px).
            let mut min_x = i32::MAX;
            let mut min_y = i32::MAX;
            let mut max_x = 0i32;
            let mut max_y = 0i32;
            if let Ok(words) = line.Words() {
                if let Ok(word_count) = words.Size() {
                    for j in 0..word_count {
                        let word = match words.GetAt(j) {
                            Ok(w) => w,
                            Err(_) => continue,
                        };
                        let b = match word.BoundingRect() {
                            Ok(bb) => bb,
                            Err(_) => continue,
                        };
                        let (x, y) = (b.X as i32, b.Y as i32);
                        let (w, h) = (b.Width as i32, b.Height as i32);
                        min_x = min_x.min(x);
                        min_y = min_y.min(y);
                        max_x = max_x.max(x + w);
                        max_y = max_y.max(y + h);
                    }
                }
            }
            if min_x == i32::MAX {
                lines_out.push(text);
                rects.push((0, 0, 0, 0));
                continue;
            }
            lines_out.push(text);
            rects.push((min_x, min_y, max_x - min_x, max_y - min_y));
        }
        if lines_out.is_empty() {
            return None;
        }
        Some((lines_out.join("\n"), rects))
    }

    /// OCR text of one screen region, no rects.
    pub fn capture_region_text(x: i32, y: i32, w: i32, h: i32) -> Option<String> {
        capture_region_text_rects(x, y, w, h).map(|(t, _)| t)
    }

    /// OCR text only (no rects) — convenience wrapper.
    #[allow(dead_code)]
    pub fn capture_screen_text_only() -> Option<String> {
        capture_screen_text().map(|(t, _)| t)
    }

    /// The visible screen text as snap-able boxes, one per line.
    ///
    /// The engine reports the joined text plus one rect per line, so the
    /// lines are zipped back onto their rects. The rects are PHYSICAL px on
    /// the primary monitor — the same space the vision tier reports a point
    /// in, which is what makes the two comparable.
    pub fn screen_text_boxes() -> Vec<super::OcrBox> {
        let Some((text, rects)) = capture_screen_text() else {
            return Vec::new();
        };
        text.lines()
            .zip(rects.iter())
            .filter_map(|(line, (x, y, w, h))| {
                let t = line.trim();
                if t.is_empty() || *w <= 0 || *h <= 0 {
                    return None;
                }
                Some(super::OcrBox {
                    text: t.to_string(),
                    x: *x,
                    y: *y,
                    w: *w,
                    h: *h,
                    conf: 1.0,
                })
            })
            .collect()
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        /// LIVE probe (needs a real screen + language pack): run explicitly
        /// with `cargo test --lib ocr -- --ignored`. Ignored by default so
        /// headless CI never runs it.
        #[test]
        #[ignore]
        fn test_ocr_probe_live_screen() {
            let out = capture_screen_text();
            match out {
                Some((text, rects)) => {
                    let first: String = text.lines().next().unwrap_or("").to_string();
                    println!("OCR OK: {} lines, {} rects; first line: {:?}", text.lines().count(), rects.len(), first);
                    assert!(!text.trim().is_empty());
                    assert_eq!(rects.len(), text.lines().count());
                }
                None => panic!("OCR returned None — engine/language pack/capture failed on this machine"),
            }
        }
    }
}

#[cfg(target_os = "linux")]
mod linux_ocr {
    //! Local OCR client — text boxes for the pointer's snap-to-text tier.
    //!
    //! Sends screenshot JPEGs to the lazy RapidOCR sidecar (`lazy_ocr.rs` +
    //! `server/ocr_server.py`) and parses axis-aligned text boxes in original
    //! image pixels.

    use std::time::Duration;

    use super::OcrBox;

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
            // separate control.
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
        // Starting the sidecar is a SYNCHRONOUS poll loop, and the first
        // start has to load the ONNX models — up to a minute on a machine
        // that has to fetch them. Running it on an async worker thread
        // would stall every other task on that thread for that long, so it
        // goes to a blocking thread instead.
        tokio::task::spawn_blocking(crate::lazy_ocr::ensure_ocr_running)
            .await
            .map_err(|e| format!("ocr sidecar start failed: {e}"))?;
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
    }
}

#[cfg(target_os = "windows")]
pub use windows_ocr::*;

#[cfg(target_os = "linux")]
pub use linux_ocr::*;

// ─── Tests for the shared (platform-independent) rules ─────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn score_match_exact_beats_substring() {
        assert_eq!(score_match("save", "Save"), 1.0);
        assert_eq!(score_match("save button", "Save Button"), 1.0);
    }

    #[test]
    fn score_match_substring_hits() {
        assert!(((score_match("save", "Save As")) - 0.8).abs() < 1e-6);
        // Not contiguous ("address and search bar") → all-words score 0.7.
        assert!(((score_match("address bar", "Address and search bar")) - 0.7).abs() < 1e-6);
        assert!(((score_match("search bar", "Address and search bar")) - 0.8).abs() < 1e-6);
    }

    #[test]
    fn score_match_ignores_dropped_ocr_spaces() {
        // RapidOCR emits "Closewindow" for "Close window" — must still hit.
        assert!(((score_match("close window", "Closewindow")) - 0.75).abs() < 1e-6);
        assert!(((score_match("sign in", "Signin")) - 0.75).abs() < 1e-6);
    }

    #[test]
    fn score_match_word_overlap_ranks() {
        // All query words present, different order → 0.7 (above threshold).
        assert!(((score_match("button save", "Save Button")) - 0.7).abs() < 1e-6);
        // One word only → 0.4 (below threshold, correctly rejected).
        assert!(((score_match("save file", "Save Button")) - 0.4).abs() < 1e-6);
        assert_eq!(score_match("eiffel tower", "Save Button"), 0.0);
        assert_eq!(score_match("", "Save"), 0.0);
    }

    fn box_at(text: &str, x: i32, y: i32, w: i32, h: i32) -> OcrBox {
        OcrBox {
            text: text.into(),
            x,
            y,
            w,
            h,
            conf: 0.9,
        }
    }

    #[test]
    fn snap_to_text_picks_the_matching_box() {
        let boxes = vec![
            box_at("Cancel", 10, 10, 60, 20),
            box_at("Save", 100, 10, 60, 20),
        ];
        let (x, y) = snap_to_text(&boxes, "save", 130.0, 20.0, 120.0).expect("the Save box");
        assert!((x - 130.0).abs() < 1e-6);
        assert!((y - 20.0).abs() < 1e-6);
    }

    #[test]
    fn snap_to_text_ignores_a_box_that_does_not_match() {
        let boxes = vec![box_at("Cancel", 10, 10, 60, 20)];
        assert!(snap_to_text(&boxes, "save button", 40.0, 20.0, 120.0).is_none());
    }

    #[test]
    fn snap_to_text_refuses_a_far_snap() {
        // A distant box is worse than the model's own coordinates.
        let boxes = vec![box_at("Save", 900, 800, 60, 20)];
        assert!(snap_to_text(&boxes, "save", 20.0, 20.0, 120.0).is_none());
    }

    #[test]
    fn snap_to_text_prefers_the_better_match_over_the_nearer_box() {
        let boxes = vec![
            box_at("Save As", 10, 10, 60, 20), // score 0.8
            box_at("Save", 100, 10, 60, 20),   // score 1.0
        ];
        let (x, _) = snap_to_text(&boxes, "save", 40.0, 20.0, 120.0).expect("a snap");
        assert!((x - 130.0).abs() < 1e-6);
    }

    #[test]
    fn snap_to_text_of_nothing_is_nothing() {
        assert!(snap_to_text(&[], "save", 10.0, 10.0, 120.0).is_none());
    }

    #[test]
    fn snap_to_text_ignores_a_malformed_confidence() {
        // Both engines produce 0..1 scores (Windows carries a fixed 1.0);
        // anything else is a malformed reply and must not become a target.
        let mut boxes = vec![box_at("Save", 100, 10, 60, 20)];
        boxes[0].conf = f32::NAN;
        assert!(snap_to_text(&boxes, "save", 130.0, 20.0, 120.0).is_none());
        boxes[0].conf = 1.5;
        assert!(snap_to_text(&boxes, "save", 130.0, 20.0, 120.0).is_none());
        boxes[0].conf = 1.0;
        assert!(snap_to_text(&boxes, "save", 130.0, 20.0, 120.0).is_some());
    }
}

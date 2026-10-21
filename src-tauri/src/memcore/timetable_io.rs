//! OS / network glue for the timetable (everything pure lives in `timetable`):
//! getting an image (screen capture or clipboard), asking the vision model to
//! read it, and opening the sites for an activity.

use tauri::{AppHandle, Manager, Runtime};

use super::timetable::{self, Choice, Extracted, Target, Want};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExtractError {
    /// No Gemini key configured.
    NoKey,
    /// Daily vision quota used up (or every model answered 429).
    Quota,
    /// The model answered, but nothing usable (not a timetable, unreadable).
    Unreadable,
    Timeout,
}

/// Largest side sent to the model. Timetables are text-heavy, so this is
/// higher than the screen-tour capture.
const IMAGE_MAX_W: u32 = 1600;
/// Reject absurd clipboard images before decoding them into RGBA.
const MAX_CLIPBOARD_PIXELS: u64 = 40_000_000;

/// Whole-screen capture for "analyse this".
pub fn capture_screen_b64() -> Option<String> {
    crate::vision::capture_plain_jpeg_base64_w(IMAGE_MAX_W).map(|(b64, _, _)| b64)
}

/// Image currently on the clipboard (a pasted screenshot), as JPEG base64.
pub fn clipboard_image_b64() -> Option<String> {
    let mut clipboard = arboard::Clipboard::new().ok()?;
    let img = clipboard.get_image().ok()?;
    let (w, h) = (img.width as u32, img.height as u32);
    if w == 0 || h == 0 || (w as u64) * (h as u64) > MAX_CLIPBOARD_PIXELS {
        return None;
    }
    let rgba = image::RgbaImage::from_raw(w, h, img.bytes.into_owned())?;
    let scale = (IMAGE_MAX_W as f32 / w as f32).min(1.0);
    let (tw, th) = (((w as f32 * scale) as u32).max(1), ((h as f32 * scale) as u32).max(1));
    let small = image::imageops::resize(&rgba, tw, th, image::imageops::FilterType::Triangle);
    let mut jpeg = Vec::new();
    image::codecs::jpeg::JpegEncoder::new_with_quality(&mut jpeg, 80)
        .encode_image(&image::DynamicImage::ImageRgb8(image::DynamicImage::ImageRgba8(small).to_rgb8()))
        .ok()?;
    use base64::Engine;
    Some(base64::engine::general_purpose::STANDARD.encode(&jpeg))
}

/// Ask the vision model to read the timetable in `b64`. Shares the Gemini
/// quota ledger with the screen tour and click grounding.
pub async fn extract<R: Runtime>(
    app: &AppHandle<R>,
    b64: &str,
    want: Option<&Want>,
) -> Result<Extracted, ExtractError> {
    let key = crate::commands::read_api_key(app, "gemini");
    if key.is_empty() {
        return Err(ExtractError::NoKey);
    }
    let dir = app.path().app_data_dir().map_err(|_| ExtractError::Unreadable)?;
    if crate::vision::exhausted(&dir, "gemini") {
        return Err(ExtractError::Quota);
    }
    // Strongest models first; a timetable is read once, so latency matters
    // less than reading it right. (The shared vision client has a 4 s cap
    // sized for click grounding, which is too short here.)
    let models = crate::vision::tour_model_ladder(None);
    let client = reqwest::Client::builder()
        .connect_timeout(std::time::Duration::from_secs(5))
        .timeout(std::time::Duration::from_secs(30))
        .build()
        .map_err(|_| ExtractError::Unreadable)?;
    let prompt = timetable::build_prompt(want);
    let call = crate::vision::gemini_json_with_image(
        &prompt,
        b64,
        &key,
        &client,
        4096,
        &models,
        timetable::parse_extraction,
    );
    match tokio::time::timeout(std::time::Duration::from_secs(45), call).await {
        Err(_) => Err(ExtractError::Timeout),
        Ok(crate::vision::JsonStep::Found(ex)) => {
            crate::vision::record_use(&dir, "gemini");
            Ok(ex)
        }
        Ok(crate::vision::JsonStep::Quota) => {
            crate::vision::mark_exhausted(&dir, "gemini");
            Err(ExtractError::Quota)
        }
        Ok(crate::vision::JsonStep::Miss) => Err(ExtractError::Unreadable),
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Opened {
    Browser,
    App,
    /// The user prefers the app but none is installed: opened in the browser.
    BrowserInstead,
    Failed,
}

/// Open one target the way the user prefers.
pub fn open_target(t: &Target, choice: Choice) -> Opened {
    if choice == Choice::App {
        let label = timetable::label_for(t.site).to_lowercase();
        if let Some(entry) = crate::app_registry::lookup(&label) {
            return if crate::app_registry::launch(&entry).is_ok() { Opened::App } else { Opened::Failed };
        }
        return if open::that(&t.url).is_ok() { Opened::BrowserInstead } else { Opened::Failed };
    }
    if open::that(&t.url).is_ok() { Opened::Browser } else { Opened::Failed }
}

/// Spoken summary of what happened when an activity was started. Pure.
pub fn start_speech(title: &str, targets: &[Target], outcomes: &[(String, Opened)], youtube_missing: bool) -> String {
    if targets.is_empty() {
        return format!("Okay, {title} it is. I have nothing to open for it.");
    }
    let mut parts: Vec<String> = vec![];
    let mut fell_back: Vec<&str> = vec![];
    for (t, (_, o)) in targets.iter().zip(outcomes.iter()) {
        let name = timetable::label_for(t.site);
        match o {
            Opened::Failed => parts.push(format!("I couldn't open {name}")),
            Opened::BrowserInstead => {
                fell_back.push(name);
                parts.push(format!("opened {name}"));
            }
            _ => {
                if t.resumed {
                    parts.push(format!("opened {name} where you left off"));
                } else {
                    parts.push(format!("opened {name}"));
                }
            }
        }
    }
    let mut s = format!("Starting {title}: {}.", parts.join(", and "));
    if !fell_back.is_empty() {
        s.push_str(&format!(" {} has no desktop app here, so it opened in the browser.", fell_back.join(" and ")));
    }
    if youtube_missing {
        s.push_str(" I don't have a video from your last session, so I left YouTube alone.");
    }
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    fn t(site: &'static str, resumed: bool) -> Target {
        Target { site, url: format!("https://{site}/x"), resumed }
    }

    #[test]
    fn speech_resumed_fallback_and_missing_video() {
        let targets = vec![t("leetcode.com", true), t("youtube.com", true)];
        let outcomes = vec![("a".to_string(), Opened::Browser), ("b".to_string(), Opened::Browser)];
        assert_eq!(
            start_speech("DSA", &targets, &outcomes, false),
            "Starting DSA: opened LeetCode where you left off, and opened YouTube where you left off."
        );
        let one = vec![t("leetcode.com", false)];
        let s = start_speech("DSA", &one, &[("a".into(), Opened::Browser)], true);
        assert!(s.contains("opened LeetCode.") && s.contains("left YouTube alone"), "{s}");
        let s = start_speech("DSA", &one, &[("a".into(), Opened::BrowserInstead)], false);
        assert!(s.contains("LeetCode has no desktop app here"), "{s}");
        let s = start_speech("DSA", &one, &[("a".into(), Opened::Failed)], false);
        assert!(s.contains("couldn't open LeetCode"), "{s}");
        assert!(start_speech("Gym", &[], &[], false).contains("nothing to open"));
    }

    /// Live: needs an image on the Windows clipboard.
    /// cargo test --lib live_clipboard_image -- --ignored --nocapture
    #[test]
    #[ignore = "reads the real clipboard"]
    fn live_clipboard_image() {
        let b64 = clipboard_image_b64().expect("no image on the clipboard");
        println!("clipboard image -> {} base64 chars", b64.len());
        assert!(b64.len() > 1000);
    }

    /// Live: sends ONE request to Gemini with the key from settings.json.
    /// NEXUS_TT_IMAGE=<path to jpg> cargo test --lib live_extract_image -- --ignored --nocapture
    #[test]
    #[ignore = "network + Gemini key + 1 vision request"]
    fn live_extract_image() {
        use base64::Engine;
        let path = std::env::var("NEXUS_TT_IMAGE").expect("set NEXUS_TT_IMAGE");
        let bytes = std::fs::read(&path).unwrap();
        let b64 = base64::engine::general_purpose::STANDARD.encode(&bytes);
        let settings = std::path::PathBuf::from(std::env::var("APPDATA").unwrap())
            .join("com.nexus.assistant")
            .join("settings.json");
        let json: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(settings).unwrap()).unwrap();
        let key = json["geminiApiKey"].as_str().filter(|k| !k.is_empty()).expect("no geminiApiKey in settings.json").to_string();
        let want = Some(Want::Number(2));
        let models = crate::vision::tour_model_ladder(None);
        let client = reqwest::Client::builder().timeout(std::time::Duration::from_secs(30)).build().unwrap();
        let prompt = timetable::build_prompt(want.as_ref());
        let rt = tokio::runtime::Runtime::new().unwrap();
        let step = rt.block_on(crate::vision::gemini_json_with_image(
            &prompt, &b64, &key, &client, 4096, &models, timetable::parse_extraction,
        ));
        match step {
            crate::vision::JsonStep::Found(ex) => {
                for sec in &ex.sections {
                    println!("SECTION {:?}", sec.name);
                    for sl in &sec.slots {
                        println!("   {} | {} | {}", timetable::describe(sl), timetable::fmt_days(&sl.days), sl.id);
                    }
                }
                let picked = timetable::select_section(&ex, want.as_ref()).unwrap();
                println!("SECTION 2 PICK -> {:?}", picked.iter().map(timetable::describe).collect::<Vec<_>>());
                assert_eq!(picked.len(), 3, "expected DSA practice, Revision, Dinner");
            }
            crate::vision::JsonStep::Quota => panic!("quota"),
            crate::vision::JsonStep::Miss => panic!("no usable answer"),
        }
    }
}

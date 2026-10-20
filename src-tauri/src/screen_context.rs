//! What the user is actually looking at — context for the narrated screen
//! tour ("Nexus, analyse my screen").
//!
//! Pixels alone cannot say what a page or video IS, so the tour also gets:
//! the foreground app, the window/tab title, the page URL (privacy-trimmed)
//! and, for YouTube, the video title + channel from YouTube's public oEmbed
//! endpoint (no key; only the clean video URL is sent). It also decides the
//! screen REGION to capture: the page content only (UI Automation `Document`
//! for browsers, else the window), so tabs / URL bar / taskbar never reach
//! the model. Everything pure here is unit-tested; the Windows probes and the
//! oEmbed call are thin glue around it.

use std::collections::HashMap;

/// Physical-px rectangle on the primary monitor.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Region {
    pub x: i32,
    pub y: i32,
    pub w: i32,
    pub h: i32,
}

impl Region {
    pub fn full(w: i32, h: i32) -> Self {
        Self { x: 0, y: 0, w, h }
    }

    pub fn is_full(&self, mw: i32, mh: i32) -> bool {
        self.x <= 0 && self.y <= 0 && self.w >= mw && self.h >= mh
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct YoutubeMeta {
    pub title: String,
    pub author: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ContentKind {
    Video,
    BrowserPage,
    App,
    Unknown,
}

#[derive(Debug, Clone)]
pub struct ScreenContext {
    pub app: String,
    pub window_title: String,
    /// Privacy-trimmed URL (scheme + host + path; YouTube keeps only the id).
    pub url: Option<String>,
    pub youtube: Option<YoutubeMeta>,
    pub kind: ContentKind,
    pub region: Region,
    /// Region is the full monitor (no crop happened).
    pub full_screen: bool,
    /// Title/URL/app matched the sensitive-window denylist (bank, password
    /// manager, wallet…): the tour refuses instead of capturing.
    pub sensitive: bool,
    /// Whether the foreground window could actually be identified.
    ///
    /// `false` means "I could not tell" — which is NOT the same as "there is
    /// nothing interesting here". A caller that captures without an identity
    /// cannot check its privacy list, so on Linux the answer is to refuse.
    pub foreground_known: bool,
    /// OCR-visible text lines (compact), injected into the vision prompt as
    /// ground-truth clues so celebrity/creator recognition can cross-reference
    /// on-screen text (Feature 88 v2 / P3).
    pub ocr_lines: Vec<String>,
}

/// What the OS probes returned, before any interpretation.
#[derive(Debug, Clone, Default)]
pub struct RawContext {
    pub app: String,
    pub title: String,
    pub url: Option<String>,
    pub document_rect: Option<(i32, i32, i32, i32)>,
    pub window_rect: Option<(i32, i32, i32, i32)>,
    /// Whether the probes actually identified the foreground window.
    pub foreground_known: bool,
}

pub const MIN_REGION_W: i32 = 200;
pub const MIN_REGION_H: i32 = 150;

// ─── Pure helpers ──────────────────────────────────────────────────────

/// 11-char YouTube video id from a watch / youtu.be / shorts / live URL. Pure.
pub fn youtube_video_id(url: &str) -> Option<String> {
    let lower = url.trim().to_lowercase();
    let no_scheme = lower
        .strip_prefix("https://")
        .or_else(|| lower.strip_prefix("http://"))
        .unwrap_or(&lower);
    let host_end = no_scheme.find(['/', '?', '#']).unwrap_or(no_scheme.len());
    let host = no_scheme[..host_end].trim_start_matches("www.").trim_start_matches("m.");
    let is_yt = host == "youtube.com" || host == "music.youtube.com";
    let is_short = host == "youtu.be";
    if !is_yt && !is_short {
        return None;
    }
    // Work on the ORIGINAL casing for the id (ids are case-sensitive).
    let orig = url.trim();
    let orig_no_scheme = orig
        .strip_prefix("https://")
        .or_else(|| orig.strip_prefix("http://"))
        .or_else(|| orig.strip_prefix("HTTPS://"))
        .or_else(|| orig.strip_prefix("HTTP://"))
        .unwrap_or(orig);
    let rest = &orig_no_scheme[host_end.min(orig_no_scheme.len())..];
    let valid = |s: &str| -> Option<String> {
        let id: String = s
            .chars()
            .take_while(|c| c.is_ascii_alphanumeric() || *c == '_' || *c == '-')
            .collect();
        if id.len() == 11 {
            Some(id)
        } else {
            None
        }
    };
    if is_short {
        return valid(rest.trim_start_matches('/'));
    }
    for prefix in ["/shorts/", "/live/", "/embed/"] {
        if let Some(p) = rest.to_lowercase().find(prefix) {
            return valid(&rest[p + prefix.len()..]);
        }
    }
    if rest.to_lowercase().starts_with("/watch") {
        let query = rest.split_once('?').map(|x| x.1).unwrap_or("");
        let query = query.split('#').next().unwrap_or("");
        for pair in query.split('&') {
            if let Some(v) = pair.strip_prefix("v=") {
                return valid(v);
            }
        }
    }
    None
}

/// Privacy-trim a page URL before it goes to a cloud model: scheme + host +
/// path only (no query string, fragment or credentials); YouTube collapses
/// to the canonical watch URL with just the video id. Pure.
pub fn sanitize_url(url: &str) -> Option<String> {
    let u = url.trim();
    if u.is_empty() {
        return None;
    }
    if let Some(id) = youtube_video_id(u) {
        return Some(format!("https://www.youtube.com/watch?v={id}"));
    }
    let no_scheme = u
        .strip_prefix("https://")
        .or_else(|| u.strip_prefix("http://"))
        .unwrap_or(u);
    let cut = no_scheme.find(['?', '#']).unwrap_or(no_scheme.len());
    let mut clean = no_scheme[..cut].to_string();
    // Drop `user:pass@`.
    if let Some(at) = clean.find('@') {
        if clean[..at].find('/').is_none() {
            clean = clean[at + 1..].to_string();
        }
    }
    if !clean.contains('.') {
        return None; // not a web address (e.g. "about:blank", "chrome://…")
    }
    let out = format!("https://{}", clean.trim_end_matches('/'));
    Some(out.chars().take(200).collect())
}

/// Parse YouTube oEmbed JSON (`{"title": …, "author_name": …}`). Pure.
pub fn parse_oembed(json: &str) -> Option<YoutubeMeta> {
    let v: serde_json::Value = serde_json::from_str(json).ok()?;
    let title = v.get("title")?.as_str()?.trim().to_string();
    if title.is_empty() {
        return None;
    }
    let author = v
        .get("author_name")
        .and_then(|a| a.as_str())
        .unwrap_or("")
        .trim()
        .to_string();
    Some(YoutubeMeta { title, author })
}

/// Pure classification of the foreground content.
pub fn classify_content(app: &str, url: Option<&str>) -> ContentKind {
    if url.map(|u| youtube_video_id(u).is_some()).unwrap_or(false) {
        return ContentKind::Video;
    }
    if crate::browser_url::is_browser_process(app) {
        return ContentKind::BrowserPage;
    }
    if app.is_empty() {
        ContentKind::Unknown
    } else {
        ContentKind::App
    }
}

/// Intersect (x,y,w,h) with the monitor; None when the result is too small
/// to be a useful crop. Pure.
pub fn clamp_region(x: i32, y: i32, w: i32, h: i32, mw: i32, mh: i32) -> Option<Region> {
    let (x0, y0) = (x.max(0), y.max(0));
    let (x1, y1) = ((x + w).min(mw), (y + h).min(mh));
    let (cw, ch) = (x1 - x0, y1 - y0);
    if cw < MIN_REGION_W || ch < MIN_REGION_H {
        None
    } else {
        Some(Region { x: x0, y: y0, w: cw, h: ch })
    }
}

/// Toolbar height guess for a browser whose Document element is not exposed
/// (tab strip + address bar + a little): ~8.5 % of the window height,
/// clamped. Fallback only. Pure.
pub fn toolbar_inset(window_h: i32) -> i32 {
    (((window_h as f32) * 0.085) as i32).clamp(80, 140)
}

/// Pick the capture region. Order: browser page `Document` rect → (browser)
/// window rect minus a toolbar inset / (other app) window rect → full
/// monitor. Anything too small or off-monitor falls through. Pure.
pub fn choose_region(raw: &RawContext, mw: i32, mh: i32) -> Region {
    let is_browser = crate::browser_url::is_browser_process(&raw.app);
    if let Some((x, y, w, h)) = raw.document_rect {
        if let Some(r) = clamp_region(x, y, w, h, mw, mh) {
            return r;
        }
    }
    if let Some((x, y, w, h)) = raw.window_rect {
        let (y2, h2) = if is_browser {
            let inset = toolbar_inset(h);
            (y + inset, h - inset)
        } else {
            (y, h)
        };
        if let Some(r) = clamp_region(x, y2, w, h2, mw, mh) {
            return r;
        }
    }
    Region::full(mw, mh)
}

/// Sensitive-window check against the live-mode denylist (banks, password
/// managers, wallets). Pure over strings.
pub fn is_sensitive(app: &str, title: &str, url: Option<&str>) -> bool {
    // On Linux the OBSERVED identity is what counts, and `policy` is the
    // module that knows which windows hold credentials or money. The
    // live-mode denylist below matches text the *model* supplied, which a
    // prompt injection controls — so it is only ever an extra layer here.
    #[cfg(target_os = "linux")]
    if crate::policy::classify_window(app, title).is_some() {
        return true;
    }
    let t = crate::live::safety::is_target_blocked;
    t(app) || t(title) || url.map(t).unwrap_or(false)
}

/// Assemble the context from raw probes (+ optional YouTube metadata). Pure.
pub fn build_context(
    raw: &RawContext,
    youtube: Option<YoutubeMeta>,
    mw: i32,
    mh: i32,
) -> ScreenContext {
    let url = raw.url.as_deref().and_then(sanitize_url);
    let region = choose_region(raw, mw, mh);
    ScreenContext {
        app: raw.app.clone(),
        window_title: raw.title.trim().to_string(),
        kind: classify_content(&raw.app, url.as_deref()),
        full_screen: region.is_full(mw, mh),
        sensitive: is_sensitive(&raw.app, &raw.title, raw.url.as_deref()),
        foreground_known: raw.foreground_known,
        youtube,
        url,
        region,
        ocr_lines: Vec::new(),
    }
}

/// Compact the WinRT OCR text into distinct non-empty lines for the prompt
/// (bounded — the prompt must stay small). Pure + unit-tested.
pub fn compact_ocr_lines(text: &str, max_lines: usize) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for line in text.lines() {
        let line = line.trim();
        if line.is_empty() || out.iter().any(|s| s == line) {
            continue;
        }
        out.push(line.chars().take(90).collect());
        if out.len() >= max_lines {
            break;
        }
    }
    out
}

impl ScreenContext {
    /// Empty context (probes failed): full screen, nothing known.
    #[allow(dead_code)]
    pub fn unknown(mw: i32, mh: i32) -> Self {
        build_context(&RawContext::default(), None, mw, mh)
    }

    /// The block inserted into the model prompt. Never contains the raw
    /// (un-trimmed) URL.
    pub fn prompt_block(&self) -> String {
        let mut lines = vec![
            "Context about what the user is looking at (from the operating system — trust it over guesses):"
                .to_string(),
        ];
        if !self.app.is_empty() {
            lines.push(format!("- App: {}", self.app));
        }
        if !self.window_title.is_empty() {
            lines.push(format!("- Window / tab title: {}", self.window_title));
        }
        if let Some(u) = &self.url {
            lines.push(format!("- Page URL: {u}"));
        }
        if let Some(y) = &self.youtube {
            lines.push(format!("- YouTube video title: {}", y.title));
            if !y.author.is_empty() {
                lines.push(format!("- YouTube channel: {}", y.author));
            }
        }
        lines.push(match self.kind {
            ContentKind::Video => "- Kind: a video page (use the title/channel to say what it is about).".to_string(),
            ContentKind::BrowserPage => "- Kind: a web page.".to_string(),
            ContentKind::App => "- Kind: a desktop application.".to_string(),
            ContentKind::Unknown => "- Kind: unknown.".to_string(),
        });
        lines.push(if self.full_screen {
            "- The image is the full screen.".to_string()
        } else {
            "- The image shows ONLY the content area (browser tabs, address bar and the taskbar are cropped out).".to_string()
        });
        // Feature 98: ground-truth text from local OCR — the model can
        // cross-reference visible names/handles when naming public figures.
        if !self.ocr_lines.is_empty() {
            lines.push("[On-Screen Context Clues from OS & OCR]:".to_string());
            for line in &self.ocr_lines {
                lines.push(format!("- \"{line}\""));
            }
        }
        lines.join("\n")
    }
}

// ─── Probes + oEmbed (thin glue) ───────────────────────────────────────

static OEMBED_CACHE: once_cell::sync::Lazy<parking_lot::Mutex<HashMap<String, YoutubeMeta>>> =
    once_cell::sync::Lazy::new(|| parking_lot::Mutex::new(HashMap::new()));

/// Blocking OS probes (UI Automation can take a moment — call from
/// `spawn_blocking`).
#[cfg(target_os = "windows")]
fn probe_blocking() -> RawContext {
    let app = crate::browser_url::foreground_process_name().unwrap_or_default();
    let title = crate::browser_url::foreground_window_title().unwrap_or_default();
    let is_browser = crate::browser_url::is_browser_process(&app);
    let url = if is_browser {
        crate::browser_url::get_active_browser_url()
    } else {
        None
    };
    let document_rect = if is_browser {
        crate::browser_url::browser_document_rect()
    } else {
        None
    };
    // The flag says whether the probes actually IDENTIFIED the foreground —
    // not merely that they ran. `foreground_process_name()` returning `None`
    // leaves the app and title empty, and claiming "known" there would make
    // a later hardening pass trust an identity that was never established.
    let foreground_known = !app.trim().is_empty() || !title.trim().is_empty();
    RawContext {
        app,
        title,
        url,
        document_rect,
        window_rect: crate::browser_url::foreground_window_rect(),
        foreground_known,
    }
}

/// Blocking OS probes on Linux: the accessibility bus.
///
/// The identity comes from the same source the policy gate uses — the
/// toolkit's own report of the focused window — so the context and the gate
/// can never disagree about which window is in front. There is no portable
/// "active browser URL" on Wayland, and the CDP tier needs the browser to be
/// launched with a debug port, so the URL is simply not available here.
#[cfg(target_os = "linux")]
fn probe_blocking() -> RawContext {
    let fg = crate::semantic::observed_foreground();
    let (app, title, known) = match &fg {
        crate::atspi::Foreground::Known { app, title } => (app.clone(), title.clone(), true),
        crate::atspi::Foreground::Unknown { .. } => (String::new(), String::new(), false),
    };
    RawContext {
        app,
        title,
        url: None,
        document_rect: None,
        window_rect: None,
        foreground_known: known,
    }
}

#[cfg(not(any(target_os = "windows", target_os = "linux")))]
fn probe_blocking() -> RawContext {
    RawContext::default()
}

async fn fetch_oembed(video_id: &str) -> Option<YoutubeMeta> {
    if let Some(hit) = OEMBED_CACHE.lock().get(video_id).cloned() {
        return Some(hit);
    }
    let watch = format!("https://www.youtube.com/watch?v={video_id}");
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(3))
        .build()
        .ok()?;
    let resp = client
        .get("https://www.youtube.com/oembed")
        .query(&[("url", watch.as_str()), ("format", "json")])
        .send()
        .await
        .ok()?;
    if !resp.status().is_success() {
        return None;
    }
    let meta = parse_oembed(&resp.text().await.ok()?)?;
    let mut cache = OEMBED_CACHE.lock();
    if cache.len() > 64 {
        cache.clear();
    }
    cache.insert(video_id.to_string(), meta.clone());
    Some(meta)
}

/// Gather the context for the tour. Never fails: a slow/failed probe or
/// lookup just yields less context (and a full-screen capture).
///
/// **But "less context" is not "no identity".** A probe that times out
/// leaves `foreground_known` false, and the caller must treat that as a
/// refusal rather than as "nothing interesting here" — an unidentified
/// window cannot be checked against the privacy list. On Windows the capture
/// path still fails open (see the Windows note in the port handover); on
/// Linux the capture primitive itself refuses, so the whole chain is closed.
pub async fn collect(mw: i32, mh: i32) -> ScreenContext {
    let raw = match tokio::time::timeout(
        std::time::Duration::from_millis(3500),
        tokio::task::spawn_blocking(probe_blocking),
    )
    .await
    {
        Ok(Ok(raw)) => raw,
        _ => {
            tracing::warn!(
                "screen_context: probes timed out — the foreground window is unknown"
            );
            RawContext::default()
        }
    };
    // Sensitive windows never leave the machine: skip the lookup entirely.
    let sensitive = is_sensitive(&raw.app, &raw.title, raw.url.as_deref());
    let youtube = if sensitive {
        None
    } else if let Some(id) = raw.url.as_deref().and_then(youtube_video_id) {
        fetch_oembed(&id).await
    } else {
        None
    };
    let ctx = build_context(&raw, youtube, mw, mh);
    // Feature 98 P3: ground-truth text clues from local WinRT OCR (fast,
    // free, ~25ms) so the vision model can cross-reference on-screen text
    // (names, handles, chyrons) for public-figure recognition.
    #[cfg(target_os = "windows")]
    let ocr_lines = {
        let ocr = tokio::time::timeout(
            std::time::Duration::from_millis(1200),
            tokio::task::spawn_blocking(crate::ocr::capture_screen_text),
        )
        .await
        .ok()
        .and_then(|r| r.ok())
        .and_then(|r| r);
        match ocr {
            Some((text, _)) => compact_ocr_lines(&text, 10),
            None => Vec::new(),
        }
    };
    #[cfg(not(target_os = "windows"))]
    let ocr_lines = Vec::new();
    let ctx = ScreenContext { ocr_lines, ..ctx };
    tracing::info!(
        "screen_context: app='{}' kind={:?} region={:?} full={} youtube={} sensitive={} foreground_known={}",
        ctx.app,
        ctx.kind,
        ctx.region,
        ctx.full_screen,
        ctx.youtube.is_some(),
        ctx.sensitive,
        ctx.foreground_known
    );
    ctx
}

#[cfg(test)]
mod tests {
    use super::*;

    const ID: &str = "dQw4w9WgXcQ";

    #[test]
    fn youtube_ids_from_every_url_shape() {
        for u in [
            format!("https://www.youtube.com/watch?v={ID}"),
            format!("https://youtube.com/watch?feature=share&v={ID}&t=42s"),
            format!("https://m.youtube.com/watch?v={ID}#frag"),
            format!("youtube.com/watch?v={ID}"),
            format!("https://youtu.be/{ID}?si=abc"),
            format!("https://www.youtube.com/shorts/{ID}"),
            format!("https://www.youtube.com/live/{ID}?feature=share"),
            format!("https://music.youtube.com/watch?v={ID}&list=PL1"),
        ] {
            assert_eq!(youtube_video_id(&u).as_deref(), Some(ID), "url: {u}");
        }
    }

    #[test]
    fn youtube_id_rejects_non_video_urls() {
        for u in [
            "https://www.youtube.com/",
            "https://www.youtube.com/feed/subscriptions",
            "https://www.youtube.com/watch?v=short",
            "https://example.com/watch?v=dQw4w9WgXcQ",
            "https://notyoutube.com/watch?v=dQw4w9WgXcQ",
            "",
        ] {
            assert_eq!(youtube_video_id(u), None, "url: {u}");
        }
    }

    #[test]
    fn sanitize_drops_query_fragment_and_credentials() {
        assert_eq!(
            sanitize_url("https://www.womenshealthmag.com/uk/food/healthy-eating/g63752621/healthiest-nuts/?utm_source=x&token=abc#top").as_deref(),
            Some("https://www.womenshealthmag.com/uk/food/healthy-eating/g63752621/healthiest-nuts")
        );
        assert_eq!(
            sanitize_url("womenshealthmag.com/uk/food/healthiest-nuts/").as_deref(),
            Some("https://womenshealthmag.com/uk/food/healthiest-nuts")
        );
        assert_eq!(
            sanitize_url("https://user:pw@example.com/a?b=1").as_deref(),
            Some("https://example.com/a")
        );
        // YouTube collapses to the canonical id-only URL.
        assert_eq!(
            sanitize_url(&format!("https://www.youtube.com/watch?v={ID}&list=PLsecret&t=9")).as_deref(),
            Some(format!("https://www.youtube.com/watch?v={ID}").as_str())
        );
        assert_eq!(sanitize_url(""), None);
        assert_eq!(sanitize_url("about:blank"), None);
        assert_eq!(sanitize_url("chrome://settings"), None);
    }

    #[test]
    fn oembed_parsing() {
        let m = parse_oembed(r#"{"title":"Never Gonna Give You Up","author_name":"Rick Astley","type":"video"}"#).unwrap();
        assert_eq!(m.title, "Never Gonna Give You Up");
        assert_eq!(m.author, "Rick Astley");
        assert_eq!(parse_oembed(r#"{"title":"T"}"#).unwrap().author, "");
        assert!(parse_oembed(r#"{"title":"  "}"#).is_none());
        assert!(parse_oembed("not json").is_none());
        assert!(parse_oembed(r#"{"author_name":"x"}"#).is_none());
    }

    #[test]
    fn classification() {
        assert_eq!(
            classify_content("brave.exe", Some(&format!("https://www.youtube.com/watch?v={ID}"))),
            ContentKind::Video
        );
        assert_eq!(classify_content("brave.exe", Some("https://example.com/a")), ContentKind::BrowserPage);
        assert_eq!(classify_content("brave.exe", None), ContentKind::BrowserPage);
        assert_eq!(classify_content("code.exe", None), ContentKind::App);
        assert_eq!(classify_content("", None), ContentKind::Unknown);
    }

    #[test]
    fn clamp_region_intersects_and_rejects_tiny() {
        // Maximised window overhanging the monitor by the 8-px frame.
        assert_eq!(
            clamp_region(-8, -8, 1936, 1096, 1920, 1080),
            Some(Region { x: 0, y: 0, w: 1920, h: 1080 })
        );
        assert_eq!(
            clamp_region(0, 82, 1920, 950, 1920, 1080),
            Some(Region { x: 0, y: 82, w: 1920, h: 950 })
        );
        assert_eq!(clamp_region(0, 0, 100, 100, 1920, 1080), None); // too small
        assert_eq!(clamp_region(2000, 0, 500, 500, 1920, 1080), None); // off monitor
        assert_eq!(clamp_region(1800, 900, 500, 500, 1920, 1080), None); // sliver
    }

    #[test]
    fn region_prefers_document_rect() {
        let raw = RawContext {
            app: "brave.exe".into(),
            document_rect: Some((0, 82, 1920, 950)),
            window_rect: Some((0, 0, 1920, 1032)),
            ..Default::default()
        };
        assert_eq!(choose_region(&raw, 1920, 1080), Region { x: 0, y: 82, w: 1920, h: 950 });
    }

    #[test]
    fn browser_without_document_rect_uses_inset_window() {
        let raw = RawContext {
            app: "chrome.exe".into(),
            window_rect: Some((0, 0, 1920, 1040)),
            ..Default::default()
        };
        let r = choose_region(&raw, 1920, 1080);
        assert_eq!(r.y, toolbar_inset(1040));
        assert_eq!(r.y + r.h, 1040);
        assert!(r.y >= 80 && r.y <= 140);
    }

    #[test]
    fn non_browser_uses_window_rect_and_unknown_uses_full() {
        let raw = RawContext {
            app: "code.exe".into(),
            window_rect: Some((100, 50, 1200, 800)),
            ..Default::default()
        };
        assert_eq!(choose_region(&raw, 1920, 1080), Region { x: 100, y: 50, w: 1200, h: 800 });
        let none = RawContext::default();
        assert_eq!(choose_region(&none, 1920, 1080), Region::full(1920, 1080));
        // Bad probe data (tiny rect) also falls back to full screen.
        let tiny = RawContext {
            app: "code.exe".into(),
            window_rect: Some((0, 0, 50, 50)),
            ..Default::default()
        };
        assert_eq!(choose_region(&tiny, 1920, 1080), Region::full(1920, 1080));
    }

    #[test]
    fn toolbar_inset_scales_and_clamps() {
        assert_eq!(toolbar_inset(400), 80);
        assert_eq!(toolbar_inset(1000), 85);
        assert_eq!(toolbar_inset(4000), 140);
    }

    #[test]
    fn sensitive_windows_are_flagged() {
        assert!(is_sensitive("brave.exe", "Chase Bank — Sign in", None));
        assert!(is_sensitive("1password.exe", "", None));
        assert!(is_sensitive("brave.exe", "x", Some("https://www.paypal.com/myaccount")));
        assert!(!is_sensitive("brave.exe", "Top 10 healthiest nuts | Women's Health", Some("https://www.womenshealthmag.com/uk/food")));
    }

    #[test]
    fn context_assembly_for_the_nuts_page() {
        let raw = RawContext {
            app: "brave.exe".into(),
            title: "Top 10 healthiest nuts | Women's Health".into(),
            url: Some("womenshealthmag.com/uk/food/healthy-eating/g63752621/healthiest-nuts/".into()),
            document_rect: Some((0, 82, 1920, 950)),
            window_rect: Some((0, 0, 1920, 1032)),
            foreground_known: true,
        };
        let ctx = build_context(&raw, None, 1920, 1080);
        assert_eq!(ctx.kind, ContentKind::BrowserPage);
        assert!(!ctx.full_screen && !ctx.sensitive);
        assert!(ctx.foreground_known);
        assert_eq!(ctx.region, Region { x: 0, y: 82, w: 1920, h: 950 });
        let block = ctx.prompt_block();
        assert!(block.contains("Top 10 healthiest nuts"));
        assert!(block.contains("https://womenshealthmag.com/uk/food/healthy-eating/g63752621/healthiest-nuts"));
        assert!(block.contains("cropped out"));
    }

    #[test]
    fn context_for_a_video_includes_title_and_channel_but_not_tracking() {
        let raw = RawContext {
            app: "brave.exe".into(),
            title: "Some Video - YouTube".into(),
            url: Some(format!("https://www.youtube.com/watch?v={ID}&list=PLsecret&si=trk")),
            foreground_known: true,
            ..Default::default()
        };
        let meta = YoutubeMeta { title: "Never Gonna Give You Up".into(), author: "Rick Astley".into() };
        let ctx = build_context(&raw, Some(meta), 1920, 1080);
        assert_eq!(ctx.kind, ContentKind::Video);
        let block = ctx.prompt_block();
        assert!(block.contains("YouTube video title: Never Gonna Give You Up"));
        assert!(block.contains("YouTube channel: Rick Astley"));
        assert!(!block.contains("PLsecret") && !block.contains("trk"));
        assert!(block.contains("full screen")); // no rects → full capture
    }

    #[test]
    fn unknown_context_is_full_screen_and_harmless() {
        let ctx = ScreenContext::unknown(1920, 1080);
        assert!(ctx.full_screen && !ctx.sensitive);
        assert_eq!(ctx.kind, ContentKind::Unknown);
        assert!(ctx.prompt_block().contains("unknown"));
    }

    #[test]
    fn an_unidentified_foreground_is_flagged_as_unknown() {
        // The distinction the whole capture gate rests on: "I could not tell
        // which window this is" is not "nothing to redact". A probe that
        // timed out leaves the identity unknown, and the caller must be able
        // to see that.
        let raw = RawContext::default();
        let ctx = build_context(&raw, None, 1920, 1080);
        assert!(!ctx.foreground_known);
        assert!(ctx.app.is_empty());
    }

    #[test]
    fn an_identified_foreground_is_flagged_as_known() {
        let raw = RawContext {
            app: "code".into(),
            title: "main.rs".into(),
            foreground_known: true,
            ..Default::default()
        };
        let ctx = build_context(&raw, None, 1920, 1080);
        assert!(ctx.foreground_known);
    }
}

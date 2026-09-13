//! Browser navigation commands — new tab, navigate to URL, search.
//!
//! Uses keyboard shortcuts (Ctrl+T, Ctrl+L) to control the browser without
//! needing UI Automation or Playwright. This works with any browser
//! (Brave, Chrome, Edge, Firefox) because these shortcuts are universal.
//!
//! For searches, reuses the existing open_search() from command_executor
//! which opens Google in the default browser.

use std::thread;
use std::time::Duration;

use super::keyboard;

/// Open a new browser tab (Ctrl+T).
/// Assumes a browser is currently focused.
pub fn new_tab() -> Result<(), String> {
    tracing::info!("live: browser new tab");
    keyboard::press_hotkey(&["ctrl", "t"])?;
    thread::sleep(Duration::from_millis(300));
    Ok(())
}

/// Navigate to a URL in the current browser tab (Ctrl+L → type URL → Enter).
/// Assumes a browser is currently focused.
pub fn navigate(url: &str) -> Result<(), String> {
    tracing::info!("live: browser navigate to: {url}");

    // Ctrl+L focuses the address bar
    keyboard::press_hotkey(&["ctrl", "l"])?;
    thread::sleep(Duration::from_millis(200));

    // Type the URL
    keyboard::type_text(url)?;
    thread::sleep(Duration::from_millis(200));

    // Press Enter to navigate
    keyboard::press_key("enter")?;
    Ok(())
}

/// Search in the current browser tab (Ctrl+L → type query → Enter).
/// This uses the browser's default search engine.
pub fn search(query: &str) -> Result<(), String> {
    tracing::info!("live: browser search: {query}");

    // Ctrl+L focuses the address bar (doubles as search bar in most browsers)
    keyboard::press_hotkey(&["ctrl", "l"])?;
    thread::sleep(Duration::from_millis(200));

    // Type the search query
    keyboard::type_text(query)?;
    thread::sleep(Duration::from_millis(200));

    // Press Enter to search
    keyboard::press_key("enter")?;
    Ok(())
}

/// Open a specific website by name (e.g., "wikipedia", "github").
/// Maps common site names to their URLs.
pub fn open_site(site: &str) -> Result<(), String> {
    let url = resolve_site_url(site);
    tracing::info!("live: opening site '{}' → {url}", site);

    // Use the existing open::that to open in default browser
    open::that(&url).map_err(|e| format!("open site: {e}"))?;
    thread::sleep(Duration::from_millis(1000));
    Ok(())
}

/// Map a site name to its URL.
fn resolve_site_url(site: &str) -> String {
    let lower = site.to_lowercase();
    match lower.as_str() {
        "wikipedia" | "wiki" => "https://www.wikipedia.org".to_string(),
        "github" => "https://github.com".to_string(),
        "google" => "https://www.google.com".to_string(),
        "youtube" => "https://www.youtube.com".to_string(),
        "gmail" => "https://mail.google.com".to_string(),
        "twitter" | "x" => "https://twitter.com".to_string(),
        "reddit" => "https://www.reddit.com".to_string(),
        "stackoverflow" => "https://stackoverflow.com".to_string(),
        "linkedin" => "https://www.linkedin.com".to_string(),
        "amazon" => "https://www.amazon.com".to_string(),
        "netflix" => "https://www.netflix.com".to_string(),
        "spotify" => "https://open.spotify.com".to_string(),
        _ => {
            // If it looks like a URL, use it directly
            if lower.starts_with("http://") || lower.starts_with("https://") {
                lower
            } else if lower.contains('.') {
                format!("https://{lower}")
            } else {
                format!("https://www.{lower}.com")
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_resolve_site_url() {
        assert_eq!(resolve_site_url("wikipedia"), "https://www.wikipedia.org");
        assert_eq!(resolve_site_url("wiki"), "https://www.wikipedia.org");
        assert_eq!(resolve_site_url("github"), "https://github.com");
        assert_eq!(resolve_site_url("google"), "https://www.google.com");
        assert_eq!(
            resolve_site_url("youtube"),
            "https://www.youtube.com"
        );
    }

    #[test]
    fn test_resolve_site_url_direct() {
        assert_eq!(
            resolve_site_url("https://example.com"),
            "https://example.com"
        );
        assert_eq!(
            resolve_site_url("example.com"),
            "https://example.com"
        );
    }

    #[test]
    fn test_resolve_site_url_unknown() {
        assert_eq!(
            resolve_site_url("mycustomsite"),
            "https://www.mycustomsite.com"
        );
    }
}

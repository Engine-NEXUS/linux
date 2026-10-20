//! Linux screen queries: primary monitor size and actionable UI elements.

use super::UiElement;

/// Parse a mode string like "1920x1080" into (w, h). Pure, unit-tested.
pub fn parse_mode_line(line: &str) -> Option<(i32, i32)> {
    let mut parts = line.trim().split('x');
    let w = parts.next()?.parse::<i32>().ok()?;
    let h = parts.next()?.parse::<i32>().ok()?;
    if w > 0 && h > 0 {
        Some((w, h))
    } else {
        None
    }
}

/// Primary monitor size in physical px on Linux.
/// Queries connected DRM connectors, falling back to 1920x1080.
pub fn primary_monitor_size() -> Option<(i32, i32)> {
    if let Ok(entries) = std::fs::read_dir("/sys/class/drm") {
        for entry in entries.flatten() {
            let path = entry.path();
            let status_file = path.join("status");
            if let Ok(status) = std::fs::read_to_string(&status_file) {
                if status.trim() == "connected" {
                    let modes_file = path.join("modes");
                    if let Ok(modes) = std::fs::read_to_string(&modes_file) {
                        if let Some(first_line) = modes.lines().next() {
                            if let Some((w, h)) = parse_mode_line(first_line) {
                                return Some((w, h));
                            }
                        }
                    }
                }
            }
        }
    }
    Some((1920, 1080))
}

/// List actionable elements in the foreground window on Linux.
/// Returns an empty list until the AT-SPI accessibility tree tier is wired.
pub fn list_actionables() -> Vec<UiElement> {
    Vec::new()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_mode_line() {
        assert_eq!(parse_mode_line("1920x1080"), Some((1920, 1080)));
        assert_eq!(parse_mode_line("2560x1440"), Some((2560, 1440)));
        assert_eq!(parse_mode_line("3840x2160\n"), Some((3840, 2160)));
        assert_eq!(parse_mode_line("invalid"), None);
        assert_eq!(parse_mode_line("0x0"), None);
    }

    #[test]
    fn test_primary_monitor_size_fallback() {
        let size = primary_monitor_size();
        assert!(size.is_some());
        let (w, h) = size.unwrap();
        assert!(w > 0 && h > 0);
    }
}

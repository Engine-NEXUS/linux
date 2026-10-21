//! Screen background luminance probe (Windows GDI).
//!
//! Samples an 8x8 grid of physical pixels under a window or widget rectangle
//! from the desktop device context (`GetDC(HWND(0))`), computes the
//! ITU-R BT.709 relative luminance ($Y = 0.2126R + 0.7152G + 0.0722B$),
//! and applies a 25-point hysteresis filter ($Y_{light} > 140$, $Y_{dark} < 115$)
//! so liquid glass elements seamlessly adapt between light frosted glass and
//! obsidian frosted glass without boundary flickering.
//!
//! Because our windows have `WDA_EXCLUDEFROMCAPTURE` (17) applied, `GetDC(HWND(0))`
//! reads the desktop and windows BEHIND our transparent window, giving the true
//! underlying background luminance.

use serde::{Deserialize, Serialize};

/// Luminance adaptation mode.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum LuminanceMode {
    Light,
    Dark,
}

impl LuminanceMode {
    #[allow(dead_code)]
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Light => "light",
            Self::Dark => "dark",
        }
    }
}

/// Result returned to frontend / callers.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LuminanceResult {
    pub mode: LuminanceMode,
    pub luminance: f64,
    pub x: i32,
    pub y: i32,
    pub w: i32,
    pub h: i32,
}

/// Computes ITU-R BT.709 relative luminance from 8-bit RGB channels.
/// Formula: Y = 0.2126 * R + 0.7152 * G + 0.0722 * B.
/// Returns a value in [0.0, 255.0].
#[inline]
pub fn calculate_relative_luminance(r: u8, g: u8, b: u8) -> f64 {
    0.2126 * (r as f64) + 0.7152 * (g as f64) + 0.0722 * (b as f64)
}

/// Hysteresis state machine to prevent rapid theme toggling on edge colors.
/// - Dark -> Light requires Y > 140.0
/// - Light -> Dark requires Y < 115.0
/// - Values between [115.0, 140.0] maintain previous state.
#[derive(Debug, Clone, Copy)]
pub struct HysteresisFilter {
    current_mode: LuminanceMode,
    threshold_high: f64,
    threshold_low: f64,
}

impl Default for HysteresisFilter {
    fn default() -> Self {
        Self {
            current_mode: LuminanceMode::Dark,
            threshold_high: 140.0,
            threshold_low: 115.0,
        }
    }
}

impl HysteresisFilter {
    #[allow(dead_code)]
    pub fn new(initial: LuminanceMode, threshold_low: f64, threshold_high: f64) -> Self {
        Self {
            current_mode: initial,
            threshold_high,
            threshold_low,
        }
    }

    pub fn update(&mut self, y: f64) -> LuminanceMode {
        match self.current_mode {
            LuminanceMode::Dark => {
                if y > self.threshold_high {
                    self.current_mode = LuminanceMode::Light;
                }
            }
            LuminanceMode::Light => {
                if y < self.threshold_low {
                    self.current_mode = LuminanceMode::Dark;
                }
            }
        }
        self.current_mode
    }

    #[allow(dead_code)]
    pub fn mode(&self) -> LuminanceMode {
        self.current_mode
    }
}

#[cfg(target_os = "windows")]
pub fn sample_region_luminance(x: i32, y: i32, w: i32, h: i32) -> Option<f64> {
    if w <= 0 || h <= 0 {
        return None;
    }

    use windows::Win32::Foundation::HWND;
    use windows::Win32::Graphics::Gdi::{GetDC, GetPixel, ReleaseDC};

    unsafe {
        let hdc = GetDC(HWND(0));
        if hdc.0 == 0 {
            return None;
        }

        // Sample an 8x8 grid (64 sample points)
        const GRID_SIZE: i32 = 8;
        let mut total_lum = 0.0;
        let mut valid_samples = 0;

        let step_x = (w / (GRID_SIZE + 1)).max(1);
        let step_y = (h / (GRID_SIZE + 1)).max(1);

        for gy in 1..=GRID_SIZE {
            let py = y + gy * step_y;
            for gx in 1..=GRID_SIZE {
                let px = x + gx * step_x;
                let colorref = GetPixel(hdc, px, py);
                // 0xFFFFFFFF means CLR_INVALID
                if colorref != 0xFFFFFFFF {
                    let r = (colorref & 0x000000FF) as u8;
                    let g = ((colorref & 0x0000FF00) >> 8) as u8;
                    let b = ((colorref & 0x00FF0000) >> 16) as u8;
                    total_lum += calculate_relative_luminance(r, g, b);
                    valid_samples += 1;
                }
            }
        }

        ReleaseDC(HWND(0), hdc);

        if valid_samples > 0 {
            Some(total_lum / (valid_samples as f64))
        } else {
            None
        }
    }
}

#[cfg(not(target_os = "windows"))]
pub fn sample_region_luminance(_x: i32, _y: i32, _w: i32, _h: i32) -> Option<f64> {
    None
}

/// Global filter state for smooth sampling across intervals.
static GLOBAL_FILTER: once_cell::sync::Lazy<parking_lot::Mutex<HysteresisFilter>> =
    once_cell::sync::Lazy::new(|| parking_lot::Mutex::new(HysteresisFilter::default()));

/// Probe the desktop behind the given rect and return the adaptive luminance mode.
pub fn probe_screen_luminance(x: i32, y: i32, w: i32, h: i32) -> LuminanceResult {
    let lum = sample_region_luminance(x, y, w, h).unwrap_or(30.0);
    let mut filter = GLOBAL_FILTER.lock();
    let mode = filter.update(lum);
    LuminanceResult {
        mode,
        luminance: lum,
        x,
        y,
        w,
        h,
    }
}

/// IPC command: returns the probed luminance mode and average value for a rect.
#[tauri::command]
pub fn get_screen_luminance(x: i32, y: i32, w: i32, h: i32) -> Result<LuminanceResult, String> {
    Ok(probe_screen_luminance(x, y, w, h))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_relative_luminance_pure_colors() {
        // Pure black
        let black = calculate_relative_luminance(0, 0, 0);
        assert!((black - 0.0).abs() < 1e-4);

        // Pure white
        let white = calculate_relative_luminance(255, 255, 255);
        assert!((white - 255.0).abs() < 1e-4);

        // Pure red
        let red = calculate_relative_luminance(255, 0, 0);
        assert!((red - 54.213).abs() < 1e-3);

        // Pure green
        let green = calculate_relative_luminance(0, 255, 0);
        assert!((green - 182.376).abs() < 1e-3);

        // Pure blue
        let blue = calculate_relative_luminance(0, 0, 255);
        assert!((blue - 18.411).abs() < 1e-3);
    }

    #[test]
    fn test_hysteresis_filter_transitions() {
        let mut filter = HysteresisFilter::new(LuminanceMode::Dark, 115.0, 140.0);

        // Initial state is Dark
        assert_eq!(filter.mode(), LuminanceMode::Dark);

        // Value in dead-band (e.g. 125.0) remains Dark
        assert_eq!(filter.update(125.0), LuminanceMode::Dark);

        // Value crossing high threshold (145.0) switches to Light
        assert_eq!(filter.update(145.0), LuminanceMode::Light);

        // Value dropping into dead-band (130.0) remains Light
        assert_eq!(filter.update(130.0), LuminanceMode::Light);

        // Value dropping further into dead-band (120.0) still remains Light
        assert_eq!(filter.update(120.0), LuminanceMode::Light);

        // Value dropping below low threshold (110.0) switches to Dark
        assert_eq!(filter.update(110.0), LuminanceMode::Dark);

        // Value rising into dead-band (120.0) remains Dark
        assert_eq!(filter.update(120.0), LuminanceMode::Dark);
    }

    #[test]
    fn test_probe_screen_luminance_fallback() {
        // Safe fallback for negative or empty dimensions
        let res = probe_screen_luminance(0, 0, -10, -10);
        assert_eq!(res.w, -10);
        assert_eq!(res.h, -10);
        assert!(res.luminance >= 0.0);
    }
}

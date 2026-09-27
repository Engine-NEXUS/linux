//! "Fake blur" backdrop for the sidebar window (Windows only).
//!
//! Native DWM Acrylic/Mica cannot render translucently on our sidebar
//! because it's a non-activating window (see the detailed comment in
//! `lib.rs`'s setup hook and AGENTS.md) — DWM only shows the live blurred
//! material for the OS-active/foreground window, and this window is
//! deliberately never that.
//!
//! Instead, right before the window becomes visible, we capture the
//! screen region it's about to cover (via a plain GDI `BitBlt` — no
//! capture indicator, no permission prompt, works since Windows 95),
//! blur that snapshot in-process, and hand it to the frontend as a CSS
//! background image. This gives a genuine frosted-glass look without
//! depending on window activation state at all.
//!
//! Trade-off: it's a snapshot, not a live reactive blur — acceptable
//! here since the sidebar is shown once per response and the desktop
//! behind it rarely changes while it's up.

use std::ffi::c_void;

use image::{imageops::fast_blur, DynamicImage, ImageBuffer, Rgba};

use windows::Win32::Foundation::HWND;
use windows::Win32::Graphics::Gdi::{
    BitBlt, CreateCompatibleDC, CreateDIBSection, DeleteDC, DeleteObject, GetDC, ReleaseDC,
    SelectObject, BITMAPINFO, BITMAPINFOHEADER, BI_RGB, CAPTUREBLT, DIB_RGB_COLORS, ROP_CODE,
    SRCCOPY,
};

/// Captures the desktop rectangle at (x, y, w, h) — physical pixels —
/// and returns it as top-down BGRA bytes. Must be called while the
/// target window is NOT yet visible, so it doesn't capture itself.
unsafe fn capture_region_bgra(x: i32, y: i32, w: i32, h: i32) -> Option<Vec<u8>> {
    if w <= 0 || h <= 0 {
        return None;
    }

    let hdc_screen = GetDC(HWND(0));
    if hdc_screen.0 == 0 {
        return None;
    }
    let hdc_mem = CreateCompatibleDC(hdc_screen);
    if hdc_mem.0 == 0 {
        ReleaseDC(HWND(0), hdc_screen);
        return None;
    }

    let mut bmi: BITMAPINFO = std::mem::zeroed();
    bmi.bmiHeader.biSize = std::mem::size_of::<BITMAPINFOHEADER>() as u32;
    bmi.bmiHeader.biWidth = w;
    bmi.bmiHeader.biHeight = -h; // negative = top-down DIB
    bmi.bmiHeader.biPlanes = 1;
    bmi.bmiHeader.biBitCount = 32;
    bmi.bmiHeader.biCompression = BI_RGB as u32;

    let mut bits: *mut c_void = std::ptr::null_mut();
    let hbmp = match CreateDIBSection(hdc_mem, &bmi, DIB_RGB_COLORS, &mut bits, None, 0) {
        Ok(h) if !h.is_invalid() => h,
        _ => {
            DeleteDC(hdc_mem);
            ReleaseDC(HWND(0), hdc_screen);
            return None;
        }
    };

    let prev = SelectObject(hdc_mem, hbmp);

    let rop = ROP_CODE(SRCCOPY.0 | CAPTUREBLT.0);
    let ok = BitBlt(hdc_mem, 0, 0, w, h, hdc_screen, x, y, rop).as_bool();

    let result = if ok && !bits.is_null() {
        let len = (w as usize) * (h as usize) * 4;
        Some(std::slice::from_raw_parts(bits as *const u8, len).to_vec())
    } else {
        None
    };

    SelectObject(hdc_mem, prev);
    let _ = DeleteObject(hbmp);
    let _ = DeleteDC(hdc_mem);
    ReleaseDC(HWND(0), hdc_screen);

    result
}

/// BGRA (from BitBlt) -> RGBA (for the `image` crate).
fn bgra_to_rgba(bgra: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(bgra.len());
    for px in bgra.chunks_exact(4) {
        out.push(px[2]); // R
        out.push(px[1]); // G
        out.push(px[0]); // B
        out.push(255); // A — BitBlt doesn't fill alpha; force opaque
    }
    out
}

/// Captures the region behind the sidebar, blurs it, and returns a
/// `data:image/png;base64,...` URI ready to drop into a CSS
/// `background-image`. Returns `None` on any failure (caller should
/// fall back to the plain semi-transparent CSS look — never block
/// showing the sidebar on this).
pub fn capture_and_blur(x: i32, y: i32, w: i32, h: i32, sigma: f32) -> Option<String> {
    let bgra = unsafe { capture_region_bgra(x, y, w, h) }?;
    let rgba = bgra_to_rgba(&bgra);

    let img: ImageBuffer<Rgba<u8>, Vec<u8>> = ImageBuffer::from_raw(w as u32, h as u32, rgba)?;
    let blurred = fast_blur(&img, sigma);

    let mut png_bytes: Vec<u8> = Vec::new();
    DynamicImage::ImageRgba8(blurred)
        .write_to(&mut std::io::Cursor::new(&mut png_bytes), image::ImageFormat::Png)
        .ok()?;

    use base64::Engine;
    let b64 = base64::engine::general_purpose::STANDARD.encode(&png_bytes);
    Some(format!("data:image/png;base64,{b64}"))
}

/// Same as capture_and_blur but uses JPEG encoding for much faster delivery.
/// Used for both the initial backdrop capture and the live-blur loop.
pub fn capture_and_blur_jpeg(x: i32, y: i32, w: i32, h: i32, sigma: f32) -> Option<String> {
    let bgra = unsafe { capture_region_bgra(x, y, w, h) }?;
    let rgba = bgra_to_rgba(&bgra);

    let img: ImageBuffer<Rgba<u8>, Vec<u8>> = ImageBuffer::from_raw(w as u32, h as u32, rgba)?;
    let blurred = fast_blur(&img, sigma);

    let mut jpeg_bytes: Vec<u8> = Vec::new();
    // Fast JPEG encoding, lower quality is fine for blurred background
    let mut encoder = image::codecs::jpeg::JpegEncoder::new_with_quality(&mut jpeg_bytes, 60);
    encoder.encode_image(&DynamicImage::ImageRgba8(blurred)).ok()?;

    use base64::Engine;
    let b64 = base64::engine::general_purpose::STANDARD.encode(&jpeg_bytes);
    Some(format!("data:image/jpeg;base64,{b64}"))
}

/// Live-loop frame interval: 4 FPS. At 1 FPS the backdrop felt like a
/// static picture; at 4 FPS with half-res frames + CSS crossfade the motion
/// behind the glass reads as live. Cost per changed frame is ~15-25ms
/// (downscaled blur); idle ticks cost ~1ms (hash only).
pub const LIVE_BLUR_INTERVAL_MS: u64 = 250;

/// Downscale factor for live frames. The backdrop is heavily blurred anyway,
/// so half resolution is visually identical at ~1/4 the blur + JPEG cost.
pub const LIVE_BLUR_DOWNSCALE: f32 = 0.5;

/// Blurs an already-captured BGRA buffer at reduced resolution and encodes
/// it as a JPEG data URI. Same look as `blur_bgra_to_jpeg` (the image is
/// pure blur, CSS `background-size: cover` upscales it) at ~1/4 the cost —
/// this is what makes 4 FPS affordable.
pub fn blur_bgra_to_jpeg_fast(bgra: &[u8], w: i32, h: i32, sigma: f32) -> Option<String> {
    let rgba = bgra_to_rgba(bgra);
    let img: ImageBuffer<Rgba<u8>, Vec<u8>> = ImageBuffer::from_raw(w as u32, h as u32, rgba)?;
    let sw = ((w as f32 * LIVE_BLUR_DOWNSCALE).max(1.0)) as u32;
    let sh = ((h as f32 * LIVE_BLUR_DOWNSCALE).max(1.0)) as u32;
    let small = image::imageops::resize(&img, sw, sh, image::imageops::FilterType::Triangle);
    let blurred = fast_blur(&small, sigma * LIVE_BLUR_DOWNSCALE);

    let mut jpeg_bytes: Vec<u8> = Vec::new();
    let mut encoder = image::codecs::jpeg::JpegEncoder::new_with_quality(&mut jpeg_bytes, 55);
    encoder.encode_image(&DynamicImage::ImageRgba8(blurred)).ok()?;

    use base64::Engine;
    let b64 = base64::engine::general_purpose::STANDARD.encode(&jpeg_bytes);
    Some(format!("data:image/jpeg;base64,{b64}"))
}

/// Lightweight hash of a captured frame for change detection.
///
/// Samples every 16th byte of the BGRA buffer and accumulates a rolling
/// hash. This is NOT cryptographic — it's a cheap fingerprint that lets
/// the live loop skip frames where the background hasn't visibly changed
/// (e.g. nothing moved behind the sidebar), saving the expensive
/// blur→JPEG→base64→event→repaint pipeline.
///
/// Cost: ~1ms for a 600×1000 capture (vs ~50-100ms for the full pipeline).
pub fn frame_hash(bgra: &[u8]) -> u64 {
    let mut hash: u64 = 5381;
    for &byte in bgra.iter().step_by(16) {
        hash = hash.wrapping_mul(33).wrapping_add(byte as u64);
    }
    hash
}

/// Captures the raw BGRA region (without blur/encode) so the caller can
/// hash it for change detection before committing to the expensive pipeline.
/// Returns `None` on capture failure.
pub fn capture_region_bgra_public(x: i32, y: i32, w: i32, h: i32) -> Option<Vec<u8>> {
    unsafe { capture_region_bgra(x, y, w, h) }
}

/// Blurs an already-captured BGRA buffer and encodes it as a JPEG data URI.
/// This avoids re-capturing the screen when the caller already has the raw
/// bytes (e.g. after hashing them for change detection).
/// Kept as the full-resolution fallback; live loops use `blur_bgra_to_jpeg_fast`.
#[allow(dead_code)]
pub fn blur_bgra_to_jpeg(bgra: &[u8], w: i32, h: i32, sigma: f32) -> Option<String> {
    let rgba = bgra_to_rgba(bgra);
    let img: ImageBuffer<Rgba<u8>, Vec<u8>> = ImageBuffer::from_raw(w as u32, h as u32, rgba)?;
    let blurred = fast_blur(&img, sigma);

    let mut jpeg_bytes: Vec<u8> = Vec::new();
    let mut encoder = image::codecs::jpeg::JpegEncoder::new_with_quality(&mut jpeg_bytes, 60);
    encoder.encode_image(&DynamicImage::ImageRgba8(blurred)).ok()?;

    use base64::Engine;
    let b64 = base64::engine::general_purpose::STANDARD.encode(&jpeg_bytes);
    Some(format!("data:image/jpeg;base64,{b64}"))
}

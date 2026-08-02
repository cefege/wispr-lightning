//! Screen text: OCR of the frontmost window.
//!
//! Vision's `VNRecognizeTextRequest` maps onto `Windows.Media.Ocr`, which is
//! also on-device and also fast. The capture side is where the difficulty is:
//! `PrintWindow` asks the window to render itself into a DC, and without
//! `PW_RENDERFULLCONTENT` a GPU-composited window — Chrome, Electron, anything
//! hardware-accelerated, i.e. precisely what people dictate into — comes back
//! black.
//!
//! This is opportunistic context, never a dependency, so every failure path
//! returns an empty list. The one that deserves a word is a missing OCR
//! language pack: `TryCreateFromUserProfileLanguages` returns null, which is a
//! configuration fact rather than an error, so it is logged once and then
//! stays quiet.
//!
//! Unlike the Swift original, which has no timeout at all, the whole pipeline
//! is bounded: OCR over a 4K window takes seconds.

use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use windows::Graphics::Imaging::{BitmapPixelFormat, SoftwareBitmap};
use windows::Media::Ocr::OcrEngine;
use windows::Storage::Streams::DataWriter;
use windows::Win32::Graphics::Gdi::{
    CreateCompatibleBitmap, CreateCompatibleDC, DeleteDC, DeleteObject, GetDC, GetDIBits,
    ReleaseDC, SelectObject, BITMAPINFO, BITMAPINFOHEADER, BI_RGB, DIB_RGB_COLORS, HBITMAP, HDC,
    HGDIOBJ,
};
use windows::Win32::Storage::Xps::{PrintWindow, PRINT_WINDOW_FLAGS};
use windows::Win32::UI::WindowsAndMessaging::{GetForegroundWindow, GetWindowRect};

use crate::ScreenText;

/// Ceiling for capture plus recognition.
const OCR_BUDGET: Duration = Duration::from_secs(4);

/// `PW_RENDERFULLCONTENT`. Not exposed by the metadata, and load-bearing:
/// without it every composited window renders black.
const PW_RENDERFULLCONTENT: PRINT_WINDOW_FLAGS = PRINT_WINDOW_FLAGS(0x0000_0002);

/// Refuse anything past this many pixels. `OcrEngine` has its own dimension
/// limit, but a 60-megapixel capture is a large allocation before we ever get
/// to ask it.
const MAX_PIXELS: i64 = 40_000_000;

/// Logged once, then suppressed: a missing language pack does not change.
static WARNED_NO_LANGUAGE: AtomicBool = AtomicBool::new(false);

pub struct WindowsScreenText;

impl WindowsScreenText {
    pub fn new() -> Self {
        Self
    }
}

impl Default for WindowsScreenText {
    fn default() -> Self {
        Self::new()
    }
}

impl ScreenText for WindowsScreenText {
    fn ocr_frontmost_window(&self, max_lines: usize) -> Vec<String> {
        super::bounded("ocr", OCR_BUDGET, move || recognize(max_lines)).unwrap_or_default()
    }
}

fn recognize(max_lines: usize) -> Vec<String> {
    if max_lines == 0 {
        return Vec::new();
    }
    super::ensure_mta();

    let Some((pixels, width, height)) = capture_foreground_window() else {
        return Vec::new();
    };
    let engine = match OcrEngine::TryCreateFromUserProfileLanguages() {
        Ok(engine) => engine,
        Err(e) => {
            if !WARNED_NO_LANGUAGE.swap(true, Ordering::Relaxed) {
                tracing::info!(
                    error = %e,
                    "no OCR language pack is installed; screen context is unavailable"
                );
            }
            return Vec::new();
        }
    };
    if let Ok(limit) = OcrEngine::MaxImageDimension() {
        if width as u32 > limit || height as u32 > limit {
            tracing::debug!(
                width,
                height,
                limit,
                "window is larger than the OCR engine accepts"
            );
            return Vec::new();
        }
    }

    let Some(bitmap) = software_bitmap(&pixels, width, height) else {
        return Vec::new();
    };
    let result = match engine
        .RecognizeAsync(&bitmap)
        .and_then(|operation| operation.get())
    {
        Ok(result) => result,
        Err(e) => {
            tracing::debug!(error = %e, "OCR failed");
            return Vec::new();
        }
    };
    let Ok(lines) = result.Lines() else {
        return Vec::new();
    };
    // `Lines` is already in reading order, which is what the caller wants.
    lines
        .into_iter()
        .filter_map(|line| line.Text().ok())
        .map(|text| text.to_string())
        .filter(|text| !text.trim().is_empty())
        .take(max_lines)
        .collect()
}

/// BGRA pixels of the frontmost window, top row first.
fn capture_foreground_window() -> Option<(Vec<u8>, i32, i32)> {
    // SAFETY: no preconditions; a null result is handled.
    let window = unsafe { GetForegroundWindow() };
    if window.0.is_null() {
        return None;
    }
    let mut rect = Default::default();
    // SAFETY: `window` is live and `rect` is a valid out-parameter.
    unsafe { GetWindowRect(window, &mut rect) }.ok()?;
    let width = rect.right - rect.left;
    let height = rect.bottom - rect.top;
    if width <= 0 || height <= 0 || i64::from(width) * i64::from(height) > MAX_PIXELS {
        return None;
    }

    // SAFETY: every GDI object created below is released before returning,
    // including on the early-exit paths, via the explicit cleanup block.
    unsafe {
        let screen = GetDC(None);
        if screen.is_invalid() {
            return None;
        }
        let memory = CreateCompatibleDC(Some(screen));
        let bitmap = CreateCompatibleBitmap(screen, width, height);
        let previous = SelectObject(memory, HGDIOBJ(bitmap.0));

        let pixels = if memory.is_invalid() || bitmap.is_invalid() {
            None
        } else {
            let rendered = PrintWindow(window, memory, PW_RENDERFULLCONTENT).as_bool();
            if !rendered {
                tracing::debug!("PrintWindow refused to render the foreground window");
                None
            } else {
                read_pixels(memory, bitmap, width, height)
            }
        };

        SelectObject(memory, previous);
        let _ = DeleteObject(HGDIOBJ(bitmap.0));
        let _ = DeleteDC(memory);
        ReleaseDC(None, screen);
        pixels.map(|pixels| (pixels, width, height))
    }
}

/// # Safety
/// `memory` must be a device context with `bitmap` selected out of it, and
/// `width`/`height` must be the bitmap's dimensions.
unsafe fn read_pixels(memory: HDC, bitmap: HBITMAP, width: i32, height: i32) -> Option<Vec<u8>> {
    let mut info = BITMAPINFO {
        bmiHeader: BITMAPINFOHEADER {
            biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
            biWidth: width,
            // Negative height requests a top-down image, which is the order
            // `SoftwareBitmap` expects; a bottom-up buffer would OCR upside
            // down.
            biHeight: -height,
            biPlanes: 1,
            biBitCount: 32,
            biCompression: BI_RGB.0,
            ..Default::default()
        },
        ..Default::default()
    };
    let mut pixels = vec![0u8; (width as usize) * (height as usize) * 4];
    let copied = GetDIBits(
        memory,
        bitmap,
        0,
        height as u32,
        Some(pixels.as_mut_ptr().cast()),
        &mut info,
        DIB_RGB_COLORS,
    );
    if copied != height {
        return None;
    }
    // `PrintWindow` leaves the alpha channel at zero for most windows. The
    // only `SoftwareBitmap` constructor the metadata exposes assumes
    // premultiplied alpha, which would read that as a fully transparent —
    // effectively black — image, so opacity is asserted here instead.
    for pixel in pixels.chunks_exact_mut(4) {
        pixel[3] = 0xFF;
    }
    Some(pixels)
}

/// Wrap raw BGRA bytes in the `SoftwareBitmap` the OCR engine consumes.
fn software_bitmap(pixels: &[u8], width: i32, height: i32) -> Option<SoftwareBitmap> {
    let writer = DataWriter::new().ok()?;
    writer.WriteBytes(pixels).ok()?;
    let buffer = writer.DetachBuffer().ok()?;
    SoftwareBitmap::CreateCopyFromBuffer(&buffer, BitmapPixelFormat::Bgra8, width, height).ok()
}

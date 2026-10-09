//! Window-system calls the toolkit does not expose: the work area the overlay
//! is placed in, and the focus hardening that keeps it from ever taking focus
//! from the app being dictated into.

use dioxus::desktop::tao::window::Window;
use wl_shell::overlay_geometry::Rect;

/// The primary monitor's work area (excluding taskbar / Dock / menu bar) in
/// logical points, top-left origin. The *primary* monitor specifically,
/// matching the Swift `NSScreen.main`: the overlay has a fixed home rather than
/// following the cursor.
pub fn work_area(window: &Window) -> Option<Rect> {
    let monitor = window.primary_monitor()?;
    let scale = monitor.scale_factor();
    match native_work_area() {
        Some(physical) => Some(Rect {
            x: physical.x / scale,
            y: physical.y / scale,
            width: physical.width / scale,
            height: physical.height / scale,
        }),
        None => {
            // Loses the taskbar exclusion, so the pill may sit under it.
            tracing::warn!("no native work area; placing the overlay on the full monitor");
            let position = monitor.position();
            let size = monitor.size();
            Some(Rect {
                x: f64::from(position.x) / scale,
                y: f64::from(position.y) / scale,
                width: f64::from(size.width) / scale,
                height: f64::from(size.height) / scale,
            })
        }
    }
}

/// `SPI_GETWORKAREA`: the primary monitor's work area in physical pixels (the
/// process is per-monitor DPI aware, so no virtualisation applies).
#[cfg(target_os = "windows")]
fn native_work_area() -> Option<Rect> {
    use windows::Win32::Foundation::RECT;
    use windows::Win32::UI::WindowsAndMessaging::{
        SystemParametersInfoW, SPI_GETWORKAREA, SYSTEM_PARAMETERS_INFO_UPDATE_FLAGS,
    };

    let mut rect = RECT::default();
    // SAFETY: SPI_GETWORKAREA writes exactly one RECT through the pointer.
    unsafe {
        SystemParametersInfoW(
            SPI_GETWORKAREA,
            0,
            Some(std::ptr::from_mut(&mut rect).cast()),
            SYSTEM_PARAMETERS_INFO_UPDATE_FLAGS(0),
        )
    }
    .map_err(|e| tracing::warn!(error = %e, "SPI_GETWORKAREA failed"))
    .ok()?;
    Some(Rect {
        x: f64::from(rect.left),
        y: f64::from(rect.top),
        width: f64::from(rect.right - rect.left),
        height: f64::from(rect.bottom - rect.top),
    })
}

/// `NSScreen.mainScreen.visibleFrame`, flipped to a top-left origin and
/// scaled to physical pixels so the caller's division is uniform.
#[cfg(target_os = "macos")]
fn native_work_area() -> Option<Rect> {
    use objc2_app_kit::NSScreen;
    use objc2_foundation::MainThreadMarker;

    let mtm = MainThreadMarker::new()?;
    let screen = NSScreen::mainScreen(mtm)?;
    let full = screen.frame();
    let visible = screen.visibleFrame();
    let scale = screen.backingScaleFactor();
    Some(Rect {
        x: visible.origin.x * scale,
        y: (full.size.height - visible.origin.y - visible.size.height) * scale,
        width: visible.size.width * scale,
        height: visible.size.height * scale,
    })
}

#[cfg(not(any(target_os = "windows", target_os = "macos")))]
fn native_work_area() -> Option<Rect> {
    None
}

/// Make the overlay window non-activating and keep it out of Alt-Tab.
///
/// `WS_EX_NOACTIVATE` is what makes an ordinary show path non-activating;
/// `WS_EX_TOOLWINDOW` is the part `skip_taskbar` does not guarantee.
/// `SWP_FRAMECHANGED` makes the new style take effect and `SWP_NOACTIVATE`
/// keeps the topmost promotion from stealing focus.
#[cfg(target_os = "windows")]
pub fn harden_overlay(window: &Window) {
    use dioxus::desktop::tao::platform::windows::WindowExtWindows;
    use windows::Win32::Foundation::HWND;
    use windows::Win32::UI::WindowsAndMessaging::{
        GetWindowLongPtrW, SetWindowLongPtrW, SetWindowPos, GWL_EXSTYLE, HWND_TOPMOST,
        SWP_FRAMECHANGED, SWP_NOACTIVATE, SWP_NOMOVE, SWP_NOSIZE, WS_EX_NOACTIVATE,
        WS_EX_TOOLWINDOW, WS_EX_TOPMOST,
    };

    let hwnd = HWND(window.hwnd() as *mut std::ffi::c_void);
    // SAFETY: `hwnd` is the live top-level overlay window owned by this
    // process, and this runs on the thread that created it.
    let result = unsafe {
        let existing = GetWindowLongPtrW(hwnd, GWL_EXSTYLE) as u32;
        let extended = existing | WS_EX_NOACTIVATE.0 | WS_EX_TOOLWINDOW.0 | WS_EX_TOPMOST.0;
        SetWindowLongPtrW(hwnd, GWL_EXSTYLE, extended as isize);
        SetWindowPos(
            hwnd,
            Some(HWND_TOPMOST),
            0,
            0,
            0,
            0,
            SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE | SWP_FRAMECHANGED,
        )
    };
    if let Err(e) = result {
        tracing::warn!(error = %e, "could not harden the overlay window");
    }
}

/// Status-window level (floats over full-screen apps), on every Space, never
/// hidden on deactivation. The window stays an `NSWindow` rather than being
/// re-classed as a non-activating `NSPanel` (the plan's fallback, unverified
/// on a Mac), so a click on the pill's buttons may activate the app.
#[cfg(target_os = "macos")]
pub fn harden_overlay(window: &Window) {
    use dioxus::desktop::tao::platform::macos::WindowExtMacOS;
    use objc2_app_kit::{NSStatusWindowLevel, NSWindow, NSWindowCollectionBehavior};

    let ptr = window.ns_window() as *const NSWindow;
    // SAFETY: tao returns the live NSWindow backing this window; we are on the
    // main thread, where this runs from the window-creation hook.
    let Some(ns_window) = (unsafe { ptr.as_ref() }) else {
        return;
    };
    ns_window.setLevel(NSStatusWindowLevel);
    ns_window.setCollectionBehavior(
        NSWindowCollectionBehavior::CanJoinAllSpaces
            | NSWindowCollectionBehavior::Stationary
            | NSWindowCollectionBehavior::FullScreenAuxiliary
            | NSWindowCollectionBehavior::IgnoresCycle,
    );
    ns_window.setHidesOnDeactivate(false);
    ns_window.setOpaque(false);
    ns_window.setHasShadow(false);
    ns_window.setMovableByWindowBackground(false);
}

#[cfg(not(any(target_os = "windows", target_os = "macos")))]
pub fn harden_overlay(_window: &Window) {}

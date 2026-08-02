//! The recording overlay: a small pill that floats above every other window
//! and must never, under any circumstance, take keyboard focus.
//!
//! # The focus invariant
//!
//! Dictation types into whatever application the user was already using. If the
//! overlay takes focus, that application loses it and the text lands in the
//! wrong window — the app's entire purpose fails. Three separate mechanisms
//! enforce this, because each one alone is insufficient:
//!
//! - **`focusable(false)`** sets `canBecomeKeyWindow = NO` on macOS and
//!   `WS_EX_NOACTIVATE` on Windows. `focused(false)` is *not* a substitute: it
//!   only affects the first show, and tao's `set_visible(true)` — which is what
//!   `WebviewWindow::show()` calls — unconditionally invokes
//!   `makeKeyAndOrderFront:` (tao `platform_impl/macos/window.rs:666`).
//! - **macOS: an actual `NSPanel`** with `NSWindowStyleMask::NonactivatingPanel`.
//!   `canBecomeKeyWindow = NO` stops the *window* becoming key, but a click
//!   still activates the *application*. Only the nonactivating-panel bit
//!   prevents that, and AppKit honours it only on `NSPanel` instances; tao has
//!   no NSPanel support at all, hence `tauri-nspanel`. The panel is shown with
//!   `orderFrontRegardless` and hidden with `orderOut:` — never `show()`.
//! - **Windows: `WS_EX_TOOLWINDOW` + `SetWindowPos(HWND_TOPMOST, SWP_NOACTIVATE)`.**
//!   `skip_taskbar` alone does not reliably keep a window out of Alt-Tab.
//!
//! The macOS panel also buys the window level: tao's `always_on_top` is
//! `NSFloatingWindowLevel` (3), which sits *below* the menu bar and does not
//! float over full-screen apps. The overlay needs `NSStatusWindowLevel` (25).
//!
//! Covers MATRIX OVL-001..OVL-003, OVL-005..OVL-007, OVL-017, OVL-018,
//! OVL-024, OVL-032, OVL-041 and OVL-043, plus the [`crate::ui::Ui`] side of
//! the rest of section 13.

use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;
use std::time::Duration;

use parking_lot::Mutex;
use tauri::{AppHandle, Emitter, LogicalPosition, LogicalSize, Manager, WebviewUrl, WebviewWindow};

use crate::state::AppState;
use crate::ui::{Elapsed, OverlayState, Ui};

pub const OVERLAY_LABEL: &str = "overlay";

/// The panel's height never changes (OVL-017).
pub const OVERLAY_HEIGHT: f64 = 36.0;

/// Distance from the bottom of the work area to the bottom of the panel, from
/// the Swift `y = visibleFrame.minY + 50` (ui-spec §4.3).
pub const BOTTOM_MARGIN: f64 = 50.0;

/// A transient error dismisses itself after exactly this long (OVL-024).
const ERROR_DISMISS: Duration = Duration::from_millis(3000);

/// Width the overlay is built at, so its first real show is a resize rather
/// than a creation (OVL-041).
const INITIAL_WIDTH: f64 = 120.0;

// ---------------------------------------------------------------------------
// Geometry — pure, and therefore testable without a display server
// ---------------------------------------------------------------------------

/// A rectangle in logical points, top-left origin (the Tauri convention).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Rect {
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
}

/// The window width for a given overlay state, or `None` when the state puts
/// nothing on screen.
///
/// Widths come from ui-spec §4.4 / OVL-020..OVL-026 and OVL-032.
///
/// One deliberate collapse: the Swift `showRetryableError` was 260 wide without
/// a save handler and 300 with one, but [`OverlayState::Recoverable`] carries
/// no such flag — audio is always spooled before transcription, so Save is
/// always offered. 300 is therefore the only reachable width, and 260 is
/// unreachable rather than missing.
pub fn width_for(state: &OverlayState, elapsed_visible: bool) -> Option<f64> {
    Some(match state {
        OverlayState::Hidden => return None,
        // OVL-032: the panel jumps 120 -> 200 the first time the elapsed timer
        // appears, at 30 seconds.
        OverlayState::Recording | OverlayState::Locked if elapsed_visible => 200.0,
        // v2-ui-spec §1.2: 130, widened from 120 to fit the 88pt VU strip
        // inside the stack's 16pt edge insets.
        OverlayState::Recording | OverlayState::Locked => 130.0,
        OverlayState::Processing | OverlayState::Inserting => 145.0,
        OverlayState::Retrying { .. } => 175.0,
        OverlayState::Error { .. } => 180.0,
        OverlayState::Recoverable { .. } => 300.0,
    })
}

/// Bottom-centre of `work_area`, [`BOTTOM_MARGIN`] up from its bottom edge.
///
/// `work_area` excludes the menu bar and the Dock / taskbar, which is what both
/// `NSScreen.visibleFrame` and `SPI_GETWORKAREA` give.
pub fn overlay_frame(work_area: Rect, width: f64) -> Rect {
    Rect {
        x: work_area.x + (work_area.width - width) / 2.0,
        // AppKit measures y up from the bottom; Tauri measures it down from the
        // top, so the Swift `minY + 50` becomes "the work area's bottom edge,
        // less the margin, less our own height".
        y: work_area.y + work_area.height - BOTTOM_MARGIN - OVERLAY_HEIGHT,
        width,
        height: OVERLAY_HEIGHT,
    }
}

/// Whether the overlay accepts clicks in this state.
///
/// Only two states carry a control. [`OverlayState::Recoverable`] has the
/// Retry / Save / ✕ row (OVL-007); Recording and Locked have the
/// hover-revealed cancel ✕ (v2-ui-spec §1.7), which needs both the click and
/// the `mouseenter` that reveals it, and gets neither through a click-through
/// window. Everything else is passive, and a passive overlay that swallowed
/// clicks would make the strip of desktop it covers unusable.
///
/// Accepting clicks is not the same as taking focus. The window stays
/// `focusable(false)` and, on macOS, a genuine non-activating `NSPanel`: a
/// click on the ✕ must not move focus off the app being dictated into, or the
/// transcript lands in the wrong window.
pub fn accepts_clicks(state: &OverlayState) -> bool {
    matches!(
        state,
        OverlayState::Recoverable { .. } | OverlayState::Recording | OverlayState::Locked
    )
}

// ---------------------------------------------------------------------------
// The panel class (macOS)
// ---------------------------------------------------------------------------

#[cfg(target_os = "macos")]
mod panel_class {
    // Both are used by the macro expansion, not by the code written here:
    // `Manager` for `WebviewWindow::app_handle`, and the macro brings its own
    // objc2 imports, so importing `NSObjectProtocol` again would collide.
    use tauri::Manager;
    use tauri_nspanel::tauri_panel;

    tauri_panel! {
        panel!(OverlayPanel {
            config: {
                // Belt and braces with `focusable(false)`: this is the class's
                // own answer, so it holds even if the window's focusable ivar
                // is ever changed underneath us.
                can_become_key_window: false,
                can_become_main_window: false,
            }
        })
    }
}

// ---------------------------------------------------------------------------
// The live overlay
// ---------------------------------------------------------------------------

pub struct Overlay {
    app: AppHandle,
    window: WebviewWindow,

    #[cfg(target_os = "macos")]
    panel: tauri_nspanel::PanelHandle<tauri::Wry>,

    /// Last applied width, or `None` when the overlay is hidden.
    ///
    /// OVL-018: a resize to the width already in force is a no-op, and hiding
    /// clears this so the next show always repositions — which is how the panel
    /// re-centres after a wide error state, or after the display arrangement
    /// changed while it was hidden.
    current_width: Mutex<Option<f64>>,

    /// The elapsed readout is showing, so Recording/Locked is 200 wide.
    elapsed_visible: AtomicBool,

    /// Bumped on every state change. The transient-error auto-dismiss captures
    /// the value it was scheduled at and does nothing if it has moved on, so a
    /// newer state — including a second error — cancels the pending hide
    /// instead of having it fire underneath.
    generation: AtomicU64,

    last_state: Mutex<OverlayState>,
}

impl Overlay {
    /// Build the overlay window and harden it, without showing it (OVL-041:
    /// the first hotkey press must not pay construction latency).
    pub fn create(app: &AppHandle) -> tauri::Result<Arc<Self>> {
        let builder = tauri::WebviewWindowBuilder::new(
            app,
            OVERLAY_LABEL,
            WebviewUrl::App("overlay.html".into()),
        )
        // THE invariant. See the module docs.
        .focusable(false)
        .focused(false)
        .always_on_top(true)
        .decorations(false)
        .transparent(true)
        .skip_taskbar(true)
        // OVL-004 says the Swift panel had a shadow, but that is AppKit's
        // shadow, drawn around an opaque frame. On a transparent window it
        // renders around the pill's bounding box rather than its rounded
        // outline, so the frontend draws the shadow in CSS and the window has
        // none of its own.
        .shadow(false)
        .resizable(false)
        .maximizable(false)
        .minimizable(false)
        .closable(false)
        .inner_size(INITIAL_WIDTH, OVERLAY_HEIGHT)
        .visible(false);

        #[cfg(target_os = "macos")]
        let builder = builder
            // OVL-006: visible on every Space.
            .visible_on_all_workspaces(true)
            // The overlay is never the focused window, and AppKit swallows the
            // first click on an unfocused window as an activation click. Both
            // of its controls — the Retry / Save row and the recording ✕ —
            // would need pressing twice without this.
            .accept_first_mouse(true);

        // `no_redirection_bitmap` — tao's `WindowBuilderExtWindows` flag that
        // suppresses the white flash a transparent window shows before its
        // first paint — is not exposed by Tauri 2.11.5's builder, so it cannot
        // be set from here. It is not needed either: the flash only happens
        // when a transparent window is made visible before the webview has
        // painted, and this one is created hidden at launch and first shown at
        // the first hotkey press, long after. Without the flag tao instead
        // applies `DwmEnableBlurBehindWindow` with an empty region
        // (`tao platform_impl/windows/window.rs:1284`), which is the standard
        // per-pixel-alpha route and what every transparent Tauri window uses.

        let window = builder.build()?;

        #[cfg(target_os = "macos")]
        let panel = harden_macos(&window)?;

        #[cfg(target_os = "windows")]
        harden_windows(&window)?;

        // Passive states are click-through, and the overlay starts passive.
        if let Err(e) = window.set_ignore_cursor_events(true) {
            tracing::warn!(error = %e, "could not make the overlay click-through");
        }

        Ok(Arc::new(Self {
            app: app.clone(),
            window,
            #[cfg(target_os = "macos")]
            panel,
            current_width: Mutex::new(None),
            elapsed_visible: AtomicBool::new(false),
            generation: AtomicU64::new(0),
            last_state: Mutex::new(OverlayState::Hidden),
        }))
    }

    /// Size and place the window for `width`, then order it in.
    fn present(&self, width: f64) {
        {
            let mut current = self.current_width.lock();
            if *current != Some(width) {
                self.apply_frame(width);
                *current = Some(width);
            }
        }
        self.order_front();
    }

    fn apply_frame(&self, width: f64) {
        let Some(work_area) = self.work_area() else {
            tracing::warn!("no monitor to position the overlay on");
            return;
        };
        let frame = overlay_frame(work_area, width);

        if let Err(e) = self
            .window
            .set_size(LogicalSize::new(frame.width, frame.height))
        {
            tracing::warn!(error = %e, "could not resize the overlay");
        }
        if let Err(e) = self
            .window
            .set_position(LogicalPosition::new(frame.x, frame.y))
        {
            tracing::warn!(error = %e, "could not position the overlay");
        }
    }

    /// The primary monitor's work area, in logical points.
    ///
    /// The *primary* monitor specifically, matching the Swift `NSScreen.main`
    /// behaviour: the overlay has a fixed home rather than following the
    /// cursor, so it never appears somewhere the user is not looking.
    fn work_area(&self) -> Option<Rect> {
        let monitor = match self.app.primary_monitor() {
            Ok(Some(monitor)) => monitor,
            Ok(None) => return None,
            Err(e) => {
                tracing::warn!(error = %e, "could not query the primary monitor");
                return None;
            }
        };
        let scale = monitor.scale_factor();
        let area = monitor.work_area();
        Some(Rect {
            x: f64::from(area.position.x) / scale,
            y: f64::from(area.position.y) / scale,
            width: f64::from(area.size.width) / scale,
            height: f64::from(area.size.height) / scale,
        })
    }

    /// Show without activating. OVL-002: never `show()` on macOS, because that
    /// is `makeKeyAndOrderFront:`.
    fn order_front(&self) {
        #[cfg(target_os = "macos")]
        self.on_panel(|panel| panel.show());

        #[cfg(not(target_os = "macos"))]
        {
            // `WS_EX_NOACTIVATE`, applied by `focusable(false)`, makes the
            // ordinary show path non-activating on Windows.
            if let Err(e) = self.window.show() {
                tracing::warn!(error = %e, "could not show the overlay");
            }
        }
    }

    fn order_out(&self) {
        #[cfg(target_os = "macos")]
        self.on_panel(|panel| panel.hide());

        #[cfg(not(target_os = "macos"))]
        {
            if let Err(e) = self.window.hide() {
                tracing::warn!(error = %e, "could not hide the overlay");
            }
        }
    }

    /// Run an AppKit panel operation on the main thread.
    ///
    /// Every `NSWindow`/`NSPanel` message is main-thread only, and AppKit traps
    /// the process rather than misbehaving when one arrives from elsewhere —
    /// which it will, because the pipeline drives the overlay from a worker
    /// task. Tauri's own window and tray calls hop the thread internally; the
    /// raw panel does not, so this is where it happens.
    ///
    /// Fire-and-forget, and the event loop preserves submission order, so a
    /// show immediately followed by a hide cannot land inverted.
    #[cfg(target_os = "macos")]
    fn on_panel(&self, f: impl FnOnce(&dyn tauri_nspanel::Panel<tauri::Wry>) + Send + 'static) {
        let panel = self.panel.clone();
        if let Err(e) = self.app.run_on_main_thread(move || f(panel.as_ref())) {
            tracing::warn!(error = %e, "could not reach the main thread for the overlay panel");
        }
    }

    /// Apply a state to the window without emitting it, so `set_elapsed` can
    /// resize when the timer appears without republishing the state.
    fn render(&self, state: &OverlayState) {
        let elapsed_visible = self.elapsed_visible.load(Ordering::Acquire);
        match width_for(state, elapsed_visible) {
            Some(width) => {
                if let Err(e) = self.window.set_ignore_cursor_events(!accepts_clicks(state)) {
                    tracing::warn!(error = %e, "could not change overlay click-through");
                }
                self.present(width);
            }
            None => {
                self.order_out();
                // Force a reposition on the next show (OVL-018).
                *self.current_width.lock() = None;
            }
        }
    }
}

/// Schedule the OVL-024 three-second auto-dismiss for a transient error.
fn schedule_dismiss(overlay: &Arc<Overlay>, generation: u64) {
    let overlay = Arc::clone(overlay);
    tauri::async_runtime::spawn(async move {
        tokio::time::sleep(ERROR_DISMISS).await;
        // Anything that happened since supersedes the dismissal.
        if overlay.generation.load(Ordering::Acquire) != generation {
            return;
        }
        overlay.set_overlay(OverlayState::Hidden);
    });
}

/// Implemented on `Arc<Overlay>` rather than `Overlay` because the auto-dismiss
/// timer has to outlive the call that scheduled it.
impl Ui for Arc<Overlay> {
    fn set_overlay(&self, state: OverlayState) {
        // First, so a pending auto-dismiss sees the new generation even if the
        // work below is slow.
        let generation = self.generation.fetch_add(1, Ordering::AcqRel) + 1;

        // The width analogue of OVL-034: a new recording, a new processing pass
        // and a hide all start again with no timer showing.
        if matches!(
            state,
            OverlayState::Hidden
                | OverlayState::Recording
                | OverlayState::Locked
                | OverlayState::Processing
        ) {
            self.elapsed_visible.store(false, Ordering::Release);
        }

        *self.last_state.lock() = state.clone();
        self.render(&state);

        if let Err(e) = self.app.emit("overlay:state", &state) {
            tracing::warn!(error = %e, "could not publish the overlay state");
        }

        // OVL-024 / OVL-025: transient errors dismiss themselves, recoverable
        // ones persist until the user acts.
        if matches!(state, OverlayState::Error { .. }) {
            schedule_dismiss(self, generation);
        }
    }

    fn set_elapsed(&self, elapsed: Elapsed) {
        let visible = elapsed.label.is_some();
        let changed = self.elapsed_visible.swap(visible, Ordering::AcqRel) != visible;

        if let Err(e) = self.app.emit("overlay:elapsed", &elapsed) {
            tracing::warn!(error = %e, "could not publish the elapsed time");
        }

        // The 130 -> 200 jump at 30 seconds is a window resize, and a webview
        // cannot resize its own window.
        if changed {
            let state = self.last_state.lock().clone();
            self.render(&state);
        }
    }

    fn set_level(&self, level: f32) {
        // Emit and nothing else. This runs ~25 times a second for the whole
        // recording, so it deliberately does not take `last_state`, does not
        // call `render`, and never touches window geometry: the VU strip is
        // painted inside a pill whose size the level cannot change.
        //
        // A dropped frame here is invisible — the next one is 40 ms away and
        // the strip smooths across it — so a failed emit is logged at debug
        // rather than warn, to keep a disconnected webview from filling the
        // log at 25 Hz.
        if let Err(e) = self.app.emit("overlay:level", level) {
            tracing::debug!(error = %e, "could not publish the audio level");
        }
    }

    fn set_recording_indicator(&self, recording: bool) {
        if let Some(state) = self.app.try_state::<Arc<AppState>>() {
            if let Some(tray) = state.tray() {
                tray.set_recording(recording);
            }
        }
    }

    fn set_last_transcription(&self, text: &str) {
        if let Some(state) = self.app.try_state::<Arc<AppState>>() {
            if let Some(tray) = state.tray() {
                tray.set_last_transcription(text);
            }
        }
    }

    fn notify_changed(&self, topic: &str) {
        // Callers pass either a bare subject ("devices") or a full event name
        // ("devices:changed"), so neither side has to know the other's
        // spelling.
        let event = if topic.contains(':') {
            topic.to_string()
        } else {
            format!("{topic}:changed")
        };

        if event.starts_with("devices") {
            if let Some(state) = self.app.try_state::<Arc<AppState>>() {
                if let Some(tray) = state.tray() {
                    // TRY-006: the device submenu must follow an unplug, even
                    // mid-recording.
                    match state.audio.list_devices() {
                        Ok(devices) => tray.set_devices(devices),
                        Err(e) => tracing::warn!(error = %e, "could not re-enumerate devices"),
                    }
                }
            }
        }

        if let Err(e) = self.app.emit(&event, ()) {
            tracing::warn!(error = %e, event, "could not publish a change notification");
        }
    }
}

// ---------------------------------------------------------------------------
// Platform hardening
// ---------------------------------------------------------------------------

#[cfg(target_os = "macos")]
fn harden_macos(window: &WebviewWindow) -> tauri::Result<tauri_nspanel::PanelHandle<tauri::Wry>> {
    use panel_class::OverlayPanel;
    use tauri_nspanel::{CollectionBehavior, PanelLevel, StyleMask, WebviewWindowExt};

    let panel = window.to_panel::<OverlayPanel>()?;

    // The Swift panel's style mask verbatim (ui-spec §4.1). The nonactivating
    // bit is the one that stops a click activating the application.
    panel.set_style_mask(
        StyleMask::empty()
            .nonactivating_panel()
            .full_size_content_view()
            .value(),
    );

    // NSStatusWindowLevel (25). tao's always_on_top is only
    // NSFloatingWindowLevel (3), which does not float over full-screen apps.
    panel.set_level(PanelLevel::Status.value());

    // OVL-006: every Space, ignores Exposé, visible over full-screen apps, and
    // absent from Cmd-Tab.
    panel.set_collection_behavior(
        CollectionBehavior::new()
            .can_join_all_spaces()
            .stationary()
            .full_screen_auxiliary()
            .ignores_cycle()
            .value(),
    );

    // A floating panel keeps its level above the app's other windows without
    // becoming key.
    panel.set_floating_panel(true);
    // Hiding on deactivation would hide the overlay the instant focus moved to
    // the app being dictated into — i.e. always.
    panel.set_hides_on_deactivate(false);
    panel.set_opaque(false);
    panel.set_has_shadow(false);
    // OVL-005: not draggable by its background.
    panel.set_movable_by_window_background(false);
    // The overlay outlives every hide; releasing it on close would leave this
    // handle dangling.
    panel.set_released_when_closed(false);

    Ok(panel)
}

#[cfg(target_os = "windows")]
fn harden_windows(window: &WebviewWindow) -> tauri::Result<()> {
    use windows::Win32::UI::WindowsAndMessaging::{
        GetWindowLongPtrW, SetWindowLongPtrW, SetWindowPos, GWL_EXSTYLE, HWND_TOPMOST,
        SWP_FRAMECHANGED, SWP_NOACTIVATE, SWP_NOMOVE, SWP_NOSIZE, WS_EX_NOACTIVATE,
        WS_EX_TOOLWINDOW, WS_EX_TOPMOST,
    };

    let hwnd = window.hwnd()?;
    // SAFETY: `hwnd` is a live top-level window owned by this process for as
    // long as the overlay exists, and both calls happen before it is shown.
    unsafe {
        let existing = GetWindowLongPtrW(hwnd, GWL_EXSTYLE) as u32;
        // NOACTIVATE is already set by `focusable(false)`; re-asserting it costs
        // nothing and documents the dependency. TOOLWINDOW is the part
        // `skip_taskbar` does not guarantee — it is what keeps the overlay out
        // of Alt-Tab.
        let extended = existing | WS_EX_NOACTIVATE.0 | WS_EX_TOOLWINDOW.0 | WS_EX_TOPMOST.0;
        SetWindowLongPtrW(hwnd, GWL_EXSTYLE, extended as isize);

        // SWP_FRAMECHANGED makes the new extended style take effect;
        // SWP_NOACTIVATE keeps the topmost promotion from stealing focus.
        SetWindowPos(
            hwnd,
            Some(HWND_TOPMOST),
            0,
            0,
            0,
            0,
            SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE | SWP_FRAMECHANGED,
        )
        .map_err(|e| tauri::Error::Anyhow(e.into()))?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A 1440x900 laptop display with a 25-point menu bar at the top.
    fn work_area() -> Rect {
        Rect {
            x: 0.0,
            y: 25.0,
            width: 1440.0,
            height: 875.0,
        }
    }

    fn widths(elapsed_visible: bool) -> Vec<(&'static str, Option<f64>)> {
        vec![
            ("Hidden", width_for(&OverlayState::Hidden, elapsed_visible)),
            (
                "Recording",
                width_for(&OverlayState::Recording, elapsed_visible),
            ),
            ("Locked", width_for(&OverlayState::Locked, elapsed_visible)),
            (
                "Processing",
                width_for(&OverlayState::Processing, elapsed_visible),
            ),
            (
                "Inserting",
                width_for(&OverlayState::Inserting, elapsed_visible),
            ),
            (
                "Retrying",
                width_for(
                    &OverlayState::Retrying { attempt: 1, of: 3 },
                    elapsed_visible,
                ),
            ),
            (
                "Error",
                width_for(
                    &OverlayState::Error {
                        message: "Timed out".into(),
                    },
                    elapsed_visible,
                ),
            ),
            (
                "Recoverable",
                width_for(
                    &OverlayState::Recoverable {
                        message: "Connection failed".into(),
                    },
                    elapsed_visible,
                ),
            ),
        ]
    }

    /// v2-ui-spec §1.9, every row.
    #[test]
    fn state_widths_match_the_spec() {
        assert_eq!(
            widths(false),
            vec![
                ("Hidden", None),
                // 130, not 120: the pill now carries the 88pt VU strip.
                ("Recording", Some(130.0)),
                ("Locked", Some(130.0)),
                ("Processing", Some(145.0)),
                ("Inserting", Some(145.0)),
                ("Retrying", Some(175.0)),
                ("Error", Some(180.0)),
                ("Recoverable", Some(300.0)),
            ]
        );
    }

    /// OVL-032: only the two recording states widen for the elapsed readout.
    #[test]
    fn the_elapsed_timer_widens_only_the_recording_states() {
        assert_eq!(
            widths(true),
            vec![
                ("Hidden", None),
                ("Recording", Some(200.0)),
                ("Locked", Some(200.0)),
                ("Processing", Some(145.0)),
                ("Inserting", Some(145.0)),
                ("Retrying", Some(175.0)),
                ("Error", Some(180.0)),
                ("Recoverable", Some(300.0)),
            ]
        );
    }

    #[test]
    fn the_height_is_thirty_six_in_every_state() {
        for (_, width) in widths(false).into_iter().chain(widths(true)) {
            let Some(width) = width else { continue };
            assert_eq!(overlay_frame(work_area(), width).height, OVERLAY_HEIGHT);
        }
    }

    /// OVL-017: horizontally centred, and 50 points up from the bottom of the
    /// work area.
    #[test]
    fn the_frame_is_bottom_centred_in_the_work_area() {
        let frame = overlay_frame(work_area(), 120.0);
        assert_eq!(frame.x, 660.0, "centred: (1440 - 120) / 2");
        // The work area's bottom edge is y = 25 + 875 = 900; the panel's bottom
        // sits 50 above that, and its top a further 36 above.
        assert_eq!(frame.y, 900.0 - 50.0 - 36.0);
        assert_eq!(frame.y + frame.height, 900.0 - BOTTOM_MARGIN);
    }

    #[test]
    fn every_state_stays_centred_on_the_same_axis() {
        let centre = work_area().x + work_area().width / 2.0;
        for (state, width) in widths(false) {
            let Some(width) = width else { continue };
            let frame = overlay_frame(work_area(), width);
            assert_eq!(frame.x + frame.width / 2.0, centre, "{state} is off centre");
        }
    }

    /// A monitor whose work area does not start at the origin — a second
    /// display to the right, or a Windows taskbar docked to the left.
    #[test]
    fn the_frame_respects_an_offset_work_area() {
        let offset = Rect {
            x: 1440.0,
            y: 0.0,
            width: 1920.0,
            height: 1040.0,
        };
        let frame = overlay_frame(offset, 300.0);
        assert_eq!(frame.x, 1440.0 + (1920.0 - 300.0) / 2.0);
        assert_eq!(frame.y, 1040.0 - 50.0 - 36.0);
    }

    /// Exactly the states with a control accept clicks; everything else is
    /// click-through so the overlay never blocks what is underneath it
    /// (OVL-007, and v2-ui-spec §1.7 for the recording ✕).
    #[test]
    fn only_the_states_with_a_control_accept_clicks() {
        // The hover-revealed cancel ✕ lives in these two, and a click-through
        // window would receive neither the click nor the hover that reveals it.
        assert!(accepts_clicks(&OverlayState::Recording));
        assert!(accepts_clicks(&OverlayState::Locked));
        // Retry / Save / ✕.
        assert!(accepts_clicks(&OverlayState::Recoverable {
            message: "Connection failed".into()
        }));

        assert!(!accepts_clicks(&OverlayState::Hidden));
        assert!(!accepts_clicks(&OverlayState::Processing));
        assert!(!accepts_clicks(&OverlayState::Inserting));
        assert!(!accepts_clicks(&OverlayState::Retrying {
            attempt: 2,
            of: 3
        }));
        assert!(!accepts_clicks(&OverlayState::Error {
            message: "Timed out".into()
        }));
    }

    /// The event payloads are a frozen part of the IPC contract.
    #[test]
    fn overlay_states_serialise_to_the_contract_shape() {
        let json = |state: &OverlayState| serde_json::to_string(state).expect("serialisable");
        assert_eq!(json(&OverlayState::Hidden), r#""Hidden""#);
        assert_eq!(json(&OverlayState::Recording), r#""Recording""#);
        assert_eq!(json(&OverlayState::Locked), r#""Locked""#);
        assert_eq!(json(&OverlayState::Processing), r#""Processing""#);
        assert_eq!(json(&OverlayState::Inserting), r#""Inserting""#);
        assert_eq!(
            json(&OverlayState::Retrying { attempt: 1, of: 3 }),
            r#"{"Retrying":{"attempt":1,"of":3}}"#
        );
        assert_eq!(
            json(&OverlayState::Error {
                message: "Timed out".into()
            }),
            r#"{"Error":{"message":"Timed out"}}"#
        );
        assert_eq!(
            json(&OverlayState::Recoverable {
                message: "Connection failed".into()
            }),
            r#"{"Recoverable":{"message":"Connection failed"}}"#
        );
    }

    #[test]
    fn the_elapsed_payload_keeps_its_field_names() {
        let json = serde_json::to_string(&Elapsed {
            label: Some("9:05 \u{26A0}\u{FE0F}".into()),
            warning: 1,
        })
        .expect("serialisable");
        assert_eq!(json, r#"{"label":"9:05 ⚠️","warning":1}"#);
        assert_eq!(
            serde_json::to_string(&Elapsed::default()).expect("serialisable"),
            r#"{"label":null,"warning":0}"#
        );
    }
}

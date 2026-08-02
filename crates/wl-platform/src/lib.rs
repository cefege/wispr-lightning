//! Platform integration: everything the app needs from the operating system.
//!
//! This crate is the only place that may talk to AppKit, CoreAudio, Win32 or
//! UI Automation. Each capability is a trait here with one implementation per
//! target, so `wl-core` and the Tauri layer stay portable and testable.
//!
//! Every trait is object-safe and `Send + Sync`: the orchestrator holds them
//! behind `Arc<dyn _>` and calls them from worker tasks.

use std::time::Duration;

pub mod audio;
pub mod audio_impl;
pub mod chord;
pub mod error;
pub mod hotkey;
pub mod resample;
pub mod sound;
pub mod sound_impl;
pub(crate) mod typing;

#[cfg(target_os = "macos")]
pub mod macos;
#[cfg(target_os = "windows")]
pub mod windows;

pub use error::{PlatformError, Result};

/// Construct the implementation set for the current target.
pub mod current {
    #[cfg(target_os = "macos")]
    pub use crate::macos::*;
    #[cfg(target_os = "windows")]
    pub use crate::windows::*;
}

// ---------------------------------------------------------------------------
// Foreground application
// ---------------------------------------------------------------------------

/// Coarse classification of the focused app, used by the backend to pick a
/// formatting style. The wire values are lowercase and fixed by the protocol.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum AppKind {
    Messaging,
    Email,
    Ai,
    #[default]
    Other,
}

impl AppKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Messaging => "messaging",
            Self::Email => "email",
            Self::Ai => "ai",
            Self::Other => "other",
        }
    }
}

/// Identity of the frontmost application at the moment recording started.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct AppInfo {
    /// Human-readable application name.
    pub name: String,
    /// macOS bundle identifier, or the Windows executable basename.
    pub bundle_id: String,
    pub kind: AppKind,
    /// URL of the focused browser tab, when the frontmost app is a browser.
    pub url: String,
}

pub trait ForegroundApp: Send + Sync {
    fn current(&self) -> AppInfo;
}

// ---------------------------------------------------------------------------
// Text injection
// ---------------------------------------------------------------------------

// No `Eq`: the Natural variant carries an f64.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum InjectMode {
    /// Set the clipboard, synthesize the paste shortcut, then restore.
    Paste,
    /// Type character by character with human-like timing.
    Natural { chars_per_second: f64 },
}

/// Opaque snapshot of the clipboard, restored verbatim after an injection.
///
/// Not a `String`: the clipboard may hold images, rich text or several
/// representations of one item, and clobbering those is a data-loss bug.
pub struct ClipboardSnapshot(pub Box<dyn std::any::Any + Send + Sync>);

pub trait TextInjector: Send + Sync {
    /// Insert `text` at the caret of the focused application.
    ///
    /// Success means the events were synthesized, not that the target
    /// consumed them — nothing can tell us that. `CGEvent.post` and
    /// `SendInput` are fire-and-forget, and the accessibility read-back that
    /// used to stand in for confirmation was deleted upstream (B-001) because
    /// it false-negatived on essentially every paste: chat composers,
    /// `contenteditable` web fields, terminals and code editors expose no
    /// readable value, so "unverified" meant "unreadable", not "failed". An
    /// empty `text` is a no-op.
    fn inject(&self, text: &str, mode: InjectMode) -> Result<()>;

    /// Abort an in-flight [`InjectMode::Natural`] pass at the next character.
    ///
    /// Driven by the Escape watcher: half a paragraph of synthesized typing is
    /// otherwise unstoppable, and it is landing in whatever the user is
    /// looking at. Partial output is not an error — the pass that was
    /// cancelled still returns `Ok`. Calling this while nothing is typing is
    /// harmless: every pass clears the flag before its first character.
    fn cancel_typing(&self);

    /// Press the system undo shortcut once — Cmd+Z on macOS, Ctrl+Z on
    /// Windows — to take back the dictation that was just injected (B-006).
    ///
    /// Deliberately a plain shortcut and not a "delete N characters" replay:
    /// the user may have typed since, and the target application's own undo
    /// stack is the only thing that knows what our paste actually replaced.
    /// The caller is responsible for firing it at most once per dictation.
    fn undo_last_injection(&self) -> Result<()>;

    /// Full text of the focused control, used as transcription context.
    fn read_focused_text(&self) -> Vec<String>;

    fn snapshot_clipboard(&self) -> Result<ClipboardSnapshot>;
    fn restore_clipboard(&self, snapshot: ClipboardSnapshot) -> Result<()>;
}

// ---------------------------------------------------------------------------
// Screen text (OCR)
// ---------------------------------------------------------------------------

pub trait ScreenText: Send + Sync {
    /// OCR the frontmost window, returning at most `max_lines` lines in
    /// reading order. Returns empty when unavailable or not permitted — this
    /// is opportunistic context, never a hard dependency.
    fn ocr_frontmost_window(&self, max_lines: usize) -> Vec<String>;
}

// ---------------------------------------------------------------------------
// Media control
// ---------------------------------------------------------------------------

pub trait MediaControl: Send + Sync {
    /// Pause playback if something is playing. Returns whether anything was
    /// paused, so `resume` can be a no-op when we did not interrupt anything.
    fn pause(&self) -> bool;
    fn resume(&self);
}

// ---------------------------------------------------------------------------
// Permissions
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Permission {
    Microphone,
    /// macOS Accessibility: required to inject text.
    Accessibility,
    /// macOS Input Monitoring: required to observe global hotkeys.
    InputMonitoring,
    /// macOS Screen Recording: required for OCR context.
    ScreenRecording,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PermissionState {
    Granted,
    Denied,
    /// Never asked; a prompt is still possible.
    NotDetermined,
    /// The concept does not exist on this platform.
    NotApplicable,
}

pub trait Permissions: Send + Sync {
    fn status(&self, permission: Permission) -> PermissionState;

    /// Ask the OS to prompt. No-op where the platform offers no prompt —
    /// notably microphone access on Windows, which is a global user setting.
    fn request(&self, permission: Permission);

    /// Open the relevant system settings pane, for the denied case where no
    /// prompt will ever appear again.
    fn open_settings(&self, permission: Permission);
}

// ---------------------------------------------------------------------------
// App lifecycle
// ---------------------------------------------------------------------------

pub trait Lifecycle: Send + Sync {
    /// Notification that the machine is about to sleep, so an in-flight
    /// recording can be abandoned rather than resumed with a gap.
    fn on_sleep(&self, handler: Box<dyn Fn() + Send + Sync>);
    fn set_launch_at_login(&self, enabled: bool) -> Result<()>;
    fn launch_at_login(&self) -> bool;
}

// ---------------------------------------------------------------------------
// Appearance
// ---------------------------------------------------------------------------

/// An 8-bit-per-channel sRGB colour.
///
/// sRGB specifically, because the only consumer is CSS and CSS hex notation is
/// sRGB by definition. Neither platform hands its accent over in that space
/// unprompted — AppKit explicitly declines to promise a colour space for
/// `controlAccentColor` — so conversion happens in the platform module and
/// nothing downstream has to wonder.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Rgb {
    pub r: u8,
    pub g: u8,
    pub b: u8,
}

/// Text drawn on top of a *dark* accent.
pub const ACCENT_TEXT_ON_DARK: Rgb = Rgb::new(0xff, 0xff, 0xff);

/// Text drawn on top of a *light* accent — a yellow or lime accent is the case
/// that motivates this existing at all.
///
/// Near-black rather than pure black, to sit at the same weight as
/// `--text-primary` (`rgba(0, 0, 0, 0.85)` over white) instead of reading as a
/// harder edge than every other label in the window.
pub const ACCENT_TEXT_ON_LIGHT: Rgb = Rgb::new(0x1d, 0x1d, 0x1f);

/// Relative luminance above which an accent counts as light.
///
/// Not the WCAG "maximum contrast" pick, which crosses over at 0.179 and would
/// put near-black text on the *default* blue of both platforms — nobody's
/// system renders that. The midpoint keeps white on every accent macOS and
/// Windows ship except the genuinely pale ones (yellow, lime), which is what
/// the two shells themselves do.
const LIGHT_ACCENT_LUMINANCE: f64 = 0.5;

impl Rgb {
    pub const fn new(r: u8, g: u8, b: u8) -> Self {
        Self { r, g, b }
    }

    /// `#rrggbb` — the one form CSS takes without further parsing.
    pub fn to_hex(self) -> String {
        format!("#{:02x}{:02x}{:02x}", self.r, self.g, self.b)
    }

    /// WCAG 2.x relative luminance, 0.0 for black and 1.0 for white.
    pub fn relative_luminance(self) -> f64 {
        fn linear(channel: u8) -> f64 {
            let c = f64::from(channel) / 255.0;
            if c <= 0.040_45 {
                c / 12.92
            } else {
                ((c + 0.055) / 1.055).powf(2.4)
            }
        }
        0.2126 * linear(self.r) + 0.7152 * linear(self.g) + 0.0722 * linear(self.b)
    }

    /// The text colour to draw on top of this one.
    pub fn foreground(self) -> Rgb {
        if self.relative_luminance() > LIGHT_ACCENT_LUMINANCE {
            ACCENT_TEXT_ON_LIGHT
        } else {
            ACCENT_TEXT_ON_DARK
        }
    }

    /// The hover and pressed shade for an accent sitting on a *light*
    /// background — a small step toward black, which is the direction both
    /// platforms' filled controls move when the window is in light appearance.
    ///
    /// Derived rather than stated because the accent is now the user's, and
    /// derived here rather than in CSS because `color-mix()` does not exist in
    /// Safari 16.1, the oldest WebKit this app supports.
    pub fn darker(self) -> Rgb {
        self.mix(Rgb::new(0, 0, 0), 0.12)
    }

    /// The same step for a *dark* background, toward white.
    ///
    /// Larger than [`Rgb::darker`]'s: the eye reads a lightening step against a
    /// dark surround as smaller than the equivalent darkening against a light
    /// one, and 0.12 white over a mid accent barely registers.
    pub fn lighter(self) -> Rgb {
        self.mix(Rgb::new(255, 255, 255), 0.20)
    }

    /// Linear blend in sRGB space, `amount` of `other` over `self`.
    ///
    /// Gamma-space rather than light-linear on purpose: this reproduces what a
    /// translucent overlay of `other` would paint, which is what the shade is
    /// standing in for.
    fn mix(self, other: Rgb, amount: f64) -> Rgb {
        let blend = |a: u8, b: u8| {
            let a = f64::from(a);
            (a + (f64::from(b) - a) * amount).round() as u8
        };
        Rgb::new(
            blend(self.r, other.r),
            blend(self.g, other.g),
            blend(self.b, other.b),
        )
    }
}

/// The system's own colour preferences.
///
/// Just the accent so far, and it has to be read natively because the web view
/// cannot read it for itself. Both engines answer the CSS `AccentColor` keyword
/// with a hardcoded blue — WebKit's `RenderTheme.cpp` returns
/// `SRGBA<uint8_t>{0, 122, 255}` deliberately, as an anti-fingerprinting
/// measure, and Blink's `LayoutTheme::GetAccentColorOrDefault` returns
/// `#0075ff` unless `can_expose_accent_color`, which is true only for an
/// installed web app in the browser's initial profile and never for a WebView2
/// host. Both still report `true` from `@supports (color: AccentColor)`, so
/// there is no feature test that could tell the difference either.
pub trait Appearance: Send + Sync {
    /// The accent colour, or `None` where the platform has none to give or the
    /// query failed.
    ///
    /// Cheap and non-blocking: implementations serve a cached value that the
    /// change notification keeps current, rather than re-entering the system —
    /// on macOS the read has main-thread affinity and the command path must
    /// never wait on a run loop it might be blocking itself.
    fn accent(&self) -> Option<Rgb>;

    /// Call `handler` on every subsequent accent change, until this object is
    /// dropped.
    ///
    /// The new colour is handed over rather than looked up again by the
    /// handler: the notification arrives on a thread of the system's choosing,
    /// and re-reading from there is the part with threading rules attached.
    fn on_accent_change(&self, handler: Box<dyn Fn(Rgb) + Send + Sync>);
}

/// Bundle of every platform capability, so the orchestrator takes one argument
/// and tests can substitute the whole set.
pub struct Platform {
    pub foreground: std::sync::Arc<dyn ForegroundApp>,
    pub injector: std::sync::Arc<dyn TextInjector>,
    pub screen: std::sync::Arc<dyn ScreenText>,
    pub media: std::sync::Arc<dyn MediaControl>,
    pub permissions: std::sync::Arc<dyn Permissions>,
}

/// How long to wait for an accessibility query before giving up.
///
/// Unbounded AX and UIA calls are the classic way to hang a dictation app: a
/// wedged target process would otherwise block the recording path forever.
pub const ACCESSIBILITY_TIMEOUT: Duration = Duration::from_millis(500);

/// Delay before restoring the user's clipboard, long enough for the target app
/// to have consumed the paste.
pub const CLIPBOARD_RESTORE_DELAY: Duration = Duration::from_millis(250);

#[cfg(test)]
mod tests {
    use super::*;

    /// Both platforms' stock accent, and the two pale ones that made the
    /// contrast pick necessary.
    const MACOS_BLUE: Rgb = Rgb::new(0x00, 0x7a, 0xff);
    const WINDOWS_BLUE: Rgb = Rgb::new(0x00, 0x78, 0xd4);
    const MACOS_YELLOW: Rgb = Rgb::new(0xff, 0xc6, 0x00);
    const WINDOWS_LIME: Rgb = Rgb::new(0xa0, 0xd6, 0x10);

    fn close(got: f64, want: f64) -> bool {
        (got - want).abs() < 0.001
    }

    #[test]
    fn luminance_matches_the_wcag_reference_points() {
        assert!(close(Rgb::new(0, 0, 0).relative_luminance(), 0.0));
        assert!(close(Rgb::new(255, 255, 255).relative_luminance(), 1.0));
        // Mid grey is 0.2159, not 0.5: the transfer function is not linear.
        assert!(close(Rgb::new(128, 128, 128).relative_luminance(), 0.2159));
    }

    #[test]
    fn a_dark_accent_takes_white_text() {
        for accent in [MACOS_BLUE, WINDOWS_BLUE, Rgb::new(0, 0, 0)] {
            assert_eq!(
                accent.foreground(),
                ACCENT_TEXT_ON_DARK,
                "{} should carry white text",
                accent.to_hex()
            );
        }
    }

    #[test]
    fn a_light_accent_takes_dark_text() {
        for accent in [MACOS_YELLOW, WINDOWS_LIME, Rgb::new(255, 255, 255)] {
            assert_eq!(
                accent.foreground(),
                ACCENT_TEXT_ON_LIGHT,
                "{} should carry dark text",
                accent.to_hex()
            );
        }
    }

    /// The pick is a threshold on luminance, so it must be monotonic: no
    /// darker colour may be given dark text while a lighter one gets white.
    #[test]
    fn the_pick_is_monotonic_in_luminance() {
        let ramp: Vec<Rgb> = (0..=51)
            .map(|step| Rgb::new(step * 5, step * 5, step * 5))
            .collect();
        let flips = ramp
            .windows(2)
            .filter(|pair| pair[0].foreground() != pair[1].foreground())
            .count();
        assert_eq!(flips, 1, "the grey ramp should cross over exactly once");
        assert_eq!(ramp[0].foreground(), ACCENT_TEXT_ON_DARK);
        assert_eq!(ramp[ramp.len() - 1].foreground(), ACCENT_TEXT_ON_LIGHT);
    }

    #[test]
    fn hex_is_lowercase_and_zero_padded() {
        assert_eq!(Rgb::new(0x0a, 0x6c, 0xff).to_hex(), "#0a6cff");
        assert_eq!(Rgb::new(0, 0, 0).to_hex(), "#000000");
    }

    /// The hover shades replace two literals that were in `app.css` before the
    /// accent became the user's, so they have to land on those literals for the
    /// stock accent or every button changes colour for no reason.
    #[test]
    fn the_hover_shades_reproduce_the_stock_palette() {
        assert_eq!(Rgb::new(0x0a, 0x6c, 0xff).darker().to_hex(), "#095fe0");
        assert_eq!(Rgb::new(0x3b, 0x82, 0xf6).lighter().to_hex(), "#629bf8");
    }

    #[test]
    fn the_hover_shades_move_the_right_way_and_stay_in_gamut() {
        for accent in [MACOS_BLUE, WINDOWS_BLUE, MACOS_YELLOW, WINDOWS_LIME] {
            assert!(
                accent.darker().relative_luminance() < accent.relative_luminance(),
                "{} should darken",
                accent.to_hex()
            );
            assert!(
                accent.lighter().relative_luminance() > accent.relative_luminance(),
                "{} should lighten",
                accent.to_hex()
            );
        }
        // The endpoints are the interesting clamp: neither step may wrap.
        assert_eq!(Rgb::new(0, 0, 0).darker(), Rgb::new(0, 0, 0));
        assert_eq!(Rgb::new(255, 255, 255).lighter(), Rgb::new(255, 255, 255));
    }
}

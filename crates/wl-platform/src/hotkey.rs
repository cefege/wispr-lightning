//! Global hotkey observation.
//!
//! The requirement that rules out most crates: the app must see **press and
//! release of a bare modifier** (Left Control by default) while another
//! application has focus. `global-hotkey` and `tauri-plugin-global-shortcut`
//! cannot express that — their `HotKey` requires a non-modifier `Code`.

use crossbeam_channel::Receiver;
use wl_core::settings::Hotkey;

use crate::Result;

/// The sole global input binding.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Binding {
    Dictate,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Transition {
    Pressed,
    Released,
    /// The chord guard cancelled this hold: a modifier-only binding was down
    /// and the user typed a key, so the modifier was a shortcut modifier and
    /// not a push-to-talk trigger. See [`crate::chord`].
    ///
    /// Distinct from [`Transition::Released`] on purpose. A release means
    /// "stop and transcribe"; this means "throw the take away". The matching
    /// release is *not* delivered afterwards — the hold is over as far as the
    /// app is concerned the moment this arrives.
    Aborted,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct HotkeyEvent {
    pub binding: Binding,
    pub transition: Transition,
}

pub trait HotkeyBackend: Send + Sync {
    /// Replace the active push-to-talk alternatives.
    fn rebind(&self, dictate: &[Hotkey]) -> Result<()>;

    /// Suppress **press** events while keeping releases flowing.
    ///
    /// The asymmetry is deliberate: a recording that started before the pause
    /// must still be able to stop, otherwise pausing mid-hold wedges the app
    /// in the recording state.
    fn set_paused(&self, paused: bool);

    fn is_paused(&self) -> bool;

    /// Stream of hotkey transitions.
    fn events(&self) -> Receiver<HotkeyEvent>;

    /// Forget any latched key state, e.g. after a rebind or a pause toggle, so
    /// a physically-held key is not treated as still down.
    ///
    /// Explicitly **not** a reset of the chord guard's disarm (see
    /// [`crate::chord`]). The pipeline calls this from its abort path, which
    /// is the path a chord abort itself takes; clearing the disarm here would
    /// let the still-held Control of a `Ctrl+C` start a fresh dictation the
    /// moment the user lets go — the exact failure the guard exists to stop.
    /// The two are complementary, not the same idea: this one says "nothing
    /// is held", the disarm says "something is held and must not count".
    fn reset(&self);

    /// Divert raw input into a capture buffer instead of matching bindings,
    /// so the settings UI can record a new binding.
    ///
    /// This has to live in the backend rather than in the webview: on macOS
    /// the Fn key never reaches a web `keydown`, so a webview-side capture
    /// could not record the very default the app ships with. Presses observed
    /// while capturing MUST NOT produce [`HotkeyEvent`]s — otherwise binding a
    /// key would start a recording.
    fn begin_capture(&self);

    /// Stop capturing and return the chord observed, if any.
    ///
    /// `None` means nothing usable was pressed, which the UI should treat as
    /// "cancelled" rather than "clear the binding".
    fn end_capture(&self) -> Option<Hotkey>;

    /// Whether the OS is actually delivering events.
    ///
    /// Both platforms can silently stop: macOS disables an unresponsive event
    /// tap, and Windows removes a low-level hook that exceeds
    /// `LowLevelHooksTimeout` **without any notification**. Without a liveness
    /// probe the app looks healthy and simply never responds to the hotkey,
    /// which is exactly the failure mode the Swift version shipped with.
    fn is_healthy(&self) -> bool;
}

//! Text injection: clipboard paste, natural-mode typing, and read-back.
//!
//! The two strategies mirror `TextInjector.swift` exactly — they are chosen by
//! a setting, not chained as fallbacks — with the Windows details that the
//! Swift version has no analogue for:
//!
//! * Every synthesized event carries [`SYNTHETIC_TAG`] in `dwExtraInfo` and
//!   arms [`begin_synthetic_input`], so a Ctrl+V paste cannot retrigger a
//!   Control push-to-talk binding.
//! * `SendInput` failing because the foreground window is elevated is
//!   *silent*: neither the return value nor `GetLastError` names UIPI. The
//!   only signal is a short count, so every batch is checked against its
//!   length and the resulting error says what actually happened.
//! * Nothing is ever read back to confirm a paste landed. The accessibility
//!   check that used to do that was deleted upstream in B-001 for
//!   false-negativing on essentially every dictation; see
//!   [`crate::TextInjector::inject`].

use std::cell::Cell;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use windows::Win32::UI::Input::KeyboardAndMouse::{
    GetAsyncKeyState, SendInput, INPUT, INPUT_0, INPUT_KEYBOARD, KEYBDINPUT, KEYBD_EVENT_FLAGS,
    KEYEVENTF_KEYUP, KEYEVENTF_UNICODE, VIRTUAL_KEY, VK_CONTROL, VK_LMENU, VK_LSHIFT, VK_LWIN,
    VK_RETURN, VK_RMENU, VK_RSHIFT, VK_RWIN, VK_TAB,
};
use windows::Win32::UI::WindowsAndMessaging::{GetForegroundWindow, GetWindowThreadProcessId};

use super::keystrokes::{to_utf16_events, Pacer};
use super::{begin_synthetic_input, clipboard, uia, SYNTHETIC_GUARD, SYNTHETIC_TAG};
use crate::typing::{drive_typing, TypingStop};
use crate::{
    ClipboardSnapshot, InjectMode, PlatformError, Result, TextInjector, ACCESSIBILITY_TIMEOUT,
    CLIPBOARD_RESTORE_DELAY,
};

/// Virtual-key codes for `V` and `Z`; letter keys use their ASCII codes.
const VK_LETTER_V: VIRTUAL_KEY = VIRTUAL_KEY(0x56);
const VK_LETTER_Z: VIRTUAL_KEY = VIRTUAL_KEY(0x5A);

/// Modifiers that would corrupt a synthesized Ctrl+V if the user
/// happened to be holding them: Ctrl+Shift+V is "paste without formatting" in
/// half the world's editors and "open devtools" in the other half.
///
/// Side-specific, not the `VK_SHIFT`/`VK_MENU` aliases: `SendInput` resolves
/// an alias to the left-hand key, so a key-up for `VK_SHIFT` would leave a
/// held Right Shift exactly where it was.
const AMBIENT_MODIFIERS: [VIRTUAL_KEY; 6] =
    [VK_LSHIFT, VK_RSHIFT, VK_LMENU, VK_RMENU, VK_LWIN, VK_RWIN];

/// Pause before the first synthesized event. The Swift comment is "to ensure
/// hotkey release is fully processed", and the same applies here: the hook
/// thread is still draining the release that triggered this injection.
const HOTKEY_SETTLE: Duration = Duration::from_millis(10);

#[derive(Default)]
pub struct WindowsInjector {
    /// Raised by [`TextInjector::cancel_typing`] from the Escape watcher and
    /// read between characters by the typing loop on its worker thread.
    cancel: AtomicBool,
}

impl WindowsInjector {
    pub fn new() -> Self {
        Self::default()
    }
}

impl TextInjector for WindowsInjector {
    fn inject(&self, text: &str, mode: InjectMode) -> Result<()> {
        if text.is_empty() {
            return Ok(());
        }
        std::thread::sleep(HOTKEY_SETTLE);
        match mode {
            InjectMode::Paste => paste(text),
            InjectMode::Natural { chars_per_second } => {
                // Clear first: an Escape pressed while the *previous* pass was
                // running must not cancel this one before its first character.
                self.cancel.store(false, Ordering::Release);
                type_out(text, chars_per_second, &self.cancel)
            }
        }
    }

    fn cancel_typing(&self) {
        self.cancel.store(true, Ordering::Release);
    }

    fn undo_last_injection(&self) -> Result<()> {
        // Goes through `send_shortcut` rather than a bare pair of events so it
        // gets the same two protections as every other chord we synthesize:
        // the armed window that stops our own hook seeing it, and the release
        // of any ambient Shift or Alt that would turn Ctrl+Z into something
        // else entirely (Ctrl+Shift+Z is redo in most editors).
        send_shortcut(VK_LETTER_Z)
    }

    /// Text of the focused control, for the transcription backend to use as
    /// context.
    ///
    /// Frequently empty, and that is not a fault: Electron and Chromium
    /// controls expose no readable value through UI Automation, and password
    /// fields are refused outright. Callers must treat context as a bonus.
    fn read_focused_text(&self) -> Vec<String> {
        match focused_text() {
            Some(text) if !text.is_empty() => vec![text],
            _ => Vec::new(),
        }
    }

    fn snapshot_clipboard(&self) -> Result<ClipboardSnapshot> {
        Ok(ClipboardSnapshot(Box::new(clipboard::snapshot()?)))
    }

    fn restore_clipboard(&self, snapshot: ClipboardSnapshot) -> Result<()> {
        match snapshot.0.downcast::<clipboard::Snapshot>() {
            Ok(snapshot) => clipboard::restore(*snapshot),
            Err(_) => Err(PlatformError::Clipboard(
                "clipboard snapshot came from a different platform backend".into(),
            )),
        }
    }
}

// ---------------------------------------------------------------------------
// Paste
// ---------------------------------------------------------------------------

/// Strategy A: set the clipboard, press Ctrl+V, restore.
///
/// Nothing is read back. `SendInput` cannot tell us whether the foreground
/// window consumed the Ctrl+V, and the UI Automation read-back that used to
/// stand in for that answer was deleted upstream in B-001: it false-negatived
/// on essentially every dictation, because the controls people dictate into —
/// chat composers, browsers, terminals, editors — expose no readable value.
fn paste(text: &str) -> Result<()> {
    // A failed snapshot is not fatal — the Swift version pastes over an
    // unreadable pasteboard too — but it does mean there is nothing to put
    // back, so the restore below will simply clear the transcript away.
    let mut snapshot = clipboard::snapshot().unwrap_or_else(|e| {
        tracing::warn!(error = %e, "could not snapshot the clipboard before pasting");
        clipboard::Snapshot::default()
    });

    if let Err(e) = clipboard::set_text(text, &mut snapshot) {
        clipboard::restore(snapshot)?;
        return Err(e);
    }

    let outcome = send_shortcut(VK_LETTER_V);
    // DV4: on every path, including the one where synthesis failed.
    clipboard::schedule_restore(snapshot, CLIPBOARD_RESTORE_DELAY);
    outcome
}

// ---------------------------------------------------------------------------
// Natural mode
// ---------------------------------------------------------------------------

/// The keys a control character is typed as — modifiers first, the key itself
/// last — or `None` when the character is ordinary text.
///
/// Newline is **Shift+Return**, not Return. A bare Return submits in every
/// chat composer — Slack, Discord, Teams, the ChatGPT and Claude Code prompts
/// — and executes in a shell, so dictating a paragraph break would send the
/// message half-written. Shift+Return is the near-universal "newline without
/// submit" convention. Raw shells submit on either form; that is a known
/// limitation with no better answer. Tab carries no modifier: Shift+Tab moves
/// focus backwards, which is the opposite of typing one.
///
/// Side-specific `VK_LSHIFT` rather than the `VK_SHIFT` alias, for the same
/// reason as [`AMBIENT_MODIFIERS`]: `SendInput` resolves the alias to the
/// left-hand key on the way down but the key-up would not necessarily match.
fn control_key_chord(ch: char) -> Option<&'static [VIRTUAL_KEY]> {
    match ch {
        '\n' => Some(&[VK_LSHIFT, VK_RETURN]),
        '\t' => Some(&[VK_TAB]),
        _ => None,
    }
}

/// The process id owning the foreground window, or `None` when there is none —
/// during a desktop switch, or while a secure-desktop prompt is up.
fn foreground_pid() -> Option<i32> {
    // SAFETY: no preconditions. A null handle is a documented result and
    // `GetWindowThreadProcessId` rejects it by returning zero, which is the
    // case handled below.
    let window = unsafe { GetForegroundWindow() };
    let mut pid = 0u32;
    // SAFETY: `pid` is a live, correctly typed slot; the handle is either a
    // live window or null.
    let thread = unsafe { GetWindowThreadProcessId(window, Some(&mut pid)) };
    (thread != 0).then_some(pid as i32)
}

/// Type `text` one character at a time with human-like timing.
///
/// Control characters go out as real key presses because that is what
/// applications listen for; everything else goes out as UTF-16 through
/// `KEYEVENTF_UNICODE`, which ignores the active keyboard layout and so needs
/// none of the `UCKeyTranslate` reverse-mapping the macOS version builds.
///
/// Stops early when `cancel` is raised or the foreground application changes.
/// Neither is an error: the characters that went out went out.
fn type_out(text: &str, chars_per_second: f64, cancel: &AtomicBool) -> Result<()> {
    let total = text.chars().count();
    let mut pacer = Pacer::from_clock();
    // Drawn with the hold but consumed by the pause that follows the key, so
    // it has to survive between the two closures.
    let gap = Cell::new(Duration::ZERO);

    let (typed, stop) = drive_typing(
        text,
        &|| cancel.load(Ordering::Acquire),
        &foreground_pid,
        &mut |ch| {
            gap.set(pacer.delay(chars_per_second));
            post_character(ch, pacer.hold())
        },
        &mut || std::thread::sleep(gap.get()),
    )?;

    match stop {
        TypingStop::Completed => {}
        TypingStop::Cancelled => {
            tracing::debug!(typed, total, "typing cancelled by Escape");
        }
        TypingStop::FocusMoved { from, to } => {
            tracing::warn!(
                typed,
                total,
                ?from,
                ?to,
                "focus changed mid-typing; stopped"
            );
        }
    }
    Ok(())
}

/// Synthesize one character, holding whatever key it needs for `hold`.
fn post_character(ch: char, hold: Duration) -> Result<()> {
    if let Some(chord) = control_key_chord(ch) {
        // Released in reverse, so the modifier outlives the key it modifies.
        let down: Vec<INPUT> = chord.iter().map(|key| key_event(*key, false)).collect();
        let up: Vec<INPUT> = chord
            .iter()
            .rev()
            .map(|key| key_event(*key, true))
            .collect();
        return press_and_release(&down, &up, hold);
    }

    let mut buffer = [0u8; 4];
    let units = to_utf16_events(ch.encode_utf8(&mut buffer));
    // Both halves of a surrogate pair must reach Windows in one batch or the
    // composition is lost, so the down events go together and the up events go
    // together.
    let down: Vec<INPUT> = units.iter().map(|u| unicode_event(*u, false)).collect();
    let up: Vec<INPUT> = units.iter().map(|u| unicode_event(*u, true)).collect();
    press_and_release(&down, &up, hold)
}

fn press_and_release(down: &[INPUT], up: &[INPUT], hold: Duration) -> Result<()> {
    send_batch(down)?;
    std::thread::sleep(hold);
    send_batch(up)
}

// ---------------------------------------------------------------------------
// Event synthesis
// ---------------------------------------------------------------------------

fn key_event(key: VIRTUAL_KEY, up: bool) -> INPUT {
    keyboard_input(
        key,
        0,
        if up {
            KEYEVENTF_KEYUP
        } else {
            KEYBD_EVENT_FLAGS(0)
        },
    )
}

fn unicode_event(unit: u16, up: bool) -> INPUT {
    let flags = if up {
        KEYEVENTF_UNICODE | KEYEVENTF_KEYUP
    } else {
        KEYEVENTF_UNICODE
    };
    // `wVk` must be zero for a unicode event; the code unit rides in `wScan`.
    keyboard_input(VIRTUAL_KEY(0), unit, flags)
}

fn keyboard_input(key: VIRTUAL_KEY, scan: u16, flags: KEYBD_EVENT_FLAGS) -> INPUT {
    INPUT {
        r#type: INPUT_KEYBOARD,
        Anonymous: INPUT_0 {
            ki: KEYBDINPUT {
                wVk: key,
                wScan: scan,
                dwFlags: flags,
                time: 0,
                dwExtraInfo: SYNTHETIC_TAG,
            },
        },
    }
}

/// Send one Ctrl+`key` chord as a single atomic batch.
///
/// Any ambient Shift/Alt/Win the user happens to be holding is released first
/// and deliberately *not* restored: Windows reconciles the state on their next
/// real keystroke, whereas a Ctrl+Shift+V would have gone somewhere we cannot
/// undo.
fn send_shortcut(key: VIRTUAL_KEY) -> Result<()> {
    let mut batch: Vec<INPUT> = AMBIENT_MODIFIERS
        .iter()
        .filter(|modifier| is_physically_down(**modifier))
        .map(|modifier| key_event(*modifier, true))
        .collect();
    batch.extend([
        key_event(VK_CONTROL, false),
        key_event(key, false),
        key_event(key, true),
        key_event(VK_CONTROL, true),
    ]);
    send_batch(&batch)
}

/// `GetAsyncKeyState`'s high bit means "currently down".
fn is_physically_down(key: VIRTUAL_KEY) -> bool {
    // SAFETY: no preconditions. Not called from inside a hook callback, where
    // Microsoft documents the async state as not yet updated.
    unsafe { GetAsyncKeyState(key.0 as i32) as u16 & 0x8000 != 0 }
}

fn send_batch(inputs: &[INPUT]) -> Result<()> {
    if inputs.is_empty() {
        return Ok(());
    }
    // Armed *before* the send: the events reach the low-level hook almost
    // immediately, and the pump must already be ignoring them.
    begin_synthetic_input(SYNTHETIC_GUARD);
    // SAFETY: `inputs` is a valid slice of initialised `INPUT` values and the
    // size argument matches the type the slice is made of.
    let sent = unsafe { SendInput(inputs, std::mem::size_of::<INPUT>() as i32) } as usize;
    if sent != inputs.len() {
        return Err(PlatformError::InputSynthesis(format!(
            "SendInput delivered {sent} of {} events; the foreground window is \
             probably elevated, which blocks input from an unelevated app (UIPI)",
            inputs.len()
        )));
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Reading the focused control
// ---------------------------------------------------------------------------

/// Text of the focused control, via UI Automation.
///
/// `TextPattern` first (rich edit controls, browsers, Office), `ValuePattern`
/// second (plain edits). Password fields are refused outright rather than
/// shipped to a transcription backend.
fn focused_text() -> Option<String> {
    uia::with_uia("focused-text", ACCESSIBILITY_TIMEOUT, |automation| {
        let element = automation.get_focused_element().ok()?;
        if element.is_password().unwrap_or(false) {
            tracing::debug!("focused control is a password field; not reading it");
            return None;
        }
        if let Ok(pattern) = element.get_pattern::<uiautomation::patterns::UITextPattern>() {
            if let Some(text) = pattern
                .get_document_range()
                .and_then(|range| range.get_text(-1))
                .ok()
                .filter(|text| !text.is_empty())
            {
                return Some(text);
            }
        }
        element
            .get_pattern::<uiautomation::patterns::UIValuePattern>()
            .ok()?
            .get_value()
            .ok()
            .filter(|text| !text.is_empty())
    })
}

/// Inject an inert keystroke so the hotkey pump can confirm its hook is alive.
///
/// Lives here rather than in `hotkey.rs` because it must carry the same
/// `dwExtraInfo` tag and go through the same short-count check as every other
/// event we synthesize.
pub(crate) fn send_probe_keystroke() -> Result<()> {
    const VK_F24: VIRTUAL_KEY = VIRTUAL_KEY(0x87);
    send_batch(&[key_event(VK_F24, false), key_event(VK_F24, true)])
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Shift is the whole fix: a bare Return sends the message in every chat
    /// composer, so a dictated paragraph break would post half a sentence.
    #[test]
    fn a_newline_is_typed_as_shift_return() {
        let chord = control_key_chord('\n').expect("newline is a control key");
        assert!(
            chord.contains(&VK_LSHIFT),
            "bare Return submits mid-dictation"
        );
        assert_eq!(
            chord.last(),
            Some(&VK_RETURN),
            "the modified key must come last so it is released first"
        );
    }

    /// Tab is deliberately unmodified — Shift+Tab moves focus backwards, which
    /// is the opposite of typing one.
    #[test]
    fn a_tab_is_typed_unmodified() {
        assert_eq!(control_key_chord('\t'), Some(&[VK_TAB][..]));
    }

    /// Anything else has to fall through to the unicode path, or every
    /// ordinary character would be typed as a Return.
    #[test]
    fn ordinary_characters_are_not_control_keys() {
        for ch in ['a', ' ', '\r', '\u{1F600}', '.', '\u{0}'] {
            assert_eq!(control_key_chord(ch), None, "{ch:?} claimed a control key");
        }
    }

    /// Cmd+Z's counterpart must be the letter Z, not a scan code that happens
    /// to sit where Z does on a US layout.
    #[test]
    fn undo_presses_the_letter_z() {
        assert_eq!(VK_LETTER_Z, VIRTUAL_KEY(u16::from(b'Z')));
    }
}

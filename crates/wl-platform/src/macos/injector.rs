//! Text injection at the caret, plus the accessibility reads the rest of the
//! crate needs.
//!
//! Two strategies, chosen by the caller and never chained (§3 of the platform
//! spec): clipboard + synthetic Cmd+V, or character-by-character synthesis
//! with human timing. The single exception is a failure to create the isolated
//! event source for Natural Mode, which falls back to pasting.
//!
//! Everything posted from here is marked as ours twice over, mirroring the
//! Windows injector: the event source carries [`SYNTHETIC_USER_DATA`], and
//! every burst arms [`begin_synthetic_input`] so the hotkey listener ignores
//! the keystrokes we are about to make. Without the second one a transcript
//! containing a space retriggers a Space push-to-talk binding, and the flags
//! we pin on each event corrupt the listener's tracked modifier state.

use std::cell::Cell;
use std::collections::HashMap;
use std::ptr::NonNull;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Once};
use std::thread;
use std::time::Duration;

use objc2::MainThreadMarker;
use objc2_core_graphics::{
    CGEvent, CGEventFlags, CGEventSource, CGEventSourceStateID, CGEventTapLocation,
};
use parking_lot::Mutex;
use rand::Rng;
use tracing::{debug, warn};

use super::clipboard::{self, PasteboardSnapshot};
use super::main_thread;
use super::{begin_synthetic_input, SYNTHETIC_GUARD, SYNTHETIC_USER_DATA};
use crate::typing::{drive_typing, TypingStop};
use crate::{
    ClipboardSnapshot, InjectMode, PlatformError, Result, TextInjector, CLIPBOARD_RESTORE_DELAY,
};

/// Virtual key code for `V`; layout-independent, matching the paste shortcut.
const VK_V: u16 = 9;
const VK_RETURN: u16 = 36;
const VK_TAB: u16 = 48;
/// `Z`, for the Cmd+Z of [`TextInjector::undo_last_injection`].
const VK_Z: u16 = 6;
/// The carrier key for `CGEventKeyboardSetUnicodeString`; its code is ignored
/// once a unicode string is attached, but it must be a valid one.
const VK_UNICODE_CARRIER: u16 = 0;

/// Let the hotkey release finish dispatching before synthesizing anything.
///
/// Without it the target app can still be processing the modifier-up when our
/// Cmd+V arrives, and the paste lands with a stray Control held.
const HOTKEY_RELEASE_SETTLE: Duration = Duration::from_millis(10);

/// Per-element deadline for the focused-text ladder.
///
/// Tighter than the process-wide [`crate::ACCESSIBILITY_TIMEOUT`] on purpose:
/// the ladder makes up to eight round trips (four attributes on the focused
/// element, then the same four on its parent), and this is the multiplier that
/// turns a wedged target process into a bounded 400 ms rather than a stalled
/// recording. The context it produces is a hint the backend may ignore, so it
/// is never worth waiting longer for.
const FOCUS_TEXT_TIMEOUT: Duration = Duration::from_millis(50);

// ---------------------------------------------------------------------------
// Accessibility
// ---------------------------------------------------------------------------

/// Thin, timeout-bounded wrappers over the AX client API.
///
/// Every call here is synchronous IPC into another process. Without
/// `AXUIElementSetMessagingTimeout` a single wedged application blocks the
/// dictation pipeline for the framework default of about six seconds — which
/// is longer than most dictations.
///
/// # Threading
///
/// Every call below runs on a worker, deliberately. Apple states no threading
/// contract for the AX client API anywhere: `AXUIElement.h` is silent, and so
/// is the reference documentation for each function. The only Apple-sourced
/// claim is a developer-forums post relaying a DTS email — "all Accessibility
/// functions are only safe to call from an application's main thread" — which
/// is second-hand, undated and contradicted by the shape of the API itself:
/// `AXUIElementSetMessagingTimeout` exists because these calls block for up to
/// six seconds by default, and `find_url` in `appinfo` can spend nearly two
/// hundred of them in a row. Moving that onto the main thread would freeze the
/// UI for exactly as long as the queries take, so the timeout is the mitigation
/// and the worker is the right place. Recorded here because it is a judgement
/// call, not a fact.
pub(super) mod ax {
    use super::*;
    use objc2_application_services::{AXError, AXUIElement};
    use objc2_core_foundation::{CFAttributedString, CFRetained, CFString, CFType, CFURL};

    pub(crate) const FOCUSED_UI_ELEMENT: &str = "AXFocusedUIElement";
    pub(crate) const FOCUSED_WINDOW: &str = "AXFocusedWindow";
    pub(crate) const VALUE: &str = "AXValue";
    /// Browsers publish the address of the current page here — Chromium on
    /// the window itself, WebKit only on the web area inside it.
    pub(crate) const URL: &str = "AXURL";
    pub(crate) const CHILDREN: &str = "AXChildren";
    pub(crate) const PARENT: &str = "AXParent";
    /// What the user has highlighted. Some editors expose this even when
    /// `AXValue` is unset.
    pub(crate) const SELECTED_TEXT: &str = "AXSelectedText";
    /// The greyed-out prompt of an empty field. Worth having: an empty
    /// composer's placeholder still says which app the user is dictating into.
    pub(crate) const PLACEHOLDER_VALUE: &str = "AXPlaceholderValue";
    /// Rich-text controls that publish nothing else often publish this, as a
    /// `CFAttributedString`. There is no Carbon constant for it — the name is
    /// the whole API.
    pub(crate) const ATTRIBUTED_DESCRIPTION: &str = "AXAttributedDescription";

    /// Bound how long one element's queries may block this thread.
    ///
    /// Overrides the process default from [`ensure_timeout`] for this element
    /// alone, which is how a latency-sensitive read can be stricter than the
    /// rest of the crate without making everything else stricter too.
    pub(crate) fn set_timeout(element: &AXUIElement, timeout: Duration) {
        // SAFETY: `element` is live and the timeout is a plain float; the call
        // has no other preconditions.
        let err = unsafe { element.set_messaging_timeout(timeout.as_secs_f32()) };
        if err != AXError::Success {
            debug!(?err, "could not bound the accessibility messaging timeout");
        }
    }

    /// Apply the process-wide messaging deadline once.
    ///
    /// Apple documents setting it on the system-wide element as establishing
    /// the default for every element this process creates, so one call covers
    /// the application elements built later for browser-URL reads too.
    fn ensure_timeout() {
        static ONCE: Once = Once::new();
        ONCE.call_once(|| {
            let sys = system_wide();
            // SAFETY: `sys` is a live system-wide element and the timeout is a
            // plain float; the call has no other preconditions.
            let err =
                unsafe { sys.set_messaging_timeout(crate::ACCESSIBILITY_TIMEOUT.as_secs_f32()) };
            if err != AXError::Success {
                warn!(?err, "could not set the accessibility messaging timeout");
            }
        });
    }

    pub(crate) fn system_wide() -> CFRetained<AXUIElement> {
        // SAFETY: no preconditions; always returns a valid +1 element.
        unsafe { AXUIElement::new_system_wide() }
    }

    pub(crate) fn application(pid: i32) -> CFRetained<AXUIElement> {
        // SAFETY: no preconditions; an element for a dead pid simply fails
        // every subsequent query.
        unsafe { AXUIElement::new_application(pid) }
    }

    /// Read one attribute, or `None` if it is absent, unreadable or timed out.
    fn copy(element: &AXUIElement, attribute: &str) -> Option<CFRetained<CFType>> {
        ensure_timeout();
        let name = CFString::from_str(attribute);
        let mut out: *const CFType = std::ptr::null();
        // SAFETY: `out` is a live, correctly typed slot. AX only writes a +1
        // reference into it when it returns Success, which is the only case in
        // which we read it back.
        let err = unsafe { element.copy_attribute_value(&name, NonNull::from(&mut out)) };
        if err != AXError::Success {
            return None;
        }
        // SAFETY: on Success the slot holds a +1 CFType we now own.
        NonNull::new(out.cast_mut()).map(|p| unsafe { CFRetained::from_raw(p) })
    }

    /// Read an attribute as text, accepting either a plain or an attributed
    /// string.
    ///
    /// `AXAttributedDescription` and, in a few controls, `AXValue` itself hand
    /// back a `CFAttributedString`; a plain `CFString` downcast quietly loses
    /// exactly the rich-text fields the focused-text ladder exists to reach.
    /// Empty is treated as absent so the ladder keeps climbing.
    pub(crate) fn copy_string(element: &AXUIElement, attribute: &str) -> Option<String> {
        let value = copy(element, attribute)?;
        let text = match value.downcast::<CFString>() {
            Ok(string) => string.to_string(),
            Err(other) => other
                .downcast::<CFAttributedString>()
                .ok()?
                .string()?
                .to_string(),
        };
        (!text.is_empty()).then_some(text)
    }

    pub(crate) fn copy_element(
        element: &AXUIElement,
        attribute: &str,
    ) -> Option<CFRetained<AXUIElement>> {
        copy(element, attribute)?.downcast::<AXUIElement>().ok()
    }

    /// The element's direct children, empty when it has none or refuses.
    pub(crate) fn children(element: &AXUIElement) -> Vec<CFRetained<AXUIElement>> {
        let Some(value) = copy(element, CHILDREN) else {
            return Vec::new();
        };
        let Ok(array) = value.downcast::<objc2_core_foundation::CFArray>() else {
            return Vec::new();
        };
        (0..array.count())
            .filter_map(|index| {
                // SAFETY: `index` is in range, and the AXChildren array holds
                // AXUIElements borrowed from the still-live array.
                let raw = unsafe { array.value_at_index(index) };
                NonNull::new(raw.cast_mut()).map(|p| {
                    // SAFETY: the element is borrowed from the array, so it
                    // must be retained to outlive it.
                    unsafe { CFRetained::retain(p.cast::<AXUIElement>()) }
                })
            })
            .collect()
    }

    /// Read an attribute that holds a URL. Browsers publish `AXURL` as a
    /// `CFURL`, but some Chromium builds hand back a bare string.
    pub(crate) fn copy_url(element: &AXUIElement, attribute: &str) -> Option<String> {
        let value = copy(element, attribute)?;
        match value.downcast::<CFURL>() {
            Ok(url) => {
                // SAFETY: CFURL and NSURL are toll-free bridged, so the CFURL
                // pointer is a valid NSURL for as long as `url` is alive.
                let ns = unsafe {
                    &*(CFRetained::as_ptr(&url).as_ptr() as *const objc2_foundation::NSURL)
                };
                ns.absoluteString().map(|s| s.to_string())
            }
            Err(other) => other.downcast::<CFString>().ok().map(|s| s.to_string()),
        }
    }

    /// The control that currently has keyboard focus.
    ///
    /// Asks the frontmost application rather than the system-wide element.
    /// The system-wide route is the obvious one and is what the Swift original
    /// used, but it answers `kAXErrorCannotComplete` for any process that is
    /// not a bundled `NSApplication` — which includes the probe example and
    /// every test harness, so relying on it would mean the accessibility
    /// context silently works only in the shipped bundle. Asking the app
    /// directly works in both, and resolves to the same element whenever the
    /// frontmost app owns key focus, which is the whole dictation scenario.
    /// The system-wide element stays as a fallback.
    pub(crate) fn focused_element() -> Option<CFRetained<AXUIElement>> {
        let frontmost = objc2_app_kit::NSWorkspace::sharedWorkspace()
            .frontmostApplication()
            .map(|app| application(app.processIdentifier()));
        frontmost
            .and_then(|app| copy_element(&app, FOCUSED_UI_ELEMENT))
            .or_else(|| copy_element(&system_wide(), FOCUSED_UI_ELEMENT))
    }
}

// ---------------------------------------------------------------------------
// Natural Mode timing
// ---------------------------------------------------------------------------

/// Multiplicative jitter applied to the base inter-key delay (±40 %).
const JITTER: std::ops::Range<f64> = 0.6..1.4;
/// Key-down to key-up interval, in seconds. Long enough that fast-key
/// detectors register a press rather than dismissing it as a glitch.
const HOLD_SECONDS: std::ops::Range<f64> = 0.030..0.080;

/// Gap before the next character: `1/cps` scaled by `jitter`.
fn inter_key_delay(chars_per_second: f64, jitter: f64) -> Duration {
    Duration::from_secs_f64(jitter / chars_per_second)
}

/// Sample one character's timing: how long to hold the key, and how long to
/// wait before the next one.
fn sample_timing(rng: &mut impl Rng, chars_per_second: f64) -> (Duration, Duration) {
    let hold = Duration::from_secs_f64(rng.random_range(HOLD_SECONDS));
    let gap = inter_key_delay(chars_per_second, rng.random_range(JITTER));
    (hold, gap)
}

// ---------------------------------------------------------------------------
// Keyboard layout reverse map
// ---------------------------------------------------------------------------

/// Character to (virtual key, modifier flags) for the active keyboard layout.
type LayoutMap = HashMap<char, (u16, CGEventFlags)>;

mod layout {
    use super::*;
    use objc2_core_foundation::{CFData, CFString};

    // Carbon `Events.h` modifier masks. `UCKeyTranslate` takes the high byte
    // of the classic event modifiers, hence the `>> 8` at the use site.
    const SHIFT_KEY: u32 = 0x0200;
    const OPTION_KEY: u32 = 0x0800;

    const K_UC_KEY_ACTION_DOWN: u16 = 0;
    /// `1 << kUCKeyTranslateNoDeadKeysBit`. Dead keys would otherwise swallow
    /// the first press of an accent and produce nothing.
    const K_UC_KEY_TRANSLATE_NO_DEAD_KEYS: u32 = 1;

    /// Virtual keys 0..128 covers every physical key on every Apple layout;
    /// higher codes are media and vendor keys that produce no character.
    const MAX_VIRTUAL_KEY: u16 = 128;

    /// Modifier combinations probed for each key, in preference order.
    ///
    /// Command is deliberately absent: it suppresses character generation, so
    /// including it would map characters to chords that type nothing.
    fn combos() -> [(u32, CGEventFlags); 4] {
        [
            (0, CGEventFlags::empty()),
            (SHIFT_KEY >> 8, CGEventFlags::MaskShift),
            (OPTION_KEY >> 8, CGEventFlags::MaskAlternate),
            (
                (SHIFT_KEY | OPTION_KEY) >> 8,
                CGEventFlags::MaskShift.union(CGEventFlags::MaskAlternate),
            ),
        ]
    }

    // Text Input Sources and the Unicode key-translation routine live in
    // HIToolbox, which has no objc2 binding.
    #[link(name = "Carbon", kind = "framework")]
    extern "C" {
        fn TISCopyCurrentKeyboardLayoutInputSource() -> *mut std::ffi::c_void;
        fn TISGetInputSourceProperty(
            source: *mut std::ffi::c_void,
            key: *const CFString,
        ) -> *mut std::ffi::c_void;
        static kTISPropertyUnicodeKeyLayoutData: *const CFString;
        static kTISPropertyInputSourceID: *const CFString;
        fn LMGetKbdType() -> u8;
        fn UCKeyTranslate(
            key_layout_ptr: *const u8,
            virtual_key_code: u16,
            key_action: u16,
            modifier_key_state: u32,
            keyboard_type: u32,
            key_translate_options: u32,
            dead_key_state: *mut u32,
            max_string_length: usize,
            actual_string_length: *mut usize,
            unicode_string: *mut u16,
        ) -> i32;
    }

    extern "C" {
        fn CFRelease(cf: *const std::ffi::c_void);
    }

    /// Releases a `TISCopyCurrent…` result on every exit path.
    struct Owned(*mut std::ffi::c_void);

    impl Drop for Owned {
        fn drop(&mut self) {
            // SAFETY: `self.0` came from a Copy-rule CF function, so we own
            // exactly one reference and release it exactly once.
            unsafe { CFRelease(self.0.cast_const()) };
        }
    }

    /// Whether a `UCKeyTranslate` result is a character we can type.
    ///
    /// Rejects failures, empty results, multi-character results (ligature and
    /// dead-key output) and control characters — Return and Tab are posted as
    /// real keys instead, and nothing else below U+0020 is typeable.
    pub(super) fn accept(status: i32, units: &[u16]) -> Option<char> {
        if status != 0 || units.is_empty() {
            return None;
        }
        let text = String::from_utf16(units).ok()?;
        let mut chars = text.chars();
        let ch = chars.next()?;
        if chars.next().is_some() || (ch as u32) < 0x20 {
            return None;
        }
        Some(ch)
    }

    /// Build the reverse map for the active layout.
    ///
    /// Returns the input-source identifier alongside the map so the caller can
    /// tell when the user switched layouts and the cache is stale.
    ///
    /// Takes a [`MainThreadMarker`] because `TextInputSources.h` says so:
    /// *"TextInputSources API is not thread safe. If you are a UI application,
    /// you must call TextInputSources API on the main thread."* Demanding the
    /// proof is what keeps that from being a comment nobody reads — the only
    /// way to get one here is through [`main_thread::run`].
    pub(super) fn build(_mtm: MainThreadMarker) -> Option<(String, LayoutMap)> {
        // SAFETY: no preconditions. Returns +1 or null.
        let source = unsafe { TISCopyCurrentKeyboardLayoutInputSource() };
        if source.is_null() {
            return None;
        }
        let source = Owned(source);

        // SAFETY: `source.0` is a live TISInputSourceRef and the key is the
        // framework's own constant. The result follows the Get rule, so it is
        // borrowed for as long as the source lives.
        let id_ptr = unsafe { TISGetInputSourceProperty(source.0, kTISPropertyInputSourceID) };
        let source_id = if id_ptr.is_null() {
            String::new()
        } else {
            // SAFETY: the property is documented as a CFStringRef.
            unsafe { (*(id_ptr as *const CFString)).to_string() }
        };

        // SAFETY: as above.
        let data_ptr =
            unsafe { TISGetInputSourceProperty(source.0, kTISPropertyUnicodeKeyLayoutData) };
        if data_ptr.is_null() {
            // Input methods (Pinyin, Kotoeri) expose no `uchr` table.
            warn!("keyboard layout data unavailable; Natural Mode will type unicode events");
            return Some((source_id, LayoutMap::new()));
        }
        // SAFETY: the property is documented as a CFDataRef, borrowed from the
        // still-live input source.
        let layout_data = unsafe { &*(data_ptr as *const CFData) };
        let layout_ptr = layout_data.byte_ptr();

        // SAFETY: no preconditions.
        let keyboard_type = unsafe { LMGetKbdType() } as u32;

        let mut map = LayoutMap::new();
        for virtual_key in 0..MAX_VIRTUAL_KEY {
            for (modifier_state, flags) in combos() {
                let mut dead_key_state = 0u32;
                let mut buffer = [0u16; 4];
                let mut produced = 0usize;
                // SAFETY: `layout_ptr` is the layout blob owned by the live
                // input source; every out-parameter points at a live local and
                // `max_string_length` matches the buffer.
                let status = unsafe {
                    UCKeyTranslate(
                        layout_ptr,
                        virtual_key,
                        K_UC_KEY_ACTION_DOWN,
                        modifier_state,
                        keyboard_type,
                        K_UC_KEY_TRANSLATE_NO_DEAD_KEYS,
                        &mut dead_key_state,
                        buffer.len(),
                        &mut produced,
                        buffer.as_mut_ptr(),
                    )
                };
                let produced = produced.min(buffer.len());
                if let Some(ch) = accept(status, &buffer[..produced]) {
                    // First combination wins, so the unshifted key is preferred
                    // over a shifted one producing the same character.
                    map.entry(ch).or_insert((virtual_key, flags));
                }
            }
        }
        Some((source_id, map))
    }
}

/// The cached layout map and the input source it was built from.
struct LayoutCache {
    source_id: String,
    map: Arc<LayoutMap>,
}

/// How long to wait for the main thread to hand back a rebuilt layout map.
///
/// Injection necessarily runs on a worker — it sleeps for hundreds of
/// milliseconds — while the Text Input Sources call that builds the map is
/// main-thread-only, so the two are separated by [`main_thread::run`]. Giving
/// up is safe: the caller falls back to the cached map, or to unicode events,
/// rather than blocking dictation behind a wedged UI.
const LAYOUT_BUILD_TIMEOUT: Duration = Duration::from_millis(250);

// ---------------------------------------------------------------------------
// Injector
// ---------------------------------------------------------------------------

/// A clipboard restore that is due but has not happened yet.
///
/// The generation counter lets a second injection cancel the first one's
/// pending restore, so what comes back is the user's real clipboard — not the
/// transcript the first injection put there.
#[derive(Default)]
struct PendingRestore {
    snapshot: Option<PasteboardSnapshot>,
    generation: u64,
}

impl PendingRestore {
    /// Hand back the snapshot if `generation` is still the current one.
    fn claim(&mut self, generation: u64) -> Option<PasteboardSnapshot> {
        (self.generation == generation).then(|| self.snapshot.take())?
    }
}

pub struct MacInjector {
    /// Serializes injections. Two concurrent pastes would interleave on the
    /// one system pasteboard and restore each other's text.
    serial: Mutex<()>,
    restore: Arc<Mutex<PendingRestore>>,
    layout: Mutex<Option<LayoutCache>>,
    /// Raised by [`TextInjector::cancel_typing`] from the Escape watcher and
    /// read between characters by the typing loop on its worker thread.
    cancel: AtomicBool,
}

impl Default for MacInjector {
    fn default() -> Self {
        Self::new()
    }
}

impl MacInjector {
    pub fn new() -> Self {
        Self {
            serial: Mutex::new(()),
            restore: Arc::new(Mutex::new(PendingRestore::default())),
            layout: Mutex::new(None),
            cancel: AtomicBool::new(false),
        }
    }

    /// Take ownership of the user's clipboard, returning the generation token
    /// that must be presented to restore it.
    fn begin_restore(&self) -> u64 {
        let mut pending = self.restore.lock();
        pending.generation = pending.generation.wrapping_add(1);
        if pending.snapshot.is_none() {
            pending.snapshot = Some(clipboard::snapshot());
        }
        pending.generation
    }

    fn restore_now(&self, generation: u64) {
        if let Some(snapshot) = self.restore.lock().claim(generation) {
            debug!(items = snapshot.item_count(), "clipboard restored");
            clipboard::restore(&snapshot);
        }
    }

    /// Restore after `delay`, unless a later injection supersedes us.
    fn restore_after(&self, generation: u64, delay: Duration) {
        let restore = Arc::clone(&self.restore);
        thread::spawn(move || {
            thread::sleep(delay);
            let claimed = restore.lock().claim(generation);
            if let Some(snapshot) = claimed {
                debug!(items = snapshot.item_count(), "clipboard restored");
                clipboard::restore(&snapshot);
            }
        });
    }

    /// Strategy A: set the clipboard, press Cmd+V, restore.
    ///
    /// Nothing is read back. `CGEvent::post` cannot tell us whether the
    /// focused app consumed the Cmd+V, and the accessibility read-back that
    /// used to stand in for that answer was deleted upstream in B-001: chat
    /// composers, `contenteditable` fields, terminals and code editors do not
    /// expose `AXValue`, so it reported failure on pastes that had landed and
    /// drove spurious retries and error UI. Trust the post.
    fn paste(&self, text: &str) -> Result<()> {
        let generation = self.begin_restore();

        if !clipboard::set_string(text) {
            self.restore_now(generation);
            return Err(PlatformError::Clipboard(
                "the pasteboard refused the transcript".into(),
            ));
        }

        // The Swift original returned here without restoring, permanently
        // clobbering the user's clipboard whenever event creation failed.
        // Deviation DV4: every exit path restores.
        if let Err(err) = post_shortcut(VK_V) {
            self.restore_now(generation);
            return Err(err);
        }

        self.restore_after(generation, CLIPBOARD_RESTORE_DELAY);
        Ok(())
    }

    /// The reverse layout map for the layout that is active right now.
    fn current_layout(&self) -> Arc<LayoutMap> {
        let cached_id = self.layout.lock().as_ref().map(|c| c.source_id.clone());
        let rebuilt = main_thread::run(
            move |mtm| {
                let (source_id, map) = layout::build(mtm)?;
                if Some(&source_id) == cached_id.as_ref() {
                    // Unchanged: skip copying the map back across the hop.
                    return Some((source_id, None));
                }
                Some((source_id, Some(map)))
            },
            LAYOUT_BUILD_TIMEOUT,
        );

        let mut cache = self.layout.lock();
        match rebuilt.flatten() {
            Some((source_id, Some(map))) => {
                let map = Arc::new(map);
                *cache = Some(LayoutCache {
                    source_id,
                    map: Arc::clone(&map),
                });
                map
            }
            Some((_, None)) => cache
                .as_ref()
                .map(|c| Arc::clone(&c.map))
                .unwrap_or_default(),
            None => {
                warn!("keyboard layout unavailable; typing unicode events instead");
                cache
                    .as_ref()
                    .map(|c| Arc::clone(&c.map))
                    .unwrap_or_default()
            }
        }
    }

    /// Strategy B: synthesize each character with human timing.
    ///
    /// Returns once the text is exhausted, the Escape watcher cancels, or the
    /// user moves to another application. A pass that stops early is not an
    /// error: the characters that went out went out, and the transcript is
    /// still retired normally.
    fn type_naturally(&self, text: &str, chars_per_second: f64) -> Result<()> {
        // A private state source ignores the live hardware modifiers. Without
        // it a still-held Control from the push-to-talk key, or Caps Lock,
        // corrupts every character: `,` becomes `<`, `'` becomes `"`.
        let Some(source) = CGEventSource::new(CGEventSourceStateID::Private) else {
            warn!("could not create an isolated event source; falling back to paste");
            return self.paste(text);
        };
        // The private state ID isolates the *modifier* state, not the event's
        // identity: a private-source event still reaches every session tap
        // carrying our own PID, so it does not distinguish our keystrokes on
        // its own. The user-data tag is what does.
        CGEventSource::set_user_data(Some(&source), SYNTHETIC_USER_DATA);

        // Clear first: an Escape pressed while the *previous* pass was running
        // must not cancel this one before its first character.
        self.cancel.store(false, Ordering::Release);

        let map = self.current_layout();
        let total = text.chars().count();
        debug!(
            chars = total,
            chars_per_second,
            layout_entries = map.len(),
            "typing"
        );

        // The hold and the gap are drawn together, but consumed by two
        // different closures — the key post and the pause after it — so the
        // gap has to survive between them.
        let mut rng = rand::rng();
        let gap = Cell::new(Duration::ZERO);
        let (typed, stop) = drive_typing(
            text,
            &|| self.cancel.load(Ordering::Acquire),
            &frontmost_pid,
            &mut |ch| {
                let (hold, next) = sample_timing(&mut rng, chars_per_second);
                gap.set(next);
                match control_key_spec(ch) {
                    Some((virtual_key, flags)) => post_key(&source, virtual_key, flags, hold),
                    None => match map.get(&ch) {
                        Some(&(virtual_key, flags)) => post_key(&source, virtual_key, flags, hold),
                        None => post_unicode(&source, ch, hold),
                    },
                }
            },
            &mut || thread::sleep(gap.get()),
        )?;

        match stop {
            TypingStop::Completed => {}
            TypingStop::Cancelled => {
                debug!(typed, total, "typing cancelled by Escape");
            }
            TypingStop::FocusMoved { from, to } => {
                warn!(
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
}

impl TextInjector for MacInjector {
    fn inject(&self, text: &str, mode: InjectMode) -> Result<()> {
        if text.is_empty() {
            return Ok(());
        }
        let _serial = self.serial.lock();
        thread::sleep(HOTKEY_RELEASE_SETTLE);
        match mode {
            InjectMode::Paste => self.paste(text),
            InjectMode::Natural { chars_per_second } => self.type_naturally(text, chars_per_second),
        }
    }

    fn cancel_typing(&self) {
        self.cancel.store(true, Ordering::Release);
    }

    fn undo_last_injection(&self) -> Result<()> {
        // The HID system state, deliberately unlike Natural Mode's private
        // source: this is a one-shot system shortcut the user could have
        // pressed themselves, not a synthesized character that has to be
        // insulated from their held modifiers.
        post_shortcut(VK_Z)
    }

    /// Text around the caret, for the transcription backend to use as context.
    ///
    /// Four attributes on the focused element, then the same four on its
    /// parent, first non-empty answer wins. Widening the query past `AXValue`
    /// is worth doing but does not make this reliable: most modern targets —
    /// Slack, Cursor, terminals, web composers, document editors — publish
    /// none of the four, so an empty answer is the common case and not a
    /// fault. Callers must treat context as a bonus, never a precondition.
    fn read_focused_text(&self) -> Vec<String> {
        const LADDER: [&str; 4] = [
            ax::VALUE,
            ax::SELECTED_TEXT,
            ax::PLACEHOLDER_VALUE,
            ax::ATTRIBUTED_DESCRIPTION,
        ];

        let Some(focused) = ax::focused_element() else {
            return Vec::new();
        };
        ax::set_timeout(&focused, FOCUS_TEXT_TIMEOUT);
        if let Some(text) = LADDER.iter().find_map(|a| ax::copy_string(&focused, a)) {
            return vec![text];
        }

        // One level up only. Containers delegate their text to a child far
        // more often than the reverse, and an unbounded walk toward the window
        // would cost a round trip per ancestor for steadily less relevant text.
        let Some(parent) = ax::copy_element(&focused, ax::PARENT) else {
            return Vec::new();
        };
        ax::set_timeout(&parent, FOCUS_TEXT_TIMEOUT);
        LADDER
            .iter()
            .find_map(|a| ax::copy_string(&parent, a))
            .map(|text| vec![text])
            .unwrap_or_default()
    }

    fn snapshot_clipboard(&self) -> Result<ClipboardSnapshot> {
        Ok(ClipboardSnapshot(Box::new(clipboard::snapshot())))
    }

    fn restore_clipboard(&self, snapshot: ClipboardSnapshot) -> Result<()> {
        let snapshot = snapshot
            .0
            .downcast::<PasteboardSnapshot>()
            .map_err(|_| PlatformError::Clipboard("snapshot came from another platform".into()))?;
        clipboard::restore(&snapshot);
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// Natural Mode keys
// ---------------------------------------------------------------------------

/// The key and modifiers a control character is typed as, or `None` when the
/// character is ordinary text the layout map has to resolve.
///
/// Newline is **Shift+Return**, not Return. A bare Return submits in every
/// chat composer — Slack, Discord, the ChatGPT and Claude Code prompts — and
/// executes in a shell, so dictating a paragraph break would send the message
/// half-written. Shift+Return is the near-universal "newline without submit"
/// convention. Raw shells submit on either form; that is a known limitation
/// with no better answer. Tab carries no modifier: nothing overloads it the
/// same way.
fn control_key_spec(ch: char) -> Option<(u16, CGEventFlags)> {
    match ch {
        '\n' => Some((VK_RETURN, CGEventFlags::MaskShift)),
        '\t' => Some((VK_TAB, CGEventFlags::empty())),
        _ => None,
    }
}

/// The process id of the frontmost application.
///
/// `None` when AppKit will not answer — mid Space switch, or with no
/// application active. Compared against itself, so a `None` baseline that
/// stays `None` is correctly read as "focus has not moved".
fn frontmost_pid() -> Option<i32> {
    objc2_app_kit::NSWorkspace::sharedWorkspace()
        .frontmostApplication()
        .map(|app| app.processIdentifier())
}

// ---------------------------------------------------------------------------
// Event synthesis
// ---------------------------------------------------------------------------

/// Post Command + `virtual_key` as a real HID-level event.
fn post_shortcut(virtual_key: u16) -> Result<()> {
    // The HID system state carries the live modifier flags, which is what we
    // want here: the shortcut has to look like the user pressed it.
    let source = CGEventSource::new(CGEventSourceStateID::HIDSystemState);
    if let Some(source) = source.as_deref() {
        CGEventSource::set_user_data(Some(source), SYNTHETIC_USER_DATA);
    }
    // Armed *before* the post: the event is re-injected at the bottom of the
    // stack and reaches the tap almost immediately, so the hotkey worker has
    // to already be ignoring it.
    begin_synthetic_input(SYNTHETIC_GUARD);
    for down in [true, false] {
        let event =
            CGEvent::new_keyboard_event(source.as_deref(), virtual_key, down).ok_or_else(|| {
                PlatformError::InputSynthesis(
                    "could not create a keyboard event; Accessibility permission is required"
                        .into(),
                )
            })?;
        CGEvent::set_flags(Some(&event), CGEventFlags::MaskCommand);
        CGEvent::post(CGEventTapLocation::HIDEventTap, Some(&event));
    }
    Ok(())
}

/// Press and release one key, holding it for `hold`.
fn post_key(
    source: &CGEventSource,
    virtual_key: u16,
    flags: CGEventFlags,
    hold: Duration,
) -> Result<()> {
    // One burst per character, so this keeps extending the window for as long
    // as we are typing. The hold below is bounded well inside it, which is
    // what makes the key-up ours too.
    begin_synthetic_input(SYNTHETIC_GUARD);
    for down in [true, false] {
        let event =
            CGEvent::new_keyboard_event(Some(source), virtual_key, down).ok_or_else(|| {
                PlatformError::InputSynthesis("could not create a keyboard event".into())
            })?;
        // Pinned on both edges, even when empty: otherwise an ambient modifier
        // the user is still holding rides along and changes the character.
        CGEvent::set_flags(Some(&event), flags);
        CGEvent::post(CGEventTapLocation::HIDEventTap, Some(&event));
        if down {
            thread::sleep(hold);
        }
    }
    Ok(())
}

/// Type a character the active layout cannot reach — emoji, CJK, anything from
/// a layout the user is not currently using.
fn post_unicode(source: &CGEventSource, ch: char, hold: Duration) -> Result<()> {
    let mut utf16 = [0u16; 2];
    let units = ch.encode_utf16(&mut utf16);
    begin_synthetic_input(SYNTHETIC_GUARD);
    for down in [true, false] {
        let event = CGEvent::new_keyboard_event(Some(source), VK_UNICODE_CARRIER, down)
            .ok_or_else(|| {
                PlatformError::InputSynthesis("could not create a keyboard event".into())
            })?;
        CGEvent::set_flags(Some(&event), CGEventFlags::empty());
        // SAFETY: `units` is a live UTF-16 slice whose length is passed
        // alongside it; the event copies the string, so the buffer does not
        // need to outlive the call.
        unsafe {
            CGEvent::keyboard_set_unicode_string(Some(&event), units.len() as u64, units.as_ptr());
        }
        CGEvent::post(CGEventTapLocation::HIDEventTap, Some(&event));
        if down {
            thread::sleep(hold);
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use wl_core::settings::TypingSpeed;

    /// The three shipped speeds must stay inside the documented per-character
    /// windows: slow 240–560 ms, normal 150–350 ms, expert 92.3–215.4 ms.
    #[test]
    fn the_inter_key_delay_always_lands_inside_the_documented_band() {
        let mut rng = rand::rng();
        for speed in [TypingSpeed::Slow, TypingSpeed::Normal, TypingSpeed::Expert] {
            let cps = speed.chars_per_second();
            let base = 1.0 / cps;
            let low = Duration::from_secs_f64(base * JITTER.start);
            let high = Duration::from_secs_f64(base * JITTER.end);
            for _ in 0..10_000 {
                let (_, gap) = sample_timing(&mut rng, cps);
                assert!(
                    gap >= low && gap <= high,
                    "{speed:?}: {gap:?} outside {low:?}..{high:?}"
                );
            }
        }
    }

    #[test]
    fn the_key_hold_always_lands_between_thirty_and_eighty_milliseconds() {
        let mut rng = rand::rng();
        for _ in 0..10_000 {
            let (hold, _) = sample_timing(&mut rng, 4.0);
            assert!(
                hold >= Duration::from_millis(30) && hold <= Duration::from_millis(80),
                "{hold:?}"
            );
        }
    }

    /// A constant delay would satisfy the bounds check above while destroying
    /// the point of the jitter, so assert the samples actually spread.
    #[test]
    fn the_timing_jitter_covers_the_whole_band() {
        let mut rng = rand::rng();
        let cps = 4.0;
        let base = 1.0 / cps;
        let (mut min, mut max) = (f64::MAX, f64::MIN);
        for _ in 0..10_000 {
            let (_, gap) = sample_timing(&mut rng, cps);
            let ratio = gap.as_secs_f64() / base;
            min = min.min(ratio);
            max = max.max(ratio);
        }
        assert!(min < 0.65, "jitter never went near the low end: {min}");
        assert!(max > 1.35, "jitter never went near the high end: {max}");
    }

    #[test]
    fn a_faster_speed_produces_a_proportionally_shorter_delay() {
        assert_eq!(inter_key_delay(2.5, 1.0), Duration::from_secs_f64(0.4));
        assert_eq!(inter_key_delay(4.0, 1.0), Duration::from_secs_f64(0.25));
        assert_eq!(
            inter_key_delay(6.5, 0.6),
            Duration::from_secs_f64(0.6 / 6.5)
        );
    }

    #[test]
    fn the_layout_filter_keeps_ordinary_printable_characters() {
        assert_eq!(layout::accept(0, &[b'a' as u16]), Some('a'));
        assert_eq!(layout::accept(0, &[0x00E9]), Some('é'));
        assert_eq!(layout::accept(0, &[b' ' as u16]), Some(' '));
    }

    #[test]
    fn the_layout_filter_rejects_failures_and_untypeable_results() {
        // A non-zero OSStatus.
        assert_eq!(layout::accept(-50, &[b'a' as u16]), None);
        // Nothing produced.
        assert_eq!(layout::accept(0, &[]), None);
        // Control characters: Return and Tab are posted as real keys instead.
        assert_eq!(layout::accept(0, &[0x0D]), None);
        assert_eq!(layout::accept(0, &[0x09]), None);
        assert_eq!(layout::accept(0, &[0x00]), None);
        // More than one character — ligatures and dead-key output.
        assert_eq!(layout::accept(0, &[b'f' as u16, b'i' as u16]), None);
    }

    /// Astral characters arrive as a surrogate pair but are a single scalar,
    /// so they must survive the "exactly one character" filter.
    #[test]
    fn the_layout_filter_accepts_a_surrogate_pair_as_one_character() {
        assert_eq!(layout::accept(0, &[0xD83D, 0xDE00]), Some('\u{1F600}'));
    }

    /// A second injection arriving before the first one's restore fires must
    /// win the snapshot, or the user gets the transcript back on the clipboard
    /// instead of what they had copied.
    #[test]
    fn a_superseded_restore_does_not_fire() {
        let mut pending = PendingRestore {
            snapshot: Some(PasteboardSnapshot::default()),
            generation: 7,
        };
        assert!(pending.claim(6).is_none(), "a stale generation claimed it");
        assert!(pending.claim(7).is_some());
        assert!(pending.claim(7).is_none(), "claimed twice");
    }

    /// Shift is the whole fix: a bare Return sends the message in every chat
    /// composer, so a dictated paragraph break would post half a sentence.
    #[test]
    fn a_newline_is_typed_as_shift_return() {
        let (virtual_key, flags) = control_key_spec('\n').expect("newline is a control key");
        assert_eq!(virtual_key, 36, "kVK_Return");
        assert!(
            flags.contains(CGEventFlags::MaskShift),
            "bare Return submits mid-dictation"
        );
    }

    /// Tab is deliberately unmodified — Shift+Tab moves focus backwards, which
    /// is the opposite of typing one.
    #[test]
    fn a_tab_is_typed_unmodified() {
        let (virtual_key, flags) = control_key_spec('\t').expect("tab is a control key");
        assert_eq!(virtual_key, 48, "kVK_Tab");
        assert_eq!(flags, CGEventFlags::empty(), "Shift+Tab is not a tab");
    }

    /// Anything else has to fall through to the layout map, or every ordinary
    /// character would be typed as a Return.
    #[test]
    fn ordinary_characters_are_not_control_keys() {
        for ch in ['a', ' ', '\r', '\u{1F600}', '.', '\u{0}'] {
            assert_eq!(control_key_spec(ch), None, "{ch:?} claimed a control key");
        }
    }
}

# LibPlatformInput research



## [answer]

"# Global input + text injection on macOS & Windows (Rust / Tauri v2)

All verdicts below were verified by reading crate source (downloaded from crates.io / cloned from upstream on 2026-08-01) and Microsoft/Apple reference docs. Nothing here is from memory.

---

## 1. Modifier-only push-to-talk hotkey (press AND release of a bare modifier)

### 1a. `global-hotkey` â **NO. Cannot do modifier-only.** (verified)

**Crate**: `global-hotkey` 0.8.0 (upstream `tauri-apps/global-hotkey`; crates.io latest published 0.7.0 also identical in this respect).

The type system forbids it. `HotKey` requires exactly one non-optional `Code`:

```rust
pub struct HotKey { pub mods: Modifiers, pub key: Code, pub id: u32 }
pub fn new(mods: Option<Modifiers>, key: Code) -> Self
```

And even if you passed `Code::ControlLeft`, registration fails: I grepped the platform key tables for modifier entries and both are empty.

- macOS `key_to_scancode(code: Code) -> Option<u32>` (`src/platform_impl/macos/mod.rs:411`) has **no** `ControlLeft`/`ShiftLeft`/`AltLeft`/`MetaLeft` arm â returns `None` â `Err(FailedToRegister)`. Underlying mechanism is Carbon `RegisterEventHotKey`, which is a *combo* API and never fires for a bare modifier.
- Windows `key_to_vk(key: &Code) -> Option<VIRTUAL_KEY>` (`src/platform_impl/windows/mod.rs:204`) likewise has no modifier arm; underlying `RegisterHotKey(hwnd, id, mods, vk)` requires a non-modifier VK.

`HotKeyState::{Pressed, Released}` **does** exist, so hold-to-talk works for *real* combos (e.g. `Ctrl+Alt+Space`). Note how Released is produced on Windows â it is not a real key-up event, it's a polling thread:

```rust
// global_hotkey_proc, WM_HOTKEY branch
std::thread::spawn(move || loop {
    let state = GetAsyncKeyState(HIWORD(lparam as u32) as i32);
    if state == 0 { /* send Released */ break; }
    std::thread::sleep(Duration::from_millis(50));
});
```

â up to **50 ms release latency** and one spawned thread per press on Windows.

### 1b. Tauri `global-shortcut` plugin â **NO, same limitation.** (verified)

`tauri-plugin-global-shortcut` 2.3.1 is a thin re-export wrapper:

```rust
use global_hotkey::GlobalHotKeyEvent;
pub use global_hotkey::{ GlobalHotKeyEvent as ShortcutEvent, HotKeyState as ShortcutState, ... };
struct GlobalHotKeyManager(global_hotkey::GlobalHotKeyManager);
```

It inherits `HotKey`'s mandatory `Code`. **Your suspicion is correct: the Tauri plugin cannot do modifier-only hold-to-talk.** It *can* do hold-to-talk for a modifier+key combo (`ShortcutState::Pressed`/`Released`).

### 1c. `rdev` â **partially; do not use for modifier PTT.**

**Crate**: `rdev` 0.6.0 (`Narsil/rdev`, HEAD 2026-05-12). It *does* surface modifier key-up: `EventType::KeyRelease(Key::ControlLeft)` from `CGEventType::FlagsChanged`. Three disqualifying defects:

1. **Left/right desync.** `src/macos/common.rs:78-132` decides press-vs-release purely from the *shared* `CGEventFlags` bit (`MaskShift`, `MaskControl`, `MaskAlternate`, `MaskCommand`). Hold LeftCtrl, tap RightCtrl, release LeftCtrl â the Control mask never clears, so **no release event is emitted**. Fatal for PTT. Also has no `Fn`/CapsLock handling.
2. **Never re-enables a timed-out tap.** Grepping the whole crate for `TapDisabled` returns zero hits; the only `tap_enable` calls are at setup (`listen.rs:64`, `grab.rs:65`). A slow callback â macOS silently kills the tap â hotkey dies permanently until restart. This is the exact bug class filed against Handy (cjpais/Handy#840).
3. **API shape.** `pub fn listen<T>(callback: T) -> Result<(), ListenError>` stores the callback in a `static mut GLOBAL_CALLBACK` and then calls `CFRunLoop::run()` / `GetMessageA(...)` â it **blocks forever, one listener per process, no stop handle**.

Upstream Windows path uses `winapi` + `SetWindowsHookExA(WH_KEYBOARD_LL, ...)` and does emit `WM_KEYUP`/`WM_SYSKEYUP` â `KeyRelease`, which is fine in isolation.

### 1d. `device_query` â **works but is polling; wrong tool.**

**Crate**: `device_query` 4.0.1. `DeviceState::query_keymap() -> Vec<Keycode>` includes `Keycode::LControl` distinctly (macOS mapping at `src/device_state/macos/mod.rs:84`, Windows `VK_LCONTROL => Keycode::LControl`). Backends: Windows `GetAsyncKeyState`, macOS `readkey` â `CGEventSourceKeyState(kCGEventSourceStateCombined, keycode)`. `DeviceEventsHandler::new(sleep_dur)` spawns `keyboard_thread`/`mouse_thread` that busy-poll with `sleep(sleep_dur)`.

Consequences: latency == poll interval, missed sub-interval taps, constant CPU wakeups, and **no ability to suppress the key** from the focused app. Acceptable only as a last-ditch fallback.

### 1e. â­ Recommended: `handy-keys` â **YES, purpose-built, and this is shipping prior art.**

**Crate**: `handy-keys` 0.3.3 (`handy-computer/handy-keys`, MIT). This is the library extracted from **Handy**, an open-source Rust+Tauri v2 push-to-talk dictation app â i.e. exactly your product shape, already solved. (Handy's `src-tauri/Cargo.toml` line 77: `handy-keys = "0.3.3"`, replacing the Tauri global-shortcut plugin; see `src/shortcut/handy_keys.rs`.)

```rust
pub struct Hotkey { pub modifiers: Modifiers, pub key: Option<Key> }   // key is OPTIONAL
pub fn Hotkey::new(modifiers: Modifiers, key: impl Into<Option<Key>>) -> Result<Self>
// doc example, verbatim:  let hotkey = Hotkey::new(Modifiers::CMD | Modifiers::SHIFT, None).unwrap();
```

Side-specific modifiers are first-class â `Modifiers::CTRL_LEFT`, `CTRL_RIGHT`, plus compound `CTRL = CTRL_LEFT | CTRL_RIGHT`, and `FN`. So "Left Control alone, press and release" is `Hotkey::new(Modifiers::CTRL_LEFT, None)`.

Manager API: `HotkeyManager::new()`, `new_with_blocking()` (suppression), `register(Hotkey) -> Result<HotkeyId>`, `unregister(HotkeyId)`, `recv()/try_recv() -> HotkeyEvent { id, state: HotkeyState::{Pressed,Released} }`.

**Fallback if `handy-keys` doesn't fit**: write the ~400 lines of raw FFI yourself against `objc2-core-graphics` 0.3 (`CGEvent::tap_create`) and `windows` 0.62 (`SetWindowsHookExW`). Both are fully bound; see Â§2 for the exact requirements. Do **not** fall back to `global-hotkey`.

---

## 2. Event loop / thread requirements, and avoiding silent hook death

### macOS â CGEventTap

- **No main-thread requirement.** Apple: *"Your callback function is invoked from the run loop to which the event tap is added as a source."* Any thread works as long as **that thread runs a CFRunLoop**. `handy-keys` does exactly this: `thread::spawn(move || run_event_tap(...))`, creating the tap on the worker and returning a `TapRunLoop(CFRetained<CFRunLoop>)` handle to the caller for later `CFRunLoopStop`. This is the right shape under Tauri, whose main thread is owned by the AppKit run loop.
- Setup sequence: `CGEvent::tap_create(...)` â `CFMachPort::new_run_loop_source(None, Some(&tap), 0)` â `CFRunLoop::add_source(src, kCFRunLoopCommonModes)` â `CGEvent::tap_enable(&tap, true)` â `CFRunLoop::run()`.
- **Silent-disable mitigation (mandatory).** Handle the two pseudo-events *first thing* in the callback:

```rust
if matches!(event_type, CGEventType::TapDisabledByTimeout | CGEventType::TapDisabledByUserInput) {
    CGEvent::tap_enable(tap.as_ref(), true);
    // also re-sync tracked modifier state â events were missed while dead
    reconcile_modifiers(&mut state.current_modifiers,
        CGEventSource::flags_state(CGEventSourceStateID::CombinedSessionState));
    return event.as_ptr();
}
```

  Constants exist as `CGEventType::TapDisabledByTimeout = 0xFFFF_FFFE` and `TapDisabledByUserInput = 0xFFFF_FFFF` (`objc2-core-graphics-0.3.2/src/generated/CGEventTypes.rs:245-249`).
- â ï¸ **Do not** poll `CGEventTapIsEnabled` on a timer as your primary recovery. handy-keys documents the reason in-source: recurring WindowServer RPCs *"leak kernel IPC vouchers and eventually panic the whole machine (cjpais/Handy#1827)."* Re-enable from inside the callback. A very-low-frequency health check (e.g. on wake-from-sleep only) is the belt-and-braces addition â a tap can also go inert with no disable callback at all after a code-signing/TCC change, in which case you must destroy and recreate the tap.
- â ï¸ **State desync is the #1 PTT bug.** Any missed FlagsChanged permanently desyncs a naive modifier tracker (this is the rdev flaw). Always reconcile against `CGEventSource::flags_state(...)` on non-FlagsChanged events.
- Keep the callback trivial â push to a channel, return immediately. A slow callback stalls system-wide event delivery before it gets you killed.

### Windows â `WH_KEYBOARD_LL`

Microsoft, `LowLevelKeyboardProc` reference (verbatim):

> *"This hook is called in the context of the thread that installed it. The call is made by sending a message to the thread that installed the hook. **Therefore, the thread that installed the hook must have a message loop.**"*

> *"The hook procedure should process a message in less time than the data entry specified in the **LowLevelHooksTimeout** value in `HKEY_CURRENT_USER\Control Panel\Desktop`â¦ If the hook procedure times out, the system passes the message to the next hook. **However, on Windows 7 and later, the hook is silently removed without being called. There is no way for the application to know whether the hook is removed.**"*

> *"**Windows 10 version 1709 and later** The maximum timeout value the system allows is 1000 millisecondsâ¦ The system will default to using a 1000 millisecond timeout if LowLevelHooksTimeout is set to a value larger than 1000."*

And Microsoft's own recommendation:

> *"If the application must use low level hooks, it should **run the hooks on a dedicated thread that passes the work off to a worker thread and then immediately returns**."*

Also from `SetWindowsHookEx`: `WH_KEYBOARD_LL` is **global-scope only**, but unlike other global hooks it is *not* DLL-injected â pass `hmod: None, dwThreadId: 0` and the callback runs back in your process. No DLL needed. Note the arch caveat: a 64-bit hook is injected into 64-bit processes and 32-bit processes call back to you; keep pumping messages either way.

**Practical recipe** (what `handy-keys` does, `src/platform/windows/listener.rs`):
1. `thread::spawn` a dedicated hook thread â never the Tauri main thread.
2. `SetWindowsHookExW(WH_KEYBOARD_LL, Some(keyboard_hook_proc), None, 0)` (+ `WH_MOUSE_LL` if needed).
3. Loop on `MsgWaitForMultipleObjects(..., QS_ALLINPUT)` + `PeekMessageW(..., PM_REMOVE)` â this both pumps the hook and lets you service your own thread messages.
4. In `keyboard_hook_proc`: read `KBDLLHOOKSTRUCT`, push to a channel, `CallNextHookEx` and return. Zero allocation, zero locking, zero I/O.
5. **No API tells you the hook died.** Defend by: never blocking in the callback; keeping a message-only watcher window; and re-installing the hook on the desktop-switch / session-change signals (handy-keys' watcher wndproc handles lock 0x7 / unlock 0x8 explicitly, since the hook also stops firing while the calling thread's desktop is not the active one â lock screen, UAC secure desktop).
6. Distinguish your own synthetic keystrokes with `KBDLLHOOKSTRUCT.flags & LLKHF_INJECTED (0x10)` / `LLKHF_LOWER_IL_INJECTED (0x02)`, or by stamping `dwExtraInfo` on `SendInput` (enigo exposes `Settings::windows_dw_extra_info`). Otherwise your injected paste re-triggers your own hotkey.
7. â ï¸ MS note: `GetAsyncKeyState` is **unreliable inside the callback** â *"the callback function is called before the asynchronous state of the key is updated."* Track state yourself from the WM_KEYDOWN/WM_KEYUP stream.

---

## 3. Permissions

### macOS

Two distinct TCC services, and you likely need both:

| Need | Service | API |
|---|---|---|
| Read global keys (listen-only tap), read AX tree | Input Monitoring (`kTCCServiceListenEvent`) | `CGPreflightListenEventAccess()` / `CGRequestListenEventAccess()` |
| Suppress/modify events, AXUIElement, post events | Accessibility (`kTCCServiceAccessibility`) | `AXIsProcessTrusted()` / `AXIsProcessTrustedWithOptions()` |

**Which Rust crate exposes `AXIsProcessTrustedWithOptions`?** Three real options, verified:

1. **`objc2-application-services` 0.3.2** â *recommended*, safe-ish generated bindings, same objc2 0.6 / `objc2-core-foundation` 0.3 family Tauri v2 already pulls in:
   ```rust
   pub unsafe extern "C-unwind" fn AXIsProcessTrustedWithOptions(options: Option<&CFDictionary>) -> bool
   pub unsafe extern "C-unwind" fn AXIsProcessTrusted() -> bool
   pub static kAXTrustedCheckOptionPrompt: &'static CFString;
   ```
2. **`accessibility-sys` 0.2.0** â older `core-foundation-sys` raw style: `pub fn AXIsProcessTrustedWithOptions(options: CFDictionaryRef) -> bool` (`src/ui_element.rs:33`).
3. **`macos-accessibility-client` 0.0.1** â tiny convenience wrapper used by `device_query`: `accessibility::application_is_trusted() -> bool` and `application_is_trusted_with_prompt() -> bool` (builds the `{kAXTrustedCheckOptionPrompt: true}` dict for you).

Note `enigo` 0.6.1 does **not** re-export this â it declares its own private `extern fn AXIsProcessTrustedWithOptions` (`src/macos/macos_impl.rs:1258`) behind `Settings::open_prompt_to_get_permissions: bool`.

For posting events (your text injection), also gate on `CGPreflightPostEventAccess()`.

Gotchas: the prompt only appears for a **properly signed, bundled .app** â TCC keys on the code signature; a re-signed/updated binary re-prompts and can leave an already-created tap inert. TCC grants do **not** take effect for an already-running process reliably; plan a restart-after-grant UX.

### Windows

- **A low-level keyboard hook needs no elevation and no UIAccess for ordinary apps.** `SetWindowsHookExW(WH_KEYBOARD_LL, ...)` from a standard-user process succeeds and receives all input on its desktop.
- **But against elevated windows it goes dark.** UIPI prevents lower-privilege processes from hooking/sending to higher-privilege ones. Per Microsoft's UIAccess docs, a UIAccess process gains the ability to *"Drive any application window by using the SendInput function"* and to *"use read input for all integrity levels by using low-level hooks, raw input, GetKeyState, GetAsyncKeyState."* A non-UIAccess process gets none of that when an elevated window is foreground: **hotkey won't fire and paste won't land in an admin app** (e.g. an elevated terminal, Task Manager).
- Also dark on the **secure desktop** (UAC prompt, lock screen, Ctrl+Alt+Del) â unconditionally, for everyone.
- `SendInput` failing to UIPI is **silent**: *"neither GetLastError nor the return value will indicate the failure was caused by UIPI blocking."* Detect it by comparing `SendInput`'s `u32` return against `pinputs.len()`.
- **UIAccess is almost certainly not worth it**: it requires an Authenticode signature chaining to a Trusted Root, install into an admin-only-writable dir (`%ProgramFiles%`), and `uiAccess="true"` in the manifest. Ship without it and treat elevated apps as an unsupported target with a clear user-facing message.
- Windows Store / packaged-app processes also won't load your hook unless you are a UIAccess process.

---

## 4. Text injection at the cursor

### `enigo` 0.6.1 â **yes for arbitrary Unicode; no built-in delays.**

```rust
pub trait Keyboard {
    fn fast_text(&mut self, text: &str) -> InputResult<Option<()>>;   // hidden; platform bulk path
    fn text(&mut self, text: &str) -> InputResult<()>;                // use this
    fn key(&mut self, key: Key, direction: Direction) -> InputResult<()>;
    fn raw(&mut self, keycode: u16, direction: Direction) -> InputResult<()>;
}
```

- **Arbitrary Unicode: yes.** Doc comment: *"You can use unicode here like: â¤ï¸. This works regardless of the current keyboard layout."*
  - macOS: `fast_text` builds a `CGEvent` and calls `CGEvent::keyboard_set_unicode_string(event, buflen, buf.as_ptr())` on UTF-16, then `CGEvent::post(CGEventTapLocation::HIDEventTap, ...)`. It **chunks at 20 chars** (workaround for enigo#68 â `CGEventKeyboardSetUnicodeString` truncates past 20) and special-cases leading `\t`/`\r`/`\n` (enigo#260 â `set_string` silently fails on a leading newline, so it prefixes U+200B).
  - Windows: `fast_text` returns `Ok(None)`; `text()` is overridden to batch every char into one `SendInput` array via `KEYEVENTF_UNICODE`.
- **Key-by-key with delays: no built-in delay.** `Settings` (0.6.1) has `x11_display, wayland_display, windows_dw_extra_info, event_source_user_data, release_keys_when_dropped, open_prompt_to_get_permissions, independent_of_keyboard_state, windows_subject_to_mouse_speed_and_acceleration_level, restore_token` â **no delay/interval field** (older 0.2-era `delay` is gone). To pace input you loop `enigo.key(Key::Unicode(c), Direction::Click)?` yourself with your own `sleep` between chars. `text()`'s generic fallback does exactly this loop, undelayed.
- Useful settings: `windows_dw_extra_info` / `event_source_user_data` let you tag your synthetic events so **your own global hook ignores them** â set these.
- â ï¸ Windows surrogate bug to know about: in `queue_char`, the key-**up** event reuses `result[0]` (the high surrogate) for both halves of a surrogate pair, with a `// TODO: Double check` comment in-source. In practice Windows composes on the key-down pair so emoji land correctly, but if you hand-roll, see below.

### Exactly how to send a Unicode string reliably on Windows (incl. emoji)

```rust
use windows::Win32::UI::Input::KeyboardAndMouse::{
    SendInput, INPUT, INPUT_KEYBOARD, KEYBDINPUT, KEYEVENTF_UNICODE, KEYEVENTF_KEYUP, VIRTUAL_KEY,
};
// pub unsafe fn SendInput(pinputs: &[INPUT], cbsize: i32) -> u32
// pub struct KEYBDINPUT { pub wVk: VIRTUAL_KEY, pub wScan: u16, pub dwFlags: KEYBD_EVENT_FLAGS, pub time: u32, pub dwExtraInfo: usize }
// pub const KEYEVENTF_UNICODE: KEYBD_EVENT_FLAGS = KEYBD_EVENT_FLAGS(4u32);
```

Rules:
1. **`wVk` MUST be `VIRTUAL_KEY(0)`** and the character goes in **`wScan` as a UTF-16 code unit**, with `dwFlags = KEYEVENTF_UNICODE`.
2. **Encode the whole string to UTF-16 (`str::encode_utf16`), not chars.** A non-BMP char (emoji, U+1F600+) becomes **two** code units. Emit the **high surrogate then the low surrogate as two consecutive INPUT entries in the same `SendInput` batch** â Windows only composes them if they arrive adjacently and atomically. Never split a pair across two `SendInput` calls.
3. Emit down+up per code unit: down = `KEYEVENTF_UNICODE`, up = `KEYEVENTF_UNICODE | KEYEVENTF_KEYUP`.
4. **One `SendInput` call for the whole batch** â the array is injected atomically and cannot be interleaved with real user keystrokes. `cbsize = size_of::<INPUT>() as i32`.
5. Stamp `dwExtraInfo` with a private magic value so your `WH_KEYBOARD_LL` hook can ignore it.
6. **Check the return value** against `pinputs.len()` (silent UIPI failure, Â§3).
7. `KEYEVENTF_UNICODE` works regardless of the active keyboard layout and does **not** require `VkKeyScan`.

Caveat: a few legacy/console apps and IME-hostile controls drop `KEYEVENTF_UNICODE`. That's why the clipboard+Ctrl+V path stays as the primary and unicode-typing is the fallback (Handy makes this a user setting, `PasteMethod`).

### Clipboard save/restore â **`arboard` is NOT sufficient. Verdict: partially.**

**Crate**: `arboard` 3.6.1. Full public surface:

```rust
Clipboard::new/get_text/set_text/set_html/get_image/set_image/clear/clear_with/get/set
Get::text/image/html/file_list      Set::text/html/image/file_list
```

That is the *entire* format vocabulary: text, HTML, image (RGBAâ`CF_DIBV5` on Windows), and file lists (`CF_HDROP`). **You cannot round-trip arbitrary clipboard content** â RTF, Office/OLE formats, delayed-render (`SetClipboardData(fmt, NULL)`) offers, and any app-private format are destroyed by a naive saveâoverwriteârestore cycle. There is no `EnumClipboardFormats` equivalent and no `changeCount`/sequence-number API.

**This is not theoretical.** Handy â the shipping app with this exact requirement â **does not use arboard for the paste path.** Its Windows implementation goes raw Win32: `OpenClipboard, EnumClipboardFormats, GetClipboardData, GetClipboardOwner, GetClipboardSequenceNumber, SetClipboardData, EmptyClipboard, RegisterClipboardFormatW, CF_UNICODETEXT, CF_BITMAP` (`src-tauri/src/paste_tx/windows.rs`), snapshotting every enumerated format into `HGLOBAL` copies and restoring them, and it registers its transcript as a **delayed-render offer** (`SetClipboardData(CF_UNICODETEXT, NULL)`) from a hidden message-only window so the data is materialized on `WM_RENDERFORMAT`. It uses `GetClipboardSequenceNumber()` to verify the clipboard is still "ours" before restoring â so it never clobbers something the user copied in the meantime.

On macOS Handy uses `NSPasteboard` via `objc2-app-kit` with a declared owner object implementing `pasteboard:provideDataForType:` and `pasteboardChangedOwner:` â again lazy provision, not a blind overwrite.

**Recommendation**: use `arboard` for casual read/write (settings, copy-transcript button). For the inject-and-restore path, write the raw `windows`-crate clipboard code and the `objc2-app-kit` `NSPasteboard` owner. Budget for it; it's ~600 lines per platform and it is where the polish lives.

### macOS AXUIElement injection

Raw `objc2-application-services` 0.3.2 gives you everything (see Â§5 signatures) â `AXUIElementSetAttributeValue(elem, kAXValueAttribute, new_text)` writes directly into the focused field with no clipboard involvement and no keystroke synthesis. It's the highest-fidelity path but the least universally supported (Electron/Chromium/Java/games often refuse or silently no-op), so keep it as the *preferred* method with clipboard+Cmd+V as fallback â which is what your Swift app already does.

---

## 5. Reading the focused text field's content (`ax_context`)

### macOS â `objc2-application-services` 0.3.2 (recommended) or `accessibility-sys` 0.2.0

Call sequence:

```rust
let sys = AXUIElement::new_system_wide();                             // AXUIElementCreateSystemWide
// STRONGLY RECOMMENDED before any query:
sys.set_messaging_timeout(0.25);                                      // AXUIElementSetMessagingTimeout
let focused = sys.copy_attribute_value(kAXFocusedUIElementAttribute, out)?;
let value   = focused.copy_attribute_value(kAXValueAttribute, out)?;         // full field text
let sel     = focused.copy_attribute_value(kAXSelectedTextAttribute, out)?;  // selection
let range   = focused.copy_attribute_value(kAXSelectedTextRangeAttribute, out)?; // caret offset (AXValue CFRange)
```

Verified signatures (`AXUIElement.rs`):
```rust
pub unsafe fn AXUIElement::new_system_wide() -> CFRetained<AXUIElement>
pub unsafe fn AXUIElement::copy_attribute_value(&self, attribute: &CFString, value: NonNull<*const CFType>) -> AXError
pub unsafe fn AXUIElement::set_attribute_value(&self, attribute: &CFString, value: &CFType) -> AXError
pub unsafe fn AXUIElementCreateApplication(pid: libc::pid_t) -> ...
```
Attribute string constants (`accessibility-sys/src/attribute_constants.rs`): `kAXValueAttribute="AXValue"`, `kAXSelectedTextAttribute="AXSelectedText"`, `kAXSelectedTextRangeAttribute`, `kAXNumberOfCharactersAttribute`, `kAXFocusedUIElementAttribute`, `kAXFocusedWindowAttribute`, `kAXRoleAttribute`, `kAXTitleAttribute`, **`kAXURLAttribute="AXURL"`**.

â ï¸ **`AXUIElementSetMessagingTimeout` is the single most important call here.** Every AX query is a synchronous IPC to the target app; a hung/busy target blocks *your* thread for the default ~6 s. Set a sub-second timeout and run all AX work off the UI thread. Apple's docs note setting it on the system-wide element sets the process-global default.

### Windows â `uiautomation` 0.24.0 (recommended over raw COM)

```rust
let auto = UIAutomation::new()?;                    // CoInitializeEx(None, COINIT_MULTITHREADED) + CoCreateInstance(CUIAutomation)
let el   = auto.get_focused_element()?;             // IUIAutomation::GetFocusedElement
// preferred: TextPattern (rich edit controls, browsers, Office)
let tp: UITextPattern = el.get_pattern()?;
let all  = tp.get_document_range()?.get_text(-1)?;  // -1 = no limit
let sel  = tp.get_selection()?;                     // Vec<UITextRange>, then .get_text(-1)
// fallback: ValuePattern (simple Edit controls)
let vp: UIValuePattern = el.get_pattern()?;
let text = vp.get_value()?;
```

Verified signatures:
```rust
pub fn UIAutomation::new() -> Result<UIAutomation>
pub fn UIAutomation::new_direct() -> Result<UIAutomation>       // when you already CoInitialize'd
pub fn UIAutomation::get_focused_element(&self) -> Result<UIElement>
pub fn UIElement::get_pattern<T: UIPattern + TryFrom<IUnknown, Error = Error>>(&self) -> Result<T>
pub fn UITextPattern::get_document_range(&self) -> Result<UITextRange>
pub fn UITextPattern::get_selection(&self) -> Result<Vec<UITextRange>>
pub fn UITextRange::get_text(&self, max_length: i32) -> Result<String>
pub fn UIValuePattern::get_value(&self) -> Result<String>
pub fn UIValuePattern::set_value(&self, value: &str) -> Result<()>
pub fn UIElement::get_property_value(&self, property: UIProperty) -> Result<Variant>   // UIProperty::ValueValue = 30045
```

Gotchas:
- **COM apartment.** `UIAutomation::new()` calls `CoInitializeEx(None, COINIT_MULTITHREADED)` on the *calling* thread. Tauri's main thread is already **STA** (WebView2 requires it) â calling `new()` there will fail with `RPC_E_CHANGED_MODE`. **Run all UIA work on a dedicated MTA thread**, or use `new_direct()` after initializing the apartment yourself. Do not share `UIAutomation`/`UIElement` across apartments.
- **Every UIA call is cross-process and can block for hundreds of ms.** Never on the hotkey/hook thread. Use `create_cache_request()` + `*_build_cache` variants to batch property fetches into one round trip.
- `TextPattern` unsupported â fall back to `ValuePattern` â fall back to `WM_GETTEXT`. Chromium/Electron may need the accessibility tree activated first.
- `is_password()` â check it and refuse to read.
- Elevated targets: blocked without UIAccess (Â§3).

Raw `windows` crate COM (`Win32_UI_Accessibility`: `IUIAutomation`, `IUIAutomationElement`, `IUIAutomationTextPattern`) works too and avoids a dependency, but `uiautomation` is a thin, faithful wrapper â the `TryFrom<IUnknown>`-based `get_pattern` and `Variant` handling save real pain. Use the crate.

---

## 6. Frontmost application info

### macOS â `objc2-app-kit` 0.3.2 + AX

```rust
pub fn NSWorkspace::frontmostApplication(&self) -> Option<Retained<NSRunningApplication>>;
pub fn NSRunningApplication::localizedName(&self) -> Option<Retained<NSString>>;
pub fn NSRunningApplication::bundleIdentifier(&self) -> Option<Retained<NSString>>;
pub fn NSRunningApplication::executableURL(&self) -> Option<Retained<NSURL>>;
pub fn NSRunningApplication::processIdentifier(&self) -> libc::pid_t;
```

Window title: take the `pid` â `AXUIElementCreateApplication(pid)` â `kAXFocusedWindowAttribute` â `kAXTitleAttribute`.
Browser URL: from the focused window, walk to the `AXWebArea` element and read **`kAXURLAttribute`** (`"AXURL"`, present in `accessibility-sys`). Safari/Chrome/Edge/Arc expose it. AppleScript (`osascript`) is the crude fallback but is slow and triggers a separate Automation TCC prompt per browser.

### Windows â raw `windows` 0.62 + `uiautomation`

```rust
// verified signatures, windows 0.62.2
pub unsafe fn GetForegroundWindow() -> HWND
pub unsafe fn GetWindowThreadProcessId(hwnd: HWND, lpdwprocessid: Option<*mut u32>) -> u32
pub unsafe fn GetWindowTextW(hwnd: HWND, lpstring: &mut [u16]) -> i32
pub unsafe fn QueryFullProcessImageNameW(hprocess: HANDLE, dwflags: PROCESS_NAME_FORMAT,
                                         lpexename: PWSTR, lpdwsize: *mut u32) -> windows_core::Result<()>
```

Sequence:
1. `let hwnd = GetForegroundWindow();` (null if the foreground window belongs to another desktop or a secure desktop is up â handle it).
2. `let mut pid = 0u32; GetWindowThreadProcessId(hwnd, Some(&mut pid));`
3. `OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid)` â **`_LIMITED_` is the important one**: it succeeds against higher-integrity processes where `PROCESS_QUERY_INFORMATION` fails. Requires `Win32_System_Threading`.
4. `QueryFullProcessImageNameW(h, PROCESS_NAME_WIN32, PWSTR(buf.as_mut_ptr()), &mut len)` â full exe path. `.file_name()` gives the process name; there is no bundle-id analogue â use the exe path (and optionally `GetFileVersionInfoW` for a display name, or the AUMID via `IPropertyStore`/`PKEY_AppUserModel_ID` for packaged apps).
5. Window title: `GetWindowTextW(hwnd, &mut buf)`. â ï¸ `GetWindowTextW` on a window owned by *another* process does not send `WM_GETTEXT` â it returns the cached title, which is fine and, importantly, cannot deadlock. Prefer it over `SendMessageW(WM_GETTEXT)`.
6. **Browser URL**: UIA on the address bar.
   ```rust
   let auto = UIAutomation::new()?;                          // on an MTA thread
   let root = auto.element_from_handle(Handle::from(hwnd))?;
   let bar  = auto.create_matcher().from(root)
                  .control_type(ControlType::Edit)
                  .name("Address and search bar")            // Chrome/Edge en-US; Firefox: "Search with ... or enter address"
                  .depth(12).timeout(200)
                  .find_first()?;
   let url = bar.get_pattern::<UIValuePattern>()?.get_value()?;
   // or, one call: bar.get_property_value(UIProperty::ValueValue)?  // = 30045
   ```
   â ï¸ **The `name` is localized and version-dependent** â do not hard-code it as the only strategy. Robust version: match on `ControlType::Edit` within the browser's toolbar subtree and take the first element exposing a `ValuePattern` whose value parses as a URL; keep the localized names as a fast path. Also: the URL shown is the *displayed* one (Chrome hides `https://` and may show the in-progress typed text if the omnibox has focus).
   Alternative: `element_from_handle(hwnd)` â `UIProperty::ValueValue` on the browser's document element, which Chromium exposes for the `Document` control type â often more stable than the omnibox and unaffected by user typing.

**Crates needed**: `windows` 0.62 with features `Win32_Foundation, Win32_UI_WindowsAndMessaging, Win32_System_Threading, Win32_UI_Input_KeyboardAndMouse, Win32_System_DataExchange, Win32_System_Memory, Win32_System_Ole` (Handy's list is a good reference), plus `uiautomation` 0.24.

---

## Recommended stack (summary)

| Concern | macOS | Windows |
|---|---|---|
| Modifier-only PTT hotkey | **`handy-keys` 0.3.3** (CGEventTap on a worker thread w/ CFRunLoop) | **`handy-keys` 0.3.3** (`WH_KEYBOARD_LL` on a dedicated pumped thread) |
| Permissions | `objc2-application-services` 0.3.2 (`AXIsProcessTrustedWithOptions`) + `objc2-core-graphics` 0.3 (`CGPreflightListenEventAccess`, `CGPreflightPostEventAccess`) | none needed (accept: no elevated-app support) |
| Keystroke/paste synthesis | `enigo` 0.6.1 (`text`, `key`) â set `event_source_user_data` | `enigo` 0.6.1 or hand-rolled `SendInput`+`KEYEVENTF_UNICODE` â set `windows_dw_extra_info` |
| Clipboard save/restore | raw `objc2-app-kit` `NSPasteboard` owner (**not** arboard) | raw `windows` clipboard + `EnumClipboardFormats` + delayed render (**not** arboard) |
| Read focused field / inject at caret | `objc2-application-services` AXUIElement (+ `set_messaging_timeout`!) | `uiautomation` 0.24 `TextPattern`/`ValuePattern` on an **MTA** thread |
| Frontmost app + URL | `objc2-app-kit` NSWorkspace + AX `kAXURLAttribute` | `windows` 0.62 `GetForegroundWindow`/`QueryFullProcessImageNameW` + `uiautomation` |

**Do not use**: `global-hotkey` / `tauri-plugin-global-shortcut` (no modifier-only), `rdev` (left/right modifier desync + never re-enables a timed-out tap), `device_query` (polling, can't suppress), `arboard` for the injection path (can't preserve arbitrary formats).

**Strongest single recommendation**: read `cjpais/Handy` (`src-tauri/src/shortcut/`, `src-tauri/src/paste_tx/`, `src-tauri/src/clipboard.rs`). It is MIT-licensed, cross-platform Rust + Tauri v2, and it is the *same product*. Its `src-tauri/Cargo.toml` is a validated dependency set: `handy-keys = "0.3.3"`, `enigo = "0.6.1"`, `windows = "0.61.3"`, `objc2 = "0.6"` / `objc2-app-kit = "0.3"`, and it kept `rdev` (rustdesk fork) only for non-hotkey uses. Every hard-won workaround in this report has a corresponding issue number in their source comments."


## [api]

[
  {
    "signature": "pub struct HotKey { pub mods: Modifiers, pub key: Code, pub id: u32 }\npub fn HotKey::new(mods: Option<Modifiers>, key: Code) -> Self",
    "description": "global-hotkey 0.8.0. `key` is a mandatory non-optional `Code` — modifier-only hotkeys are unrepresentable. Both platform key tables (`key_to_scancode`, `key_to_vk`) also lack any modifier entry, so even `Code::ControlLeft` returns FailedToRegister."
  },
  {
    "signature": "pub enum HotKeyState { Pressed, Released }\npub struct GlobalHotKeyEvent { pub id: u32, pub state: HotKeyState }",
    "description": "global-hotkey does report press+release, so hold-to-talk works for real modifier+key combos. On Windows `Released` is synthesized by a spawned thread polling GetAsyncKeyState every 50ms."
  },
  {
    "signature": "pub struct Hotkey { pub modifiers: Modifiers, pub key: Option<Key> }\npub fn Hotkey::new(modifiers: Modifiers, key: impl Into<Option<Key>>) -> Result<Self>",
    "description": "handy-keys 0.3.3. `key: Option<Key>` — modifier-only is a first-class, documented case. Use `Hotkey::new(Modifiers::CTRL_LEFT, None)` for Left-Control push-to-talk."
  },
  {
    "signature": "pub struct Modifiers: u32 { CMD_LEFT, SHIFT_LEFT, CTRL_LEFT, OPT_LEFT, FN, CMD_RIGHT, SHIFT_RIGHT, CTRL_RIGHT, OPT_RIGHT, CMD, SHIFT, CTRL, OPT }",
    "description": "handy-keys side-specific modifier flags plus 'either side' compound aliases and Fn. This is what rdev lacks and why rdev desyncs on left/right modifier interleaving."
  },
  {
    "signature": "pub fn HotkeyManager::new() -> Result<Self>\npub fn HotkeyManager::new_with_blocking() -> Result<Self>\npub fn HotkeyManager::register(&self, hotkey: Hotkey) -> Result<HotkeyId>\npub fn HotkeyManager::unregister(&self, id: HotkeyId) -> Result<()>\npub fn HotkeyManager::recv(&self) -> Result<HotkeyEvent>\npub fn HotkeyManager::try_recv(&self) -> Option<HotkeyEvent>\npub struct HotkeyEvent { pub id: HotkeyId, pub state: HotkeyState }",
    "description": "handy-keys 0.3.3 manager API. `new_with_blocking` enables event suppression. Not Sync-friendly — own it on one dedicated thread and drive it via a channel (the pattern Handy uses in src/shortcut/handy_keys.rs)."
  },
  {
    "signature": "pub unsafe fn CGEvent::tap_create(tap: CGEventTapLocation, place: CGEventTapPlacement, options: CGEventTapOptions, events_of_interest: CGEventMask, callback: CGEventTapCallBack, user_info: *mut c_void) -> Option<CFRetained<CFMachPort>>\npub fn CGEvent::tap_enable(tap: &CFMachPort, enable: bool)\npub fn CGEvent::tap_is_enabled(tap: &CFMachPort) -> bool",
    "description": "objc2-core-graphics 0.3.2. The raw-FFI fallback for macOS. Callback fires on whichever thread runs the CFRunLoop the tap source was added to — no main-thread requirement."
  },
  {
    "signature": "CGEventType::TapDisabledByTimeout = 0xFFFF_FFFE\nCGEventType::TapDisabledByUserInput = 0xFFFF_FFFF",
    "description": "objc2-core-graphics 0.3.2. You MUST match these first in your tap callback and call `CGEvent::tap_enable(tap, true)`, then re-sync modifier state. rdev ignores both and dies silently."
  },
  {
    "signature": "pub extern \"C-unwind\" fn CGPreflightListenEventAccess() -> bool\npub extern \"C-unwind\" fn CGRequestListenEventAccess() -> bool\npub extern \"C-unwind\" fn CGPreflightPostEventAccess() -> bool",
    "description": "objc2-core-graphics 0.3.2. macOS Input Monitoring (kTCCServiceListenEvent) preflight/request, and post-event permission check for injection. Distinct from Accessibility."
  },
  {
    "signature": "pub unsafe extern \"C-unwind\" fn AXIsProcessTrustedWithOptions(options: Option<&CFDictionary>) -> bool\npub unsafe extern \"C-unwind\" fn AXIsProcessTrusted() -> bool\npub static kAXTrustedCheckOptionPrompt: &'static CFString",
    "description": "objc2-application-services 0.3.2 — the recommended crate exposing AXIsProcessTrustedWithOptions. Alternatives: accessibility-sys 0.2.0 (`AXIsProcessTrustedWithOptions(options: CFDictionaryRef) -> bool`) and macos-accessibility-client 0.0.1 (`application_is_trusted_with_prompt() -> bool`). enigo does NOT export it."
  },
  {
    "signature": "pub unsafe fn AXUIElement::new_system_wide() -> CFRetained<AXUIElement>\npub unsafe fn AXUIElement::copy_attribute_value(&self, attribute: &CFString, value: NonNull<*const CFType>) -> AXError\npub unsafe fn AXUIElement::set_attribute_value(&self, attribute: &CFString, value: &CFType) -> AXError\npub unsafe fn AXUIElementCreateApplication(pid: libc::pid_t) -> ...",
    "description": "objc2-application-services 0.3.2 AXUIElement. Sequence for ax_context: new_system_wide -> kAXFocusedUIElementAttribute -> kAXValueAttribute / kAXSelectedTextAttribute / kAXSelectedTextRangeAttribute. Call AXUIElementSetMessagingTimeout first — every query is blocking IPC with a ~6s default."
  },
  {
    "signature": "kAXValueAttribute=\"AXValue\", kAXSelectedTextAttribute=\"AXSelectedText\", kAXSelectedTextRangeAttribute, kAXNumberOfCharactersAttribute, kAXFocusedUIElementAttribute, kAXFocusedWindowAttribute, kAXRoleAttribute, kAXTitleAttribute, kAXURLAttribute=\"AXURL\"",
    "description": "accessibility-sys 0.2.0 attribute constants. kAXURLAttribute on the AXWebArea element is the macOS browser-tab-URL path."
  },
  {
    "signature": "pub trait Keyboard { fn text(&mut self, text: &str) -> InputResult<()>; fn key(&mut self, key: Key, direction: Direction) -> InputResult<()>; fn raw(&mut self, keycode: u16, direction: Direction) -> InputResult<()>; }",
    "description": "enigo 0.6.1. `text` handles arbitrary Unicode incl. emoji on both platforms (macOS: CGEventKeyboardSetUnicodeString in 20-char chunks; Windows: batched SendInput with KEYEVENTF_UNICODE). No built-in inter-key delay — Settings has no delay field; pace it yourself by looping `key(Key::Unicode(c), Direction::Click)` with your own sleep."
  },
  {
    "signature": "pub struct Settings { windows_dw_extra_info: Option<usize>, event_source_user_data: Option<i64>, release_keys_when_dropped: bool, open_prompt_to_get_permissions: bool, independent_of_keyboard_state: bool, ... }",
    "description": "enigo 0.6.1 Settings. Set windows_dw_extra_info / event_source_user_data to tag your synthetic events so your own global hook can ignore them. Note: NO delay/interval field exists in 0.6."
  },
  {
    "signature": "pub unsafe fn SendInput(pinputs: &[INPUT], cbsize: i32) -> u32\npub struct KEYBDINPUT { pub wVk: VIRTUAL_KEY, pub wScan: u16, pub dwFlags: KEYBD_EVENT_FLAGS, pub time: u32, pub dwExtraInfo: usize }\npub const KEYEVENTF_UNICODE: KEYBD_EVENT_FLAGS = KEYBD_EVENT_FLAGS(4u32);",
    "description": "windows 0.62.2. Unicode injection recipe: wVk = VIRTUAL_KEY(0), UTF-16 code unit in wScan, dwFlags = KEYEVENTF_UNICODE (down) / |KEYEVENTF_KEYUP (up). Encode with str::encode_utf16 so non-BMP chars become two adjacent code units in the SAME batch. Compare the u32 return against pinputs.len() — UIPI failure is silent."
  },
  {
    "signature": "pub unsafe fn SetWindowsHookExW(idhook: WINDOWS_HOOK_ID, lpfn: HOOKPROC, hmod: Option<HINSTANCE>, dwthreadid: u32) -> windows_core::Result<HHOOK>\npub const WH_KEYBOARD_LL: WINDOWS_HOOK_ID = WINDOWS_HOOK_ID(13i32);\npub struct KBDLLHOOKSTRUCT { pub vkCode: u32, pub scanCode: u32, pub flags: KBDLLHOOKSTRUCT_FLAGS, pub time: u32, pub dwExtraInfo: usize }\npub const LLKHF_INJECTED: KBDLLHOOKSTRUCT_FLAGS = KBDLLHOOKSTRUCT_FLAGS(16u32);",
    "description": "windows 0.62.2. Pass hmod=None, dwthreadid=0 — WH_KEYBOARD_LL is global-scope but not DLL-injected. Install from a dedicated thread that runs a message pump. Use LLKHF_INJECTED or a private dwExtraInfo to filter out your own injected keystrokes."
  },
  {
    "signature": "pub fn UIAutomation::new() -> Result<UIAutomation>\npub fn UIAutomation::new_direct() -> Result<UIAutomation>\npub fn UIAutomation::get_focused_element(&self) -> Result<UIElement>\npub fn UIAutomation::element_from_handle(&self, hwnd: Handle) -> Result<UIElement>\npub fn UIElement::get_pattern<T: UIPattern + TryFrom<IUnknown, Error = Error>>(&self) -> Result<T>",
    "description": "uiautomation 0.24.0. `new()` calls CoInitializeEx(COINIT_MULTITHREADED) on the calling thread — it will FAIL on Tauri's STA main thread. Run on a dedicated MTA thread, or use new_direct() after initializing the apartment yourself."
  },
  {
    "signature": "pub fn UITextPattern::get_document_range(&self) -> Result<UITextRange>\npub fn UITextPattern::get_selection(&self) -> Result<Vec<UITextRange>>\npub fn UITextRange::get_text(&self, max_length: i32) -> Result<String>\npub fn UIValuePattern::get_value(&self) -> Result<String>\npub fn UIValuePattern::set_value(&self, value: &str) -> Result<()>\npub fn UIElement::get_property_value(&self, property: UIProperty) -> Result<Variant>  // UIProperty::ValueValue = 30045",
    "description": "uiautomation 0.24.0 — the Windows equivalent of AX kAXValue/kAXSelectedText. Try TextPattern first (rich controls, browsers), fall back to ValuePattern (simple Edits), then WM_GETTEXT. get_text(-1) = unlimited. Check UIElement::is_password() before reading."
  },
  {
    "signature": "pub unsafe fn GetForegroundWindow() -> HWND\npub unsafe fn GetWindowThreadProcessId(hwnd: HWND, lpdwprocessid: Option<*mut u32>) -> u32\npub unsafe fn QueryFullProcessImageNameW(hprocess: HANDLE, dwflags: PROCESS_NAME_FORMAT, lpexename: PWSTR, lpdwsize: *mut u32) -> windows_core::Result<()>\npub unsafe fn GetWindowTextW(hwnd: HWND, lpstring: &mut [u16]) -> i32",
    "description": "windows 0.62.2 frontmost-app sequence. Open the process with PROCESS_QUERY_LIMITED_INFORMATION (succeeds across integrity levels where PROCESS_QUERY_INFORMATION fails). GetWindowTextW on a foreign window returns the cached title without sending WM_GETTEXT, so it cannot deadlock."
  },
  {
    "signature": "pub fn NSWorkspace::frontmostApplication(&self) -> Option<Retained<NSRunningApplication>>\npub fn NSRunningApplication::localizedName(&self) -> Option<Retained<NSString>>\npub fn NSRunningApplication::bundleIdentifier(&self) -> Option<Retained<NSString>>\npub fn NSRunningApplication::executableURL(&self) -> Option<Retained<NSURL>>\npub fn NSRunningApplication::processIdentifier(&self) -> libc::pid_t",
    "description": "objc2-app-kit 0.3.2 — macOS frontmost app name / bundle id / exe path / pid. Feed the pid to AXUIElementCreateApplication for window title (kAXFocusedWindow -> kAXTitle) and browser URL (AXWebArea -> kAXURLAttribute)."
  },
  {
    "signature": "Clipboard::{new, get_text, set_text, set_html, get_image, set_image, clear, clear_with, get, set}; Get::{text, image, html, file_list}; Set::{text, html, image, file_list}",
    "description": "arboard 3.6.1 — the COMPLETE public surface. Only text/HTML/image/file-list. Cannot enumerate or round-trip RTF, OLE, delayed-render, or app-private formats, and has no clipboard sequence-number API. Insufficient for save/restore around paste injection; use raw Win32 (EnumClipboardFormats + delayed render) and NSPasteboard owner objects instead."
  }
]


## [caveats]

[
  "`global-hotkey` and the Tauri `global-shortcut` plugin CANNOT do modifier-only hold-to-talk. Confirmed at the type level (`HotKey.key: Code`, non-optional) and at the platform level (neither key_to_scancode nor key_to_vk maps any modifier). Your suspicion was right.",
  "rdev's macOS FlagsChanged handling decides press-vs-release from the SHARED CGEventFlags mask, so holding LeftCtrl, tapping RightCtrl, then releasing LeftCtrl emits NO release event. It also never handles kCGEventTapDisabledByTimeout — grep the entire crate for 'TapDisabled': zero hits. Do not use it for PTT.",
  "Never poll CGEventTapIsEnabled on a fast timer as your primary recovery: handy-keys documents in-source that recurring WindowServer RPCs leak kernel IPC vouchers and eventually panic the whole machine (cjpais/Handy#1827). Re-enable from inside the tap callback.",
  "A macOS event tap can become permanently inert with NO TapDisabled callback at all after a code-signing / TCC change. The only recovery is to destroy and recreate the tap. Budget a low-frequency health check for wake-from-sleep and TCC-change events.",
  "Windows silently removes a WH_KEYBOARD_LL hook that exceeds LowLevelHooksTimeout, and MS states outright: 'There is no way for the application to know whether the hook is removed.' Timeout is capped at 1000ms on 1709+. Do zero work in the callback.",
  "GetAsyncKeyState is unreliable inside a LowLevelKeyboardProc — MS: 'the callback function is called before the asynchronous state of the key is updated.' Track modifier state from the message stream yourself.",
  "UIPI: without a signed UIAccess manifest installed to %ProgramFiles%, your hook goes dark and SendInput silently no-ops whenever an elevated window is foreground (elevated terminal, Task Manager) and always on the secure desktop. SendInput's failure is undetectable via GetLastError — compare its u32 return to your input count.",
  "uiautomation::UIAutomation::new() calls CoInitializeEx(COINIT_MULTITHREADED) on the calling thread and will fail with RPC_E_CHANGED_MODE on Tauri's STA main thread (WebView2 requires STA). All UIA work must live on a dedicated MTA thread.",
  "Every AXUIElement query is synchronous cross-process IPC with a ~6 second default timeout. Call AXUIElementSetMessagingTimeout (e.g. 0.25s) on the system-wide element before any query, and never do AX work on the hotkey or UI thread.",
  "arboard cannot preserve arbitrary clipboard content. Handy — the shipping equivalent app — deliberately bypasses it, using raw EnumClipboardFormats/SetClipboardData with delayed rendering on Windows and an NSPasteboard owner object on macOS, plus GetClipboardSequenceNumber to avoid clobbering something the user copied mid-flight. Budget ~600 lines per platform.",
  "enigo's Windows queue_char sends the key-UP for BOTH halves of a surrogate pair using result[0] (the high surrogate), with a 'TODO: Double check' left in the source. Emoji still compose correctly because Windows composes on the down pair, but if you hand-roll SendInput, emit each code unit's own up event.",
  "enigo's macOS fast_text chunks at 20 characters (CGEventKeyboardSetUnicodeString truncates past 20, enigo#68) and prefixes U+200B before leading newlines (set_string silently fails on a leading newline, enigo#260). Replicating this path by hand means replicating both workarounds.",
  "The Windows browser address-bar UIA name ('Address and search bar') is localized and version-dependent. Do not hard-code it as your only strategy — match ControlType::Edit within the toolbar subtree and take the first ValuePattern value that parses as a URL, keeping localized names as a fast path.",
  "Tag your own injected events (enigo's windows_dw_extra_info / event_source_user_data, or LLKHF_INJECTED) or your paste will retrigger your own push-to-talk hotkey."
]


## [sources]

[
  {
    "repo": "tauri-apps/global-hotkey",
    "path": "src/hotkey.rs",
    "line_start": 49,
    "line_end": 58,
    "excerpt": "/// A keyboard shortcut that consists of an optional combination\n/// of modifier keys (provided by [`Modifiers`](crate::hotkey::Modifiers)) and\n/// one key ([`Code`](crate::hotkey::Code)).\npub struct HotKey {\n    pub mods: Modifiers,\n    pub key: Code,\n    pub id: u32,\n}"
  },
  {
    "repo": "tauri-apps/global-hotkey",
    "path": "src/platform_impl/macos/mod.rs",
    "line_start": 98,
    "line_end": 131,
    "excerpt": "if let Some(scan_code) = key_to_scancode(hotkey.key) {\n    ... RegisterEventHotKey(scan_code, mods, hotkey_id, GetApplicationEventTarget(), 0, &mut hotkey_ref);\n// key_to_scancode(code: Code) has NO ControlLeft/ShiftLeft/AltLeft/MetaLeft arm (verified by grep of lines 411-520: zero matches for control|shift|alt|meta|super)"
  },
  {
    "repo": "tauri-apps/global-hotkey",
    "path": "src/platform_impl/windows/mod.rs",
    "line_start": 95,
    "line_end": 96,
    "excerpt": "let result = unsafe { RegisterHotKey(self.hwnd, hotkey.id() as _, mods, vk_code as _) };\n// key_to_vk(&Code) at line 204 has NO modifier arm (grep of lines 204-325: zero matches)"
  },
  {
    "repo": "tauri-apps/global-hotkey",
    "path": "src/platform_impl/windows/mod.rs",
    "line_start": 157,
    "line_end": 168,
    "excerpt": "std::thread::spawn(move || loop {\n    let state = GetAsyncKeyState(HIWORD(lparam as u32) as i32);\n    if state == 0 { GlobalHotKeyEvent::send(GlobalHotKeyEvent { id: wparam as _, state: crate::HotKeyState::Released }); break; }\n    // Sleep to avoid burning a core for the whole hold duration (e.g. push-to-talk). 50ms keeps release latency imperceptible.\n    std::thread::sleep(std::time::Duration::from_millis(50));\n});"
  },
  {
    "repo": "tauri-apps/plugins-workspace (tauri-plugin-global-shortcut 2.3.1)",
    "path": "src/lib.rs",
    "line_start": 21,
    "line_end": 61,
    "excerpt": "use global_hotkey::GlobalHotKeyEvent;\npub use global_hotkey::{ ..., GlobalHotKeyEvent as ShortcutEvent, HotKeyState as ShortcutState };\nstruct GlobalHotKeyManager(global_hotkey::GlobalHotKeyManager);"
  },
  {
    "repo": "Narsil/rdev (0.6.0)",
    "path": "src/macos/common.rs",
    "line_start": 78,
    "line_end": 132,
    "excerpt": "CGEventType::FlagsChanged => {\n    let flags = CGEvent::flags(Some(cg_event.as_ref()));\n    let mut global_flags = LAST_FLAGS.lock().unwrap();\n    if flags.contains(CGEventFlags::MaskShift) && !global_flags.contains(CGEventFlags::MaskShift) { ... Some(EventType::KeyPress(key)) }\n    else if !flags.contains(CGEventFlags::MaskControl) && global_flags.contains(CGEventFlags::MaskControl) { ... Some(EventType::KeyRelease(key)) }\n    // decided purely from the SHARED mask -> left/right desync"
  },
  {
    "repo": "Narsil/rdev (0.6.0)",
    "path": "src/macos/listen.rs",
    "line_start": 41,
    "line_end": 67,
    "excerpt": "pub fn listen<T>(callback: T) -> Result<(), ListenError> {\n    GLOBAL_CALLBACK = Some(Box::new(callback));\n    let tap = CGEvent::tap_create(CGEventTapLocation::HIDEventTap, CGEventTapPlacement::HeadInsertEventTap, CGEventTapOptions::ListenOnly, kCGEventMaskForAllEvents.into(), callback, null_mut())...;\n    CGEvent::tap_enable(&tap, true);\n    CFRunLoop::run();\n} // grep for TapDisabled across src/: zero matches -> never re-enabled"
  },
  {
    "repo": "ostrosco/device_query (4.0.1)",
    "path": "src/device_events/event_loop.rs",
    "line_start": 17,
    "line_end": 34,
    "excerpt": "fn keyboard_thread(callbacks: Weak<KeyboardCallbacks>, sleep_dur: Duration) -> JoinHandle<()> { ... sleep(sleep_dur); }  // polling; macOS backend calls readkey -> CGEventSourceKeyState, Windows -> GetAsyncKeyState"
  },
  {
    "repo": "handy-computer/handy-keys (0.3.3)",
    "path": "src/types/hotkey.rs",
    "line_start": 25,
    "line_end": 55,
    "excerpt": "pub struct Hotkey { pub modifiers: Modifiers, pub key: Option<Key> }\n/// // Modifier-only\n/// let hotkey = Hotkey::new(Modifiers::CMD | Modifiers::SHIFT, None).unwrap();\npub fn new(modifiers: Modifiers, key: impl Into<Option<Key>>) -> Result<Self> {\n    if modifiers.is_empty() && key.is_none() { return Err(Error::EmptyHotkey); }\n    Ok(Self { modifiers, key })\n}"
  },
  {
    "repo": "handy-computer/handy-keys (0.3.3)",
    "path": "src/types/modifiers.rs",
    "line_start": 16,
    "line_end": 33,
    "excerpt": "pub struct Modifiers: u32 {\n    const CMD_LEFT = 1 << 0; const SHIFT_LEFT = 1 << 1; const CTRL_LEFT = 1 << 2; const OPT_LEFT = 1 << 3; const FN = 1 << 4;\n    const CMD_RIGHT = 1 << 5; const SHIFT_RIGHT = 1 << 6; const CTRL_RIGHT = 1 << 7; const OPT_RIGHT = 1 << 8;\n    const CTRL = Self::CTRL_LEFT.bits() | Self::CTRL_RIGHT.bits();\n}"
  },
  {
    "repo": "handy-computer/handy-keys (0.3.3)",
    "path": "src/platform/macos/listener.rs",
    "line_start": 176,
    "line_end": 195,
    "excerpt": "// Re-enabling from the callback (rather than polling CGEventTapIsEnabled from the run loop) is the Apple-documented pattern and avoids recurring WindowServer RPCs -- those leak kernel IPC vouchers and eventually panic the whole machine (cjpais/Handy#1827).\nif matches!(event_type, CGEventType::TapDisabledByTimeout | CGEventType::TapDisabledByUserInput) {\n    if let Some(tap) = ctx.tap.get() { CGEvent::tap_enable(tap.as_ref(), true); }\n    reconcile_modifiers(&mut state.current_modifiers, CGEventSource::flags_state(CGEventSourceStateID::CombinedSessionState));\n    return event.as_ptr();\n}"
  },
  {
    "repo": "handy-computer/handy-keys (0.3.3)",
    "path": "src/platform/windows/listener.rs",
    "line_start": 132,
    "line_end": 371,
    "excerpt": "fn drain_thread_messages(msg: &mut MSG) -> DrainOutcome { while PeekMessageW(msg, None, 0, 0, PM_REMOVE).as_bool() { ... } }\nlet new_kb = match SetWindowsHookExW(WH_KEYBOARD_LL, Some(keyboard_hook_proc), None, 0) { ... };\nlet handle = thread::spawn(move || { ... });   // dedicated hook thread + MsgWaitForMultipleObjects/QS_ALLINPUT"
  },
  {
    "repo": "cjpais/Handy",
    "path": "src-tauri/Cargo.toml",
    "line_start": 50,
    "line_end": 151,
    "excerpt": "rdev = { git = \"https://github.com/rustdesk-org/rdev\" }\nhandy-keys = \"0.3.3\"\nenigo = \"0.6.1\"\nwindows = { version = \"0.61.3\", features = [ \"Win32_System_DataExchange\", \"Win32_System_Ole\", \"Win32_System_Memory\", ... ] }\nobjc2 = \"0.6\"  objc2-foundation = \"0.3\"  objc2-app-kit = \"0.3\""
  },
  {
    "repo": "cjpais/Handy",
    "path": "src-tauri/src/paste_tx/windows.rs",
    "line_start": 4,
    "line_end": 33,
    "excerpt": "//! (`SetClipboardData(CF_UNICODETEXT, NULL)`) owned by a hidden message-only ...\nuse ...{ CloseClipboard, EmptyClipboard, EnumClipboardFormats, GetClipboardData, GetClipboardOwner, GetClipboardSequenceNumber, OpenClipboard, RegisterClipboardFormatW, SetClipboardData, ... }   // NOT arboard"
  },
  {
    "repo": "enigo-rs/enigo (0.6.1, HEAD 19f6b97 2026-07-21)",
    "path": "src/lib.rs",
    "line_start": 210,
    "line_end": 288,
    "excerpt": "pub trait Keyboard {\n    fn fast_text(&mut self, text: &str) -> InputResult<Option<()>>;\n    /// Use a fast method to enter the text, if it is available. You can use unicode here like: ❤️. This works regardless of the current keyboard layout.\n    fn text(&mut self, text: &str) -> InputResult<()> { ... for c in text.chars() { self.key(Key::Unicode(c), Direction::Click)?; } ... }\n    fn key(&mut self, key: Key, direction: Direction) -> InputResult<()>;\n    fn raw(&mut self, keycode: u16, direction: Direction) -> InputResult<()>;\n}"
  },
  {
    "repo": "enigo-rs/enigo (0.6.1)",
    "path": "src/macos/macos_impl.rs",
    "line_start": 289,
    "line_end": 338,
    "excerpt": "// WORKAROUND: issue #68 -- CGEventKeyboardSetUnicodeString truncates strings down to 20 characters\nfor mut chunk in chunks(text, 20) { ... let buf: Vec<u16> = chunk.encode_utf16().collect();\n  unsafe { CGEvent::keyboard_set_unicode_string(Some(&event), buflen, buf.as_ptr()) };\n  CGEvent::post(CGEventTapLocation::HIDEventTap, Some(&event)); }"
  },
  {
    "repo": "enigo-rs/enigo (0.6.1)",
    "path": "src/win/win_impl.rs",
    "line_start": 536,
    "line_end": 556,
    "excerpt": "// Windows uses uft-16 encoding... some characters can be 32 bit long and those are encoded in high and low surrogates. Each are 16 bit wide and need to be sent after another to the SendInput function\nlet result = character.encode_utf16(buffer);\nfor &utf16_surrogate in &*result {\n    input_queue.push(keybd_event(KEYEVENTF_UNICODE, VIRTUAL_KEY(0), utf16_surrogate, self.dw_extra_info));\n    input_queue.push(keybd_event(KEYEVENTF_UNICODE | KEYEVENTF_KEYUP, VIRTUAL_KEY(0), result[0], self.dw_extra_info));  // TODO: Double check\n}"
  },
  {
    "repo": "enigo-rs/enigo (0.6.1)",
    "path": "src/lib.rs",
    "line_start": 478,
    "line_end": 515,
    "excerpt": "pub struct Settings { x11_display, wayland_display, windows_dw_extra_info: Option<usize>, event_source_user_data: Option<i64>, release_keys_when_dropped: bool, open_prompt_to_get_permissions: bool, independent_of_keyboard_state: bool, windows_subject_to_mouse_speed_and_acceleration_level: bool, restore_token }  // NO delay/interval field"
  },
  {
    "repo": "madsmtm/objc2 (objc2-application-services 0.3.2)",
    "path": "src/generated/HIServices/AXUIElement.rs",
    "line_start": 41,
    "line_end": 66,
    "excerpt": "pub unsafe extern \"C-unwind\" fn AXIsProcessTrustedWithOptions(options: Option<&CFDictionary>) -> bool { ... }\nextern \"C\" { pub static kAXTrustedCheckOptionPrompt: &'static CFString; }\npub unsafe extern \"C-unwind\" fn AXIsProcessTrusted() -> bool { ... }"
  },
  {
    "repo": "madsmtm/objc2 (objc2-application-services 0.3.2)",
    "path": "src/generated/HIServices/AXUIElement.rs",
    "line_start": 309,
    "line_end": 345,
    "excerpt": "pub unsafe fn copy_attribute_value(&self, attribute: &CFString, value: NonNull<*const CFType>) -> AXError\npub unsafe fn set_attribute_value(&self, attribute: &CFString, value: &CFType) -> AXError\npub unsafe fn new_system_wide() -> CFRetained<AXUIElement>   // AXUIElementCreateSystemWide"
  },
  {
    "repo": "eeeeeta/accessibility-rs (accessibility-sys 0.2.0)",
    "path": "src/attribute_constants.rs",
    "line_start": 2,
    "line_end": 97,
    "excerpt": "pub const kAXRoleAttribute: &str = \"AXRole\";\npub const kAXValueAttribute: &str = \"AXValue\";\npub const kAXSelectedTextAttribute: &str = \"AXSelectedText\";\npub const kAXSelectedTextRangeAttribute: &str = \"AXSelectedTextRange\";\npub const kAXFocusedUIElementAttribute: &str = \"AXFocusedUIElement\";\npub const kAXURLAttribute: &str = \"AXURL\";"
  },
  {
    "repo": "madsmtm/objc2 (objc2-core-graphics 0.3.2)",
    "path": "src/generated/CGEvent.rs",
    "line_start": 365,
    "line_end": 554,
    "excerpt": "pub unsafe fn tap_create(tap: CGEventTapLocation, place: CGEventTapPlacement, options: CGEventTapOptions, events_of_interest: CGEventMask, callback: CGEventTapCallBack, user_info: *mut c_void) -> Option<CFRetained<CFMachPort>>\npub fn tap_enable(tap: &CFMachPort, enable: bool)\npub fn tap_is_enabled(tap: &CFMachPort) -> bool\npub extern \"C-unwind\" fn CGPreflightListenEventAccess() -> bool\npub extern \"C-unwind\" fn CGRequestListenEventAccess() -> bool\npub extern \"C-unwind\" fn CGPreflightPostEventAccess() -> bool"
  },
  {
    "repo": "madsmtm/objc2 (objc2-core-graphics 0.3.2)",
    "path": "src/generated/CGEventTypes.rs",
    "line_start": 245,
    "line_end": 248,
    "excerpt": "#[doc(alias = \"kCGEventTapDisabledByTimeout\")] pub const TapDisabledByTimeout: Self = Self(4294967294);\n#[doc(alias = \"kCGEventTapDisabledByUserInput\")] pub const TapDisabledByUserInput: Self = Self(4294967295);"
  },
  {
    "repo": "microsoft/win32 docs",
    "path": "winmsg/lowlevelkeyboardproc.md",
    "line_start": 1,
    "line_end": 1,
    "excerpt": "\"This hook is called in the context of the thread that installed it... Therefore, the thread that installed the hook must have a message loop.\" ... \"The hook procedure should process a message in less time than the data entry specified in the LowLevelHooksTimeout value in HKEY_CURRENT_USER\\\\Control Panel\\\\Desktop... However, on Windows 7 and later, the hook is silently removed without being called. There is no way for the application to know whether the hook is removed.\" ... \"Windows 10 version 1709 and later The maximum timeout value the system allows is 1000 milliseconds\" ... \"it should run the hooks on a dedicated thread that passes the work off to a worker thread and then immediately returns\" ... \"the callback function is called before the asynchronous state of the key is updated. Consequently, the asynchronous state of the key cannot be determined by calling GetAsyncKeyState from within the callback function.\""
  },
  {
    "repo": "microsoft/win32 docs",
    "path": "api/winuser/nf-winuser-setwindowshookexw.md",
    "line_start": 1,
    "line_end": 1,
    "excerpt": "WH_KEYBOARD_LL | Global only. ... \"Windows Store apps: If dwThreadId is zero, then window hook DLLs are not loaded in-process for the Windows Store app processes ... unless they are installed by either UIAccess processes (accessibility tools).\""
  },
  {
    "repo": "microsoft/windows-rs (windows 0.62.2)",
    "path": "src/Windows/Win32/UI/WindowsAndMessaging/mod.rs",
    "line_start": 2316,
    "line_end": 4256,
    "excerpt": "pub unsafe fn SetWindowsHookExW(idhook: WINDOWS_HOOK_ID, lpfn: HOOKPROC, hmod: Option<HINSTANCE>, dwthreadid: u32) -> windows_core::Result<HHOOK>\npub struct KBDLLHOOKSTRUCT { pub vkCode: u32, pub scanCode: u32, pub flags: KBDLLHOOKSTRUCT_FLAGS, pub time: u32, pub dwExtraInfo: usize }\npub const WH_KEYBOARD_LL: WINDOWS_HOOK_ID = WINDOWS_HOOK_ID(13i32);\npub const LLKHF_INJECTED: KBDLLHOOKSTRUCT_FLAGS = KBDLLHOOKSTRUCT_FLAGS(16u32);"
  },
  {
    "repo": "microsoft/windows-rs (windows 0.62.2)",
    "path": "src/Windows/Win32/UI/Input/KeyboardAndMouse/mod.rs",
    "line_start": 166,
    "line_end": 562,
    "excerpt": "pub unsafe fn SendInput(pinputs: &[INPUT], cbsize: i32) -> u32\npub struct KEYBDINPUT { pub wVk: VIRTUAL_KEY, pub wScan: u16, pub dwFlags: KEYBD_EVENT_FLAGS, pub time: u32, pub dwExtraInfo: usize }\npub const KEYEVENTF_UNICODE: KEYBD_EVENT_FLAGS = KEYBD_EVENT_FLAGS(4u32);"
  },
  {
    "repo": "microsoft/windows-rs (windows 0.62.2)",
    "path": "src/Windows/Win32/UI/WindowsAndMessaging/mod.rs",
    "line_start": 866,
    "line_end": 1187,
    "excerpt": "pub unsafe fn GetForegroundWindow() -> HWND\npub unsafe fn GetWindowTextW(hwnd: HWND, lpstring: &mut [u16]) -> i32\npub unsafe fn GetWindowThreadProcessId(hwnd: HWND, lpdwprocessid: Option<*mut u32>) -> u32"
  },
  {
    "repo": "microsoft/windows-rs (windows 0.62.2)",
    "path": "src/Windows/Win32/System/Threading/mod.rs",
    "line_start": 1258,
    "line_end": 1261,
    "excerpt": "pub unsafe fn QueryFullProcessImageNameW(hprocess: HANDLE, dwflags: PROCESS_NAME_FORMAT, lpexename: windows_core::PWSTR, lpdwsize: *mut u32) -> windows_core::Result<()>"
  },
  {
    "repo": "leexgone/uiautomation-rs (uiautomation 0.24.0)",
    "path": "src/core.rs",
    "line_start": 84,
    "line_end": 160,
    "excerpt": "/// This method initializes the COM library each time, sets the thread's concurrency model as `COINIT_MULTITHREADED`.\npub fn new() -> Result<UIAutomation> { let result = unsafe { CoInitializeEx(None, COINIT_MULTITHREADED) }; if result.is_ok() { UIAutomation::new_direct() } else { Err(result.into()) } }\npub fn new_direct() -> Result<UIAutomation> { let automation: IUIAutomation = unsafe { CoCreateInstance(&CUIAutomation, None, CLSCTX_ALL)? }; ... }\npub fn get_focused_element(&self) -> Result<UIElement> { let element = unsafe { self.automation.GetFocusedElement()? }; ... }"
  },
  {
    "repo": "leexgone/uiautomation-rs (uiautomation 0.24.0)",
    "path": "src/patterns.rs",
    "line_start": 1479,
    "line_end": 1900,
    "excerpt": "pub fn UITextPattern::get_selection(&self) -> Result<Vec<UITextRange>>\npub fn UITextPattern::get_document_range(&self) -> Result<UITextRange>\npub fn UITextRange::get_text(&self, max_length: i32) -> Result<String>\npub struct UIValuePattern { pattern: IUIAutomationValuePattern }\npub fn UIValuePattern::set_value(&self, value: &str) -> Result<()>\npub fn UIValuePattern::get_value(&self) -> Result<String>"
  },
  {
    "repo": "leexgone/uiautomation-rs (uiautomation 0.24.0)",
    "path": "src/core.rs",
    "line_start": 1053,
    "line_end": 1907,
    "excerpt": "pub fn set_focus(&self) -> Result<()>\npub fn get_property_value(&self, property: UIProperty) -> Result<Variant>\npub fn get_pattern<T: super::patterns::UIPattern + TryFrom<IUnknown, Error = Error>>(&self) -> Result<T>\n// UIMatcher builder: from(element), depth(u32), timeout(u64), name(S), classname(S), control_type(ControlType), find_first(), find_all()\n// src/types.rs:396  ValueValue = 30045i32"
  },
  {
    "repo": "1Password/arboard (3.6.1)",
    "path": "src/lib.rs",
    "line_start": 81,
    "line_end": 265,
    "excerpt": "pub fn new/get_text/set_text/set_html/get_image/set_image/clear/clear_with/get/set ; Get::text/image/html/file_list ; Set::text/html/image/file_list  -- entire format vocabulary; no EnumClipboardFormats equivalent, no sequence-number/changeCount API"
  },
  {
    "repo": "madsmtm/objc2 (objc2-app-kit 0.3.2)",
    "path": "src/generated/NSWorkspace.rs",
    "line_start": 298,
    "line_end": 298,
    "excerpt": "pub fn frontmostApplication(&self) -> Option<Retained<NSRunningApplication>>;"
  },
  {
    "repo": "madsmtm/objc2 (objc2-app-kit 0.3.2)",
    "path": "src/generated/NSRunningApplication.rs",
    "line_start": 136,
    "line_end": 161,
    "excerpt": "pub fn localizedName(&self) -> Option<Retained<NSString>>;\npub fn bundleIdentifier(&self) -> Option<Retained<NSString>>;\npub fn executableURL(&self) -> Option<Retained<NSURL>>;\npub fn processIdentifier(&self) -> libc::pid_t;"
  },
  {
    "repo": "apple developer documentation",
    "path": "CoreGraphics/CGEvent/tapCreate(tap:place:options:eventsOfInterest:callback:userInfo:)",
    "line_start": 1,
    "line_end": 1,
    "excerpt": "\"Your callback function is invoked from the run loop to which the event tap is added as a source. The thread safety of the callback is defined by the run loop's environment.\" ... \"Pass the event tap to CFMachPortCreateRunLoopSource to create a run loop event source. Call CFRunLoopAddSource to add the source to the appropriate run loop.\""
  },
  {
    "repo": "microsoft learn (UAC / UIAccess policy)",
    "path": "security-policy-settings/user-account-control-only-elevate-uiaccess-applications-that-are-installed-in-secure-locations",
    "line_start": 1,
    "line_end": 1,
    "excerpt": "\"UIPI implements restrictions in the Windows subsystem that prevent lower-privilege applications from sending messages or installing hooks in higher-privilege processes.\" ... \"A process that's started with UIAccess rights has the following abilities: Set the foreground window. Drive any application window by using the SendInput function... Use read input for all integrity levels by using low-level hooks, raw input, GetKeyState, GetAsyncKeyState, and GetKeyboardInput.\" ... \"Microsoft UI Automation cannot drive the UI graphics of elevated applications on the desktop without the ability to bypass the restrictions that UIPI implements.\""
  }
]


## [version]

"Verified 2026-08-01 against: global-hotkey 0.8.0 (upstream HEAD; crates.io 0.7.0), tauri-plugin-global-shortcut 2.3.1, handy-keys 0.3.3, rdev 0.6.0 (Narsil HEAD 2026-05-12), device_query 4.0.1, enigo 0.6.1 (HEAD 19f6b97, 2026-07-21), arboard 3.6.1, uiautomation 0.24.0, windows 0.62.2, objc2-core-graphics 0.3.2, objc2-application-services 0.3.2, objc2-app-kit 0.3.2, accessibility-sys 0.2.0, macos-accessibility-client 0.0.1, readkey 0.2.2. Prior art: cjpais/Handy (Rust + Tauri v2 PTT dictation app) HEAD 2026-08."


## [breaking_changes]

[
  "enigo: the `delay`/`set_delay` knob present in 0.2.x is GONE in 0.6.1 — `Settings` has no delay or interval field. Key-by-key pacing must be implemented by the caller.",
  "enigo 0.6 migrated its macOS backend from `core-graphics` types to `objc2-core-graphics` 0.3 / `objc2` 0.6 and its Windows backend to `windows` 0.62. If you pin an older `windows` (e.g. Handy's 0.61.3) you will link two versions of the windows crate — harmless but bloats the binary; unify on 0.62.",
  "global-hotkey 0.8.0 (upstream, unreleased at time of writing) bumped thiserror to 2 and moved to objc2 0.6 / objc2-app-kit 0.3; the crates.io 0.7.0 that tauri-plugin-global-shortcut 2.3.1 depends on is the older tree. The modifier-only limitation is identical in both.",
  "objc2 framework crates 0.3.x replaced `core-foundation`'s `CFStringRef`-style raw pointers with `CFRetained<T>` / `Option<&CFString>`. Mixing `accessibility-sys` (core-foundation-sys style) and `objc2-application-services` (objc2 style) in the same file means two CFString types — pick one family."
]

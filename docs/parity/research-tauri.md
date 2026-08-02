# LibTauriShell research
**Versions:** tauri 2.11.5 (git main, crates/tauri/Cargo.toml); tao 0.36.0; wry (git main); plugins-workspace: autostart 2.5.1, single-instance 2.4.3, deep-link 2.4.9, opener 2.5.4, notification 2.3.3, store 2.4.4, sql 2.4.0; tauri-nspanel 2.1.0
# Tauri v2 for a cross-platform menu-bar dictation shell

All findings below were read out of source: `tauri` **2.11.5** (github.com/tauri-apps/tauri, `crates/tauri`), `tao` **0.36.0**, `wry` (main), `plugins-workspace` (main), `tauri-nspanel` **2.1.0**. Local registry confirms published `tauri 2.10.3` has the same APIs.

---

## 1. Tray icon — **Verdict: works** (`tauri` 2.11.5, feature `tray-icon`)

`crates/tauri/src/tray/mod.rs`. Backed by the `tray-icon` crate; menus by `muda`.

```rust
use tauri::{image::Image, tray::{TrayIconBuilder, TrayIconEvent, MouseButton, MouseButtonState}};
use tauri::menu::{Menu, MenuItem, CheckMenuItem, Submenu, PredefinedMenuItem};

let mic  = Image::from_bytes(include_bytes!("../icons/tray-idle.png"))?;
let rec  = Image::from_bytes(include_bytes!("../icons/tray-rec.png"))?;

let auto_punct = CheckMenuItem::with_id(app, "auto_punct", "Auto punctuation", true, true, None::<&str>)?;
let lang = Submenu::with_items(app, "Language", true, &[
    &CheckMenuItem::with_id(app, "lang.en", "English", true, true,  None::<&str>)?,
    &CheckMenuItem::with_id(app, "lang.de", "German",  true, false, None::<&str>)?,
])?;
let menu = Menu::with_items(app, &[
    &MenuItem::with_id(app, "settings", "Settings…", true, Some("Cmd+,"))?,
    &auto_punct, &lang,
    &PredefinedMenuItem::separator(app)?,
    &MenuItem::with_id(app, "quit", "Quit", true, None::<&str>)?,
])?;

let tray = TrayIconBuilder::with_id("main")
    .icon(mic.clone())
    .icon_as_template(true)                 // macOS only: monochrome/auto dark-light
    .menu(&menu)
    .show_menu_on_left_click(false)         // default is TRUE — you must opt out
    .tooltip("Wispr")                       // Linux unsupported
    .on_menu_event(|app, ev| { /* ev.id() */ })
    .on_tray_icon_event(|tray, ev| match ev {
        TrayIconEvent::Click { button: MouseButton::Left, button_state: MouseButtonState::Up, .. } => { /* toggle */ }
        _ => {}
    })
    .build(app)?;

tray.set_icon(Some(rec))?;            // dynamic idle↔recording swap
tray.set_icon_as_template(true)?;     // macOS only, no-op elsewhere
```

**Platform differences (from doc comments in source):**
- `icon_as_template` / `set_icon_as_template`: **macOS only** (`#[cfg(target_os = "macos")]` in `set_icon_as_template`). On Windows supply a full-color 32×32 (and a 16×16) ICO/PNG; template rendering does not exist.
- `title()` (text next to the icon): **Windows unsupported**, macOS/Linux only.
- `tooltip()`: **Linux unsupported**; works on Windows + macOS.
- `show_menu_on_left_click` defaults **true**. On macOS a status item normally opens the menu on either button; setting `false` gives you raw `Click` events for both buttons and you must pop the menu yourself. `menu_on_left_click` is deprecated since 2.2.0 — use `show_menu_on_left_click`.
- `TrayIconEvent::DoubleClick` is **Windows only** (doc comment in `tray/mod.rs:87`).
- Each `Click` carries `button_state: Up|Down`; you get **both** Down and Up events on Windows, so filter on one or you fire twice.
- Right-click on macOS delivers `MouseButton::Right`; when a menu is attached the menu is shown by the OS regardless.

**Gotcha:** all tray/menu mutators are marshalled to the main thread via `run_item_main_thread!`; calling them from a Tokio worker is safe but asynchronous-ish. Keep `TrayIcon` alive (store it in state) — dropping it removes the icon.

---

## 2. Non-activating always-on-top overlay — **Verdict: partially built-in; `focused(false)` is NOT enough. Use `focusable(false)`.**

### The key discovery
`.focused(false)` only affects the *initial* show. In tao `platform_impl/macos/window.rs:628-634`:
```rust
if visible { if focused { window.ns_window.makeKeyAndOrderFront(None) } else { window.ns_window.orderFront(None) } }
```
and `set_visible(true)` (i.e. Tauri's `window.show()`) **always** calls `make_key_and_order_front_sync` (`window.rs:666-669`). So a window created with `focused(false)` **will steal focus the next time you `show()` it.** This alone would break your text-injection invariant.

The correct primitive is the *separate*, under-documented **`focusable(bool)`** (`crates/tauri/src/webview/webview_window.rs:876`, also `tauri.conf.json > app > windows[].focusable`):
- **macOS** (`tao macos/window.rs:414-432`): tao's `WINDOW_CLASS` overrides `canBecomeKeyWindow` and `canBecomeMainWindow` to return the `focusable` ivar. With `focusable(false)`, `makeKeyAndOrderFront:` cannot make it key — keyboard focus stays with the previous app.
- **Windows** (`tao windows/window_state.rs:293-295`): `if !FOCUSABLE { style_ex |= WS_EX_NOACTIVATE; }` — **Tauri already applies `WS_EX_NOACTIVATE` for you.** `focused(false)` separately maps to `MARKER_DONT_FOCUS` → `SW_SHOWNOACTIVATE` on first show (`window_state.rs:325-330`).

### Recommended builder (both platforms)
```rust
use tauri::{WebviewUrl, WebviewWindowBuilder};

let overlay = WebviewWindowBuilder::new(app, "overlay", WebviewUrl::App("overlay.html".into()))
    .focusable(false)                    // ← THE invariant: canBecomeKeyWindow=NO / WS_EX_NOACTIVATE
    .focused(false)                      // don't activate on first show
    .always_on_top(true)
    .decorations(false)
    .transparent(true)                    // macOS needs feature `macos-private-api` + tauri.conf app.macOSPrivateApi=true
    .skip_taskbar(true)                   // Windows/Linux; macOS: no-op (documented "macOS: Unsupported")
    .shadow(false)
    .resizable(false)
    .maximizable(false).minimizable(false).closable(false)
    .visible_on_all_workspaces(true)      // macOS only; setter is "Windows/iOS/Android: Unsupported"
    .accept_first_mouse(true)             // macOS only (wry doc: "only impacts macOS")
    .inner_size(360.0, 84.0)
    .visible(false)
    .build()?;
```

### macOS: do you still need an NSPanel?
**Yes, for two reasons** — but not for keyboard focus.
1. **App activation.** `canBecomeKeyWindow=NO` stops the *window* becoming key, but clicking it still activates *your application* (menu bar swaps, previous app deactivates). Only `NSWindowStyleMask::NonactivatingPanel` prevents app activation, and AppKit honours that bit **only on `NSPanel` instances** — setting it on tao's plain `NSWindow` subclass is silently ignored. tao has zero NSPanel support (grep for `NSPanel` in tao: no hits).
2. **Window level.** tao's `always_on_top` sets `NSFloatingWindowLevel` (`macos/window.rs:289, 1389-1396`) = level 3. That is *below* the status bar and it will **not** float over full-screen/Spaces apps. A dictation HUD needs `NSStatusWindowLevel` (25) or `NSScreenSaverWindowLevel`, plus `CanJoinAllSpaces | FullScreenAuxiliary | Stationary` collection behavior.

**Option A (recommended): `tauri-nspanel` 2.1.0** (`git = "https://github.com/ahkohd/tauri-nspanel", branch = "v2.1"`). It does the `object_setClass` swap onto a generated `NSPanel` subclass (`src/panel.rs:259-290, 569-586`) and exposes `StyleMask::nonactivating_panel()` (`builder.rs:421-425`), `PanelLevel`, `CollectionBehavior::can_join_all_spaces()/ignores_cycle()`, `set_level`, `set_collection_behavior`, `hides_on_deactivate`, `becomes_key_only_if_needed`.

```rust
use tauri_nspanel::{tauri_panel, PanelBuilder, PanelLevel, StyleMask, CollectionBehavior, WebviewWindowExt};

tauri_panel! { panel!(OverlayPanel { config: { can_become_key_window: false, is_floating_panel: true } }) }

// convert the window built above:
let panel = overlay.to_panel::<OverlayPanel>()?;
panel.set_style_mask(StyleMask::new().borderless().nonactivating_panel().raw());
panel.set_level(PanelLevel::Status.raw());          // 25 — above normal apps
panel.set_collection_behavior(
    CollectionBehavior::new().can_join_all_spaces().stationary().full_screen_auxiliary().ignores_cycle().raw());
```

**Option B: raw objc2 (no extra crate).** You cannot legitimately gain non-activation without the class swap, but you *can* fix level + spaces, which combined with `focusable(false)` covers the focus invariant:

```rust
#[cfg(target_os = "macos")]
unsafe fn harden_overlay(win: &tauri::WebviewWindow) -> tauri::Result<()> {
    use objc2::rc::Retained;
    use objc2_app_kit::{NSWindow, NSWindowCollectionBehavior};

    let ptr = win.ns_window()? as *mut NSWindow;   // autoreleased +0 pointer
    let w: &NSWindow = &*ptr;

    w.setLevel(25);                                // NSStatusWindowLevel
    w.setCollectionBehavior(
        NSWindowCollectionBehavior::CanJoinAllSpaces
            | NSWindowCollectionBehavior::Stationary
            | NSWindowCollectionBehavior::FullScreenAuxiliary
            | NSWindowCollectionBehavior::IgnoresCycle,
    );
    w.setHidesOnDeactivate(false);
    w.setOpaque(false);
    w.setHasShadow(false);
    // orderFrontRegardless(): show WITHOUT activating the app — use instead of win.show()
    w.orderFrontRegardless();
    let _ = Retained::retain(ptr);                  // keep it alive if you store it
    Ok(())
}
```
Pin `objc2 = "0.6"`, `objc2-app-kit = "0.3.2"` to match `crates/tauri/Cargo.toml:106,117` or the pointer types won't unify.

> **Important macOS gotcha:** because `Window::show()` → `makeKeyAndOrderFront:`, prefer `orderFrontRegardless()` for the overlay and `orderOut(None)` for hiding. `focusable(false)` makes `makeKeyAndOrderFront:` harmless, but `orderFrontRegardless` additionally avoids any app-activation side effects.

### Windows: is `WS_EX_NOACTIVATE` needed manually?
**No — Tauri sets it for you via `.focusable(false)`.** Apply it manually only if you also want `WS_EX_TOOLWINDOW` (keeps the window out of Alt-Tab, which `skip_taskbar` does not fully guarantee), or if you toggle focusability late. HWND comes from `WebviewWindow::hwnd() -> Result<HWND>` (`window/mod.rs:1681-1694`, decoded from `RawWindowHandle::Win32`).

```rust
#[cfg(windows)]
fn harden_overlay(win: &tauri::WebviewWindow) -> tauri::Result<()> {
    use windows::Win32::UI::WindowsAndMessaging::{
        GetWindowLongPtrW, SetWindowLongPtrW, SetWindowPos,
        GWL_EXSTYLE, WS_EX_NOACTIVATE, WS_EX_TOOLWINDOW, WS_EX_TOPMOST,
        HWND_TOPMOST, SWP_NOMOVE, SWP_NOSIZE, SWP_NOACTIVATE, SWP_FRAMECHANGED,
    };
    let hwnd = win.hwnd()?;                       // windows::Win32::Foundation::HWND
    unsafe {
        let ex = GetWindowLongPtrW(hwnd, GWL_EXSTYLE) as u32;
        let ex = ex | WS_EX_NOACTIVATE.0 | WS_EX_TOOLWINDOW.0 | WS_EX_TOPMOST.0;
        SetWindowLongPtrW(hwnd, GWL_EXSTYLE, ex as isize);
        SetWindowPos(hwnd, Some(HWND_TOPMOST), 0, 0, 0, 0,
                     SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE | SWP_FRAMECHANGED)?;
    }
    Ok(())
}
```
Use `windows = "0.61"` — that's `crates/tauri/Cargo.toml:138`. A different major version gives you a *different* `HWND` type and won't compile; the escape hatch is `HWND(win.hwnd()?.0 as _)`.

Also set `.no_redirection_bitmap(true)` (Windows-only builder method) on a transparent overlay to avoid the white flash before first paint.

---

## 3. Click-through — **Verdict: works on both, but it is whole-window only**

`WebviewWindow::set_ignore_cursor_events(&self, ignore: bool) -> Result<()>`
- macOS → `NSWindow::setIgnoresMouseEvents:` dispatched to the main queue (`tao macos/util/async.rs:257-263`; comment: *"`setIgnoresMouseEvents_:` isn't thread-safe, and fails silently"*).
- Windows → `WS_EX_TRANSPARENT | WS_EX_LAYERED` (`tao windows/window_state.rs:282-284`).
- iOS/Android return `NotSupported`. Linux via GDK input region.

**There is no per-region API.** Two working patterns:

1. **Two windows (recommended, deterministic).** Window A = passive HUD, permanently `set_ignore_cursor_events(true)`. Window B = small interactive control strip (retry / cancel buttons), `focusable(false)` + `accept_first_mouse(true)`, cursor events enabled, shown only when there's something to click. Neither ever takes keyboard focus, and B's clicks reach the webview on the first click thanks to `accept_first_mouse`.
2. **Dynamic toggle.** Keep the overlay click-through, and from the Rust side poll/observe `AppHandle::cursor_position() -> Result<PhysicalPosition<f64>>` (`app.rs:908`) against the button rect; call `set_ignore_cursor_events(false)` on enter and `true` on leave. Cheap, but has a one-frame race and needs a timer. Note that while `ignore == true` the webview receives **no** pointer events at all, so JS `mouseenter` cannot drive this.

---

## 4. Dock / taskbar hiding — **Verdict: works**

**macOS:**
```rust
use tauri::ActivationPolicy;
app.set_activation_policy(ActivationPolicy::Accessory);            // in setup(), &mut App
app.handle().set_activation_policy(ActivationPolicy::Regular)?;    // AppHandle, at runtime → Result<()>
```
`App::set_activation_policy(&mut self, ActivationPolicy)` (`app.rs:1284`) and `AppHandle::set_activation_policy(&self, …) -> Result<()>` (`app.rs:640`). Doc: *"It is set to `NSApplicationActivationPolicyRegular` by default."* Runtime switching both ways is supported, which is exactly what your "Show in Dock" setting needs.

**Gotcha:** switching Regular→Accessory at runtime means the dock icon flashes for a moment during launch. `LSUIElement` is **not** a Tauri config key (grep for `LSUIElement` across the whole tauri repo: zero hits). Set it yourself through `tauri.conf.json > bundle > macOS > infoPlist` — the `info_plist: Option<String>` field is documented as *"Path to a Info.plist file to merge with the default Info.plist"* (`tauri-utils/src/config.rs:662`). Ship `LSUIElement = true` there and call `set_activation_policy(Regular)` when the user opts in.

**Windows:** `.skip_taskbar(true)` on every window plus `WS_EX_TOOLWINDOW` on the overlay. `set_skip_taskbar` is explicitly *"macOS: Unsupported"* (`window/mod.rs:2176-2177`) — that's fine, macOS uses activation policy instead. A tray-only app with all windows hidden shows nothing in the taskbar.

---

## 5. Plugins (verified versions from `plugins-workspace` main)

| Plugin | Version | Verdict / notes |
|---|---|---|
| `tauri-plugin-autostart` | **2.5.1** | Works. `Builder::new().args([..]).app_name("…").macos_launcher(MacosLauncher::LaunchAgent).build()`. macOS has two backends — `MacosLauncher::LaunchAgent` (default, writes a plist to `~/Library/LaunchAgents`) or `AppleScript` (System Events login items, triggers a TCC automation prompt). Windows uses `HKCU\…\Run`. **Use LaunchAgent** to avoid the automation permission dialog. Deprecated free fn `init(macos_launcher, args)` still exists. |
| `tauri-plugin-single-instance` | **2.4.3** | Works on Win + macOS (Android/iOS unsupported). README: *"plugins run in the order they were added… make sure that this plugin is registered **first**."* Has an optional **`deep-link` cargo feature** that forwards the second instance's argv into the deep-link plugin (`src/lib.rs:50-53, 72-74`) — enable it. |
| `tauri-plugin-deep-link` | **2.4.9** | Works, but registration differs sharply. Config: `tauri.conf.json > plugins > deep-link > desktop.schemes: ["wispr-flow"]`. **macOS**: registration is *static only* — the bundler writes `CFBundleURLTypes`/`CFBundleURLSchemes` into Info.plist (`tauri-bundler/src/bundle/macos/app.rs:298-312`); `register/unregister/is_registered` all return `Error::UnsupportedPlatform` on macOS. It arrives as an AppKit `openURLs:` event → `on_open_url`. **Windows**: registration is *runtime/registry* — `register()` writes `HKCU\Software\Classes\<scheme>` via `windows-registry`; the NSIS/WiX installer also registers it. `unregister` *"Requires admin rights if the protocol is registered on local machine"*. The URL arrives as **a new process with the URL as its only argv** — so on Windows you MUST pair it with `single-instance` (+ its `deep-link` feature) or every OAuth callback spawns a second app. Dev builds on Windows need an explicit `register("wispr-flow")` call since there's no installer. API: `app.deep_link().on_open_url(|ev| …) -> EventId`, `get_current() -> Result<Option<Vec<Url>>>`, `register_all()`. |
| `tauri-plugin-opener` | **2.5.4** | Works. This is the v2 replacement for v1's `shell.open` — use it for "open dashboard in browser". |
| `tauri-plugin-notification` | **2.3.3** | Works both. macOS: notifications only show from a **signed, bundled** `.app` — nothing appears when running `cargo run` unbundled. Windows 10/11 toasts need a valid AppUserModelID, which the installer provides; unbundled dev builds are unreliable. |
| `tauri-plugin-store` | **2.4.4** | Works. Good for settings JSON (small key/value). Not for history. |
| `tauri-plugin-sql` | **2.4.0** | Works (`features = ["sqlite"]`, sqlx + `runtime-tokio`). **Recommendation: use `rusqlite` directly instead.** The plugin's value proposition is exposing SQL *to the frontend* over IPC — a security and typing liability, and it drags in the whole sqlx async stack. Your transcript history is written by Rust, so `rusqlite` (with `bundled` feature for a self-contained SQLite on Windows) is simpler, synchronous, and lets you keep SQL out of the webview. Take the plugin only if the frontend genuinely needs ad-hoc queries. |

---

## 6. Multi-window + staying alive with zero windows — **Verdict: works**

Create on demand and reuse by label:
```rust
fn open_settings(app: &tauri::AppHandle) -> tauri::Result<()> {
    if let Some(w) = app.get_webview_window("settings") { w.show()?; w.set_focus()?; return Ok(()); }
    WebviewWindowBuilder::new(app, "settings", WebviewUrl::App("settings.html".into()))
        .title("Settings").inner_size(720.0, 520.0).build()?;
    Ok(())
}
```

**Preventing exit.** In `tauri-runtime-wry/src/lib.rs:4256-4266`, on `TaoWindowEvent::Destroyed`, when the window map becomes empty the runtime fires `RunEvent::ExitRequested { code: None, tx }` and exits *unless* prevented. So:

```rust
app.run(|_app, event| {
    if let tauri::RunEvent::ExitRequested { code: None, api, .. } = event {
        api.prevent_exit();     // no-op when code == Some(RESTART_EXIT_CODE)
    }
});
```
`ExitRequestApi::prevent_exit(&self)` (`app.rs:90-94`). Matching on `code: None` is important: it prevents only *implicit* exits, so your tray "Quit" (`app.exit(0)`, which sends `code: Some(0)`) still works.

Better still, **hide instead of close** so windows are never destroyed:
```rust
win.on_window_event(|e| if let tauri::WindowEvent::CloseRequested { api, .. } = e {
    api.prevent_close();        // CloseRequestApi::prevent_close, app.rs:103
    // then window.hide()
});
```
This is the same on macOS and Windows.

---

## 7. Packaging — **Verdict: works**

`BundleType` (`tauri-utils/src/config.rs:132-147`): `Deb | Rpm | AppImage | **Msi** | **Nsis** | **App** | **Dmg**`.

**macOS** → `.app` then `.dmg`.
- Signing config lives at `bundle > macOS`: `signing_identity`, `hardened_runtime` (**defaults to `true`**, `config.rs:656`), `provider_short_name`, `entitlements`, `infoPlist`.
- Env overrides: `APPLE_SIGNING_IDENTITY`, `APPLE_PROVIDER_SHORT_NAME` (`tauri-cli/src/interface/rust.rs:1467,1477`), CI cert import via `APPLE_CERTIFICATE` + `APPLE_CERTIFICATE_PASSWORD`.
- Notarization runs `xcrun notarytool submit --wait` then staples (`tauri-macos-sign/src/lib.rs:121-235`); credentials are either `APPLE_ID` + `APPLE_PASSWORD` + `APPLE_TEAM_ID`, or `APPLE_API_KEY` + `APPLE_API_ISSUER` + `APPLE_API_KEY_PATH` (`tauri-bundler/.../macos/sign.rs:98-118`). `--skip-stapling` exists for the first (multi-hour) submission.
- **Non-negotiable for you:** microphone + Accessibility/Input-Monitoring usage means hardened runtime with `com.apple.security.device.audio-input` entitlement, `NSMicrophoneUsageDescription` in `infoPlist`, and notarization — otherwise Gatekeeper blocks and TCC prompts misbehave.

**Windows** → **MSI (WiX v3)** and/or **NSIS (`.exe`)**.
- **Use NSIS.** WiX v3 is x64/x86 only; NSIS is the path that supports **arm64** and per-user (no-admin) installs, and it's what registers your `wispr-flow://` scheme without elevation. Config structs: `WixConfig` (`config.rs:736`), `NsisConfig` (`config.rs:847`), `NsisCompression` (`config.rs:806`).
- **WebView2 bootstrapping: handled, and on by default.** `WindowsConfig::webview_install_mode` (`config.rs:1055`) defaults to `WebviewInstallMode::DownloadBootstrapper { silent: true }` (`config.rs:998-1001`). Other variants: `Skip`, `EmbedBootstrapper`, `OfflineInstaller`, `FixedRuntime`. Windows 11 ships WebView2; Windows 10 may not, so keep the default (or `EmbedBootstrapper` if your users are offline). Also see `minimum_webview2_version`.
- Sign with `certificateThumbprint` / `digestAlgorithm` / `timestampUrl` under `bundle > windows`, or SmartScreen will warn.


## api
- {"signature": "TrayIconBuilder::<R>::with_id<I: Into<TrayIconId>>(id: I) -> Self", "description": "Start a tray icon builder with a stable id (tauri::tray, feature `tray-icon`)."}
- {"signature": "TrayIconBuilder::icon(self, icon: Image<'_>) -> Self", "description": "Sets the tray image."}
- {"signature": "TrayIconBuilder::icon_as_template(self, is_template: bool) -> Self", "description": "macOS only \u2014 render the icon as an NSImage template (monochrome, auto light/dark)."}
- {"signature": "TrayIconBuilder::show_menu_on_left_click(self, enable: bool) -> Self", "description": "Default true. Linux unsupported. `menu_on_left_click` is deprecated since 2.2.0."}
- {"signature": "TrayIconBuilder::on_tray_icon_event<F: Fn(&TrayIcon<R>, TrayIconEvent) + Sync + Send + 'static>(self, f: F) -> Self", "description": "Click/DoubleClick(Windows only)/Enter/Move/Leave with MouseButton and MouseButtonState."}
- {"signature": "TrayIcon::set_icon(&self, icon: Option<Image<'_>>) -> tauri::Result<()>", "description": "Dynamic idle\u2194recording icon swap; runs on the main thread."}
- {"signature": "TrayIcon::set_icon_as_template(&self, is_template: bool) -> tauri::Result<()>", "description": "macOS only (cfg-gated); no-op elsewhere."}
- {"signature": "CheckMenuItem::with_id<M, I, T, A>(manager: &M, id: I, text: T, enabled: bool, checked: bool, accelerator: Option<A>) -> tauri::Result<Self>", "description": "Checkmark menu item; `set_checked(bool)` / `is_checked()` at runtime."}
- {"signature": "Submenu::with_items<M: Manager<R>, S: AsRef<str>>(manager: &M, text: S, enabled: bool, items: &[&dyn IsMenuItem<R>]) -> tauri::Result<Self>", "description": "Native submenu; `append`/`append_items` mutate it later."}
- {"signature": "WebviewWindowBuilder::new<L: Into<String>>(manager: &'a M, label: L, url: WebviewUrl) -> Self", "description": "tauri::webview::WebviewWindowBuilder entry point; finish with `.build() -> Result<WebviewWindow<R>>`."}
- {"signature": "WebviewWindowBuilder::focusable(self, focusable: bool) -> Self", "description": "THE non-activating switch: macOS canBecomeKeyWindow/canBecomeMainWindow = NO; Windows WS_EX_NOACTIVATE."}
- {"signature": "WebviewWindowBuilder::focused(self, focused: bool) -> Self", "description": "Initial activation only \u2014 macOS orderFront vs makeKeyAndOrderFront; Windows SW_SHOWNOACTIVATE. Not sufficient alone."}
- {"signature": "WebviewWindowBuilder::always_on_top(self, v: bool) -> Self", "description": "macOS: NSFloatingWindowLevel (3) \u2014 below status bar and below full-screen apps."}
- {"signature": "WebviewWindowBuilder::visible_on_all_workspaces(self, v: bool) -> Self", "description": "macOS only; the runtime setter documents Windows/iOS/Android as Unsupported."}
- {"signature": "WebviewWindowBuilder::skip_taskbar(self, skip: bool) -> Self", "description": "Windows/Linux; documented \"macOS: Unsupported\"."}
- {"signature": "WebviewWindowBuilder::accept_first_mouse(self, accept: bool) -> Self", "description": "macOS only (wry): first click on an inactive window reaches the webview."}
- {"signature": "WebviewWindowBuilder::no_redirection_bitmap(self, enable: bool) -> Self", "description": "Windows only \u2014 WS_EX_NOREDIRECTIONBITMAP, avoids white flash on transparent windows."}
- {"signature": "WebviewWindow::ns_window(&self) -> tauri::Result<*mut std::ffi::c_void>", "description": "macOS NSWindow pointer (autoreleased) for objc2 / NSPanel work."}
- {"signature": "WebviewWindow::hwnd(&self) -> tauri::Result<windows::Win32::Foundation::HWND>", "description": "Windows HWND; `windows` crate must be 0.61 to match tauri's type."}
- {"signature": "WebviewWindow::set_ignore_cursor_events(&self, ignore: bool) -> tauri::Result<()>", "description": "Whole-window click-through: macOS setIgnoresMouseEvents:, Windows WS_EX_TRANSPARENT|WS_EX_LAYERED."}
- {"signature": "WebviewWindow::set_focusable(&self, focusable: bool) -> tauri::Result<()>", "description": "Runtime toggle. macOS caveat: if already focused you cannot unfocus it via this call."}
- {"signature": "AppHandle::set_activation_policy(&self, policy: ActivationPolicy) -> tauri::Result<()>", "description": "macOS only; Accessory hides the Dock icon, Regular restores it. Default is Regular."}
- {"signature": "AppHandle::cursor_position(&self) -> tauri::Result<PhysicalPosition<f64>>", "description": "Global cursor position \u2014 used to toggle click-through over interactive regions."}
- {"signature": "ExitRequestApi::prevent_exit(&self)", "description": "Called from RunEvent::ExitRequested to keep a tray-only app alive with zero windows."}
- {"signature": "CloseRequestApi::prevent_close(&self)", "description": "Called from WindowEvent::CloseRequested so windows hide instead of being destroyed."}
- {"signature": "WebviewWindowExt::to_panel<P: FromWindow<R> + 'static>(&self) -> tauri::Result<PanelHandle<R>>", "description": "tauri-nspanel 2.1.0 \u2014 swaps the NSWindow's class to a generated NSPanel subclass so NonactivatingPanel takes effect."}
- {"signature": "StyleMask::new().borderless().nonactivating_panel()", "description": "tauri-nspanel: NSWindowStyleMask::NonactivatingPanel \u2014 the only way to stop app activation on click."}


## breaking_changes
- tauri v1 `SystemTray`/`SystemTrayMenu` are gone; v2 uses `tauri::tray::TrayIconBuilder` + `tauri::menu::*` (muda), and tray support is behind the `tray-icon` cargo feature.
- `TrayIconBuilder::menu_on_left_click` is deprecated since tauri 2.2.0 in favour of `show_menu_on_left_click`.
- v1's `tauri-plugin-shell` `open` for URLs is superseded by `tauri-plugin-opener` (2.5.4).
- `ns_window()` now returns an objc2 `Retained::autorelease_ptr` derived pointer; tauri pins objc2 0.6 / objc2-app-kit 0.3.2 and `windows` 0.61 — mismatched versions in your own Cargo.toml will not typecheck.


## caveats
- `focused(false)` is NOT the focus guard. `Window::show()` calls `makeKeyAndOrderFront:` on macOS every time, so a `focused(false)` window steals focus on the second show. Use `focusable(false)` (and prefer `orderFrontRegardless()` on macOS).
- `focusable(false)` prevents keyboard focus but NOT application activation on macOS. Only NSWindowStyleMask::NonactivatingPanel on an actual NSPanel prevents activation, and tao has no NSPanel support — use tauri-nspanel or an object_setClass swap.
- tao's `always_on_top` = NSFloatingWindowLevel (3), which is below the menu bar and does not float over full-screen apps. Set NSStatusWindowLevel (25) + CanJoinAllSpaces|FullScreenAuxiliary manually.
- `set_ignore_cursor_events` is per-window only. While enabled the webview receives no pointer events, so JS hover cannot re-enable it — use a second small interactive window or poll `AppHandle::cursor_position()`.
- `transparent(true)` on macOS requires the `macos-private-api` cargo feature plus `app.macOSPrivateApi: true` in tauri.conf.json.
- `LSUIElement` is not a Tauri config key anywhere in the repo — set it via `bundle.macOS.infoPlist` merge, otherwise the Dock icon flashes before `set_activation_policy(Accessory)` runs.
- On Windows a deep link launches a NEW process with the URL as argv; without `tauri-plugin-single-instance` (with its `deep-link` feature, registered FIRST) every OAuth callback spawns another app instance.
- deep-link `register`/`unregister`/`is_registered` return `UnsupportedPlatform` on macOS — the scheme comes solely from the bundled Info.plist, so dev-mode deep links on macOS only work from a built .app.
- WiX/MSI does not support arm64; use NSIS for Windows on ARM and for per-user installs.
- macOS notifications and toasts require a signed/bundled app; they silently do nothing in `cargo run` dev builds.


## sources
- {"repo": "tauri-apps/tauri", "path": "crates/tauri/src/tray/mod.rs", "line_start": 294, "line_end": 320, "excerpt": "/// Use the icon as a [template](...). **macOS only**.\n  pub fn icon_as_template(mut self, is_template: bool) -> Self { ... }\n  /// Whether to show the tray menu on left click or not, default is `true`.\n  /// - **Linux:** Unsupported.\n  pub fn show_menu_on_left_click(mut self, enable: bool) -> Self { ... }"}
- {"repo": "tauri-apps/tauri", "path": "crates/tauri/src/tray/mod.rs", "line_start": 86, "line_end": 96, "excerpt": "/// A double click happened on the tray icon. **Windows Only**\n  DoubleClick { id: TrayIconId, /* position, rect */ button: MouseButton },"}
- {"repo": "tauri-apps/tao", "path": "src/platform_impl/macos/window.rs", "line_start": 628, "line_end": 634, "excerpt": "if visible {\n      if focused {\n        // Tightly linked with `app_state::window_activation_hack`\n        unsafe { window.ns_window.makeKeyAndOrderFront(None) };\n      } else {\n        unsafe { window.ns_window.orderFront(None) };\n      }\n    }"}
- {"repo": "tauri-apps/tao", "path": "src/platform_impl/macos/window.rs", "line_start": 666, "line_end": 670, "excerpt": "pub fn set_visible(&self, visible: bool) {\n    match visible {\n      true => unsafe { util::make_key_and_order_front_sync(&self.ns_window) },\n      false => unsafe { util::order_out_sync(&self.ns_window) },"}
- {"repo": "tauri-apps/tao", "path": "src/platform_impl/macos/window.rs", "line_start": 414, "line_end": 432, "excerpt": "decl.add_method(sel!(canBecomeMainWindow), is_focusable as extern \"C\" fn(_, _) -> _);\n  decl.add_method(sel!(canBecomeKeyWindow), is_focusable as extern \"C\" fn(_, _) -> _);\n  ...\n  decl.add_ivar::<Bool>(CStr::from_bytes_with_nul(b\"focusable\\0\").unwrap());\n  ...\nextern \"C\" fn is_focusable(this: &Object, _: Sel) -> Bool { unsafe { *(this.get_ivar(\"focusable\")) } }"}
- {"repo": "tauri-apps/tao", "path": "src/platform_impl/windows/window_state.rs", "line_start": 292, "line_end": 330, "excerpt": "if !self.contains(WindowFlags::FOCUSABLE) {\n      style_ex |= WS_EX_NOACTIVATE;\n    }\n...\n          if self.contains(WindowFlags::MARKER_DONT_FOCUS) {\n            self.set(WindowFlags::MARKER_DONT_FOCUS, false);\n            SW_SHOWNOACTIVATE\n          } else { SW_SHOW }"}
- {"repo": "tauri-apps/tao", "path": "src/platform_impl/macos/window.rs", "line_start": 1388, "line_end": 1396, "excerpt": "pub fn set_always_on_top(&self, always_on_top: bool) {\n    let level = if always_on_top { ffi::NSWindowLevel::NSFloatingWindowLevel } else { ffi::NSWindowLevel::NSNormalWindowLevel };\n    unsafe { util::set_level_async(&self.ns_window, level) };\n  }"}
- {"repo": "tauri-apps/tauri", "path": "crates/tauri/src/webview/webview_window.rs", "line_start": 874, "line_end": 879, "excerpt": "/// Whether the window will be focusable or not.\n  #[must_use]\n  pub fn focusable(mut self, focusable: bool) -> Self {\n    self.window_builder = self.window_builder.focusable(focusable);\n    self\n  }"}
- {"repo": "tauri-apps/tauri", "path": "crates/tauri/src/window/mod.rs", "line_start": 1644, "line_end": 1660, "excerpt": "#[cfg(target_os = \"macos\")]\n  pub fn ns_window(&self) -> crate::Result<*mut std::ffi::c_void> { ... if let raw_window_handle::RawWindowHandle::AppKit(h) = handle.as_raw() { let view: &objc2_app_kit::NSView = unsafe { h.ns_view.cast().as_ref() }; let ns_window = view.window().expect(...); Ok(objc2::rc::Retained::autorelease_ptr(ns_window).cast()) }"}
- {"repo": "tauri-apps/tauri", "path": "crates/tauri/src/window/mod.rs", "line_start": 1680, "line_end": 1694, "excerpt": "#[cfg(windows)]\n  pub fn hwnd(&self) -> crate::Result<HWND> { ... if let raw_window_handle::RawWindowHandle::Win32(h) = handle.as_raw() { Ok(HWND(h.hwnd.get() as _)) }"}
- {"repo": "tauri-apps/tao", "path": "src/platform_impl/macos/util/async.rs", "line_start": 257, "line_end": 263, "excerpt": "// `setIgnoresMouseEvents_:` isn't thread-safe, and fails silently.\npub unsafe fn set_ignore_mouse_events(ns_window: &NSWindow, ignore: bool) { ... ns_window.setIgnoresMouseEvents(ignore); }"}
- {"repo": "tauri-apps/tao", "path": "src/platform_impl/windows/window_state.rs", "line_start": 282, "line_end": 284, "excerpt": "if self.contains(WindowFlags::IGNORE_CURSOR_EVENT) {\n      style_ex |= WS_EX_TRANSPARENT | WS_EX_LAYERED;\n    }"}
- {"repo": "tauri-apps/tauri", "path": "crates/tauri/src/app.rs", "line_start": 627, "line_end": 644, "excerpt": "/// Sets the activation policy for the application. It is set to `NSApplicationActivationPolicyRegular` by default.\n  #[cfg(target_os = \"macos\")]\n  pub fn set_activation_policy(&self, activation_policy: ActivationPolicy) -> crate::Result<()>"}
- {"repo": "tauri-apps/tauri", "path": "crates/tauri-runtime-wry/src/lib.rs", "line_start": 4253, "line_end": 4266, "excerpt": "let is_empty = windows.0.borrow().is_empty();\n              if is_empty {\n                let (tx, rx) = channel();\n                callback(RunEvent::ExitRequested { code: None, tx });\n                let should_prevent = matches!(rx.try_recv(), Ok(ExitRequestedEventAction::Prevent));\n                if !should_prevent { *control_flow = ControlFlow::Exit; }"}
- {"repo": "tauri-apps/tauri", "path": "crates/tauri/src/app.rs", "line_start": 89, "line_end": 94, "excerpt": "pub fn prevent_exit(&self) {\n    if self.code != Some(RESTART_EXIT_CODE) {\n      self.tx.send(ExitRequestedEventAction::Prevent).unwrap();\n    }\n  }"}
- {"repo": "tauri-apps/plugins-workspace", "path": "plugins/deep-link/src/lib.rs", "line_start": 186, "line_end": 190, "excerpt": "/// On Linux and Windows the deep links trigger a new app instance with the deep link URL as its only argument."}
- {"repo": "tauri-apps/plugins-workspace", "path": "plugins/deep-link/src/lib.rs", "line_start": 257, "line_end": 262, "excerpt": "/// - **macOS / Android / iOS**: Unsupported, will return [`Error::UnsupportedPlatform`].\n        pub fn register<S: AsRef<str>>(&self, _protocol: S) -> crate::Result<()> { #[cfg(windows)] { ... } }"}
- {"repo": "tauri-apps/tauri", "path": "crates/tauri-bundler/src/bundle/macos/app.rs", "line_start": 298, "line_end": 310, "excerpt": "plist.insert(\"CFBundleURLTypes\".into(), plist::Value::Array(protocols.iter()... dict.insert(\"CFBundleURLSchemes\".into(), ...)"}
- {"repo": "tauri-apps/tauri", "path": "crates/tauri-utils/src/config.rs", "line_start": 998, "line_end": 1001, "excerpt": "impl Default for WebviewInstallMode {\n  fn default() -> Self {\n    Self::DownloadBootstrapper { silent: true }\n  }"}
- {"repo": "tauri-apps/tauri", "path": "crates/tauri-utils/src/config.rs", "line_start": 652, "line_end": 662, "excerpt": "pub signing_identity: Option<String>,\n  /// Whether the codesign should enable hardened runtime ... \n  #[serde(alias = \"hardened-runtime\", default = \"default_true\")]\n  pub hardened_runtime: bool,\n  ...\n  /// Path to a Info.plist file to merge with the default Info.plist."}
- {"repo": "ahkohd/tauri-nspanel", "path": "src/builder.rs", "line_start": 421, "line_end": 425, "excerpt": "/// Window is a non-activating panel\n    pub fn nonactivating_panel(mut self) -> Self {\n        self.0 |= objc2_app_kit::NSWindowStyleMask::NonactivatingPanel;\n        self\n    }"}
- {"repo": "ahkohd/tauri-nspanel", "path": "src/panel.rs", "line_start": 569, "line_end": 586, "excerpt": "unsafe extern \"C\" { fn object_setClass(obj: *mut NSObject, cls: *const AnyClass) -> *const AnyClass; }\n// Change the window class to our custom panel class\nobject_setClass(ns_window as *mut NSObject, [<Raw $class_name>]::class());"}
- {"repo": "tauri-apps/wry", "path": "src/lib.rs", "line_start": 1413, "line_end": 1418, "excerpt": "/// Sets whether clicking an inactive window also clicks through to the webview. Default is `false`.\n  /// ## Platform-specific\n  /// This configuration only impacts macOS.\n  pub fn with_accept_first_mouse(mut self, accept_first_mouse: bool) -> Self"}

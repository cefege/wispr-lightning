//! Sleep notification and launch-at-login.
//!
//! `NSWorkspace.willSleepNotification` becomes `WM_POWERBROADCAST` /
//! `PBT_APMSUSPEND`, which needs a window to receive it. It has to be a
//! genuine top-level window: broadcast messages are never delivered to
//! message-only (`HWND_MESSAGE`) windows, which is the trap this would
//! otherwise fall into. Without `WS_VISIBLE` it has no taskbar entry, no
//! Alt-Tab presence and no pixels.
//!
//! Handlers run inline on the window thread rather than being posted
//! elsewhere. Windows allows roughly two seconds after `PBT_APMSUSPEND` before
//! it stops caring, and the entire point is to abandon the in-flight recording
//! *before* the machine goes down; a handoff would race the suspend.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::LazyLock;

use parking_lot::Mutex;
use windows::core::w;
use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, WPARAM};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, DispatchMessageW, GetMessageW, RegisterClassW,
    TranslateMessage, MSG, WINDOW_EX_STYLE, WM_POWERBROADCAST, WNDCLASSW, WS_OVERLAPPED,
};

use crate::{Lifecycle, PlatformError, Result};

/// `PBT_APMSUSPEND` from winuser.h. The `PBT_*` constants sit behind the
/// `Win32_System_Power` feature, which is not worth enabling for one value.
const PBT_APMSUSPEND: usize = 0x0004;

type SleepHandler = Box<dyn Fn() + Send + Sync>;

static HANDLERS: LazyLock<Mutex<Vec<SleepHandler>>> = LazyLock::new(|| Mutex::new(Vec::new()));
static WINDOW_STARTED: AtomicBool = AtomicBool::new(false);

pub struct WindowsLifecycle;

impl WindowsLifecycle {
    pub fn new() -> Self {
        Self
    }
}

impl Default for WindowsLifecycle {
    fn default() -> Self {
        Self::new()
    }
}

impl Lifecycle for WindowsLifecycle {
    fn on_sleep(&self, handler: Box<dyn Fn() + Send + Sync>) {
        HANDLERS.lock().push(handler);
        ensure_power_window();
    }

    /// Not ours to answer.
    ///
    /// `tauri-plugin-autostart` owns launch-at-login on both targets
    /// (PORT_PLAN §4), and having two writers for one registry value is how
    /// you end up with a setting that silently reverts. Reporting
    /// `Unsupported` keeps the plugin the single source of truth instead of
    /// pretending here.
    fn set_launch_at_login(&self, _enabled: bool) -> Result<()> {
        Err(PlatformError::Unsupported(
            "launch at login is owned by the Tauri autostart plugin",
        ))
    }

    fn launch_at_login(&self) -> bool {
        false
    }
}

fn ensure_power_window() {
    if WINDOW_STARTED.swap(true, Ordering::SeqCst) {
        return;
    }
    if std::thread::Builder::new()
        .name("wl-power".into())
        .spawn(run_power_window)
        .is_err()
    {
        WINDOW_STARTED.store(false, Ordering::SeqCst);
        tracing::error!("could not start the power-notification window; sleep will go unnoticed");
    }
}

fn run_power_window() {
    // SAFETY: standard window creation and message pump. Every handle is owned
    // by this thread, which is the requirement for both.
    unsafe {
        let Ok(instance) = GetModuleHandleW(None) else {
            tracing::error!("GetModuleHandleW failed; sleep notifications unavailable");
            return;
        };
        let class = WNDCLASSW {
            lpfnWndProc: Some(power_wndproc),
            hInstance: instance.into(),
            lpszClassName: w!("WisprLightningPowerWatcher"),
            ..Default::default()
        };
        if RegisterClassW(&class) == 0 {
            tracing::error!("could not register the power-watcher window class");
            return;
        }
        let window = CreateWindowExW(
            WINDOW_EX_STYLE(0),
            w!("WisprLightningPowerWatcher"),
            w!("Wispr Lightning"),
            WS_OVERLAPPED,
            0,
            0,
            0,
            0,
            None,
            None,
            Some(instance.into()),
            None,
        );
        let Ok(window) = window else {
            tracing::error!("could not create the power-watcher window");
            return;
        };
        debug_assert!(!window.0.is_null());

        let mut message = MSG::default();
        while GetMessageW(&mut message, None, 0, 0).as_bool() {
            let _ = TranslateMessage(&message);
            DispatchMessageW(&message);
        }
    }
}

unsafe extern "system" fn power_wndproc(
    window: HWND,
    message: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    if message == WM_POWERBROADCAST && wparam.0 == PBT_APMSUSPEND {
        tracing::info!("system is suspending");
        for handler in HANDLERS.lock().iter() {
            handler();
        }
        // TRUE: the request is granted. Returning anything else from a
        // suspend broadcast is deprecated and ignored anyway.
        return LRESULT(1);
    }
    // SAFETY: forwarding the message we were handed, unmodified.
    unsafe { DefWindowProcW(window, message, wparam, lparam) }
}

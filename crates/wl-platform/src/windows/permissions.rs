//! Permission status and the settings deep links.
//!
//! Only one of the four permissions in the model exists on Windows.
//! Accessibility, Input Monitoring and Screen Recording are macOS TCC
//! services; the nearest Windows concept is UIPI, which is not a permission
//! anyone can grant — an unelevated process simply cannot drive an elevated
//! window — so they report [`PermissionState::NotApplicable`] rather than
//! pretending to be granted.
//!
//! Microphone access is a privacy *setting*, not a prompt. There is no API for
//! an unpackaged Win32 app to raise the consent dialog, so `request` cannot do
//! anything and `open_settings` deep-links to the page instead. The status is
//! read from the consent store rather than guessed: `NonPackaged` is the key
//! that governs desktop apps, and its absence means the default, which is
//! allow.

use std::time::Duration;

use windows::core::w;
use windows::Win32::System::Registry::{RegGetValueW, HKEY_CURRENT_USER, RRF_RT_REG_SZ};

use crate::{Permission, PermissionState, Permissions};

/// Consent store for desktop (non-packaged) applications.
const MICROPHONE_CONSENT: windows::core::PCWSTR = w!(
    r"Software\Microsoft\Windows\CurrentVersion\CapabilityAccessManager\ConsentStore\microphone\NonPackaged"
);

/// The per-user microphone toggle, which gates the NonPackaged one.
const MICROPHONE_CONSENT_ROOT: windows::core::PCWSTR = w!(
    r"Software\Microsoft\Windows\CurrentVersion\CapabilityAccessManager\ConsentStore\microphone"
);

/// Settings page for the microphone privacy toggle.
const MICROPHONE_SETTINGS: &str = "ms-settings:privacy-microphone";

/// Ceiling on the shell handoff. Generous, because the Settings app may be
/// cold-starting behind it; the bound only exists so a wedged shell cannot
/// hold the command thread forever.
const SHELL_OPEN_BUDGET: Duration = Duration::from_secs(5);

pub struct WindowsPermissions;

impl WindowsPermissions {
    pub fn new() -> Self {
        Self
    }
}

impl Default for WindowsPermissions {
    fn default() -> Self {
        Self::new()
    }
}

impl Permissions for WindowsPermissions {
    fn status(&self, permission: Permission) -> PermissionState {
        match permission {
            Permission::Microphone => microphone_state(),
            Permission::Accessibility
            | Permission::InputMonitoring
            | Permission::ScreenRecording => PermissionState::NotApplicable,
        }
    }

    fn request(&self, permission: Permission) {
        // Nothing to do, on purpose: Windows exposes no consent prompt to an
        // unpackaged process. Callers that see `Denied` should offer
        // `open_settings` instead.
        if permission == Permission::Microphone {
            tracing::debug!(
                "microphone access is a global Windows setting; there is no prompt to raise"
            );
        }
    }

    fn open_settings(&self, permission: Permission) {
        if permission != Permission::Microphone {
            return;
        }
        open_uri(MICROPHONE_SETTINGS);
    }
}

fn microphone_state() -> PermissionState {
    // Both keys must allow: the root toggle is "let apps access your
    // microphone", the NonPackaged one is "let desktop apps access your
    // microphone", and either can be off on its own.
    for key in [MICROPHONE_CONSENT_ROOT, MICROPHONE_CONSENT] {
        match consent_value(key).as_deref() {
            Some("Deny") => return PermissionState::Denied,
            _ => continue,
        }
    }
    // Absent means never configured, which Windows treats as allow. Reporting
    // `Granted` here is honest for everything except an exclusive-mode
    // conflict, which surfaces as a capture error rather than a permission.
    PermissionState::Granted
}

fn consent_value(key: windows::core::PCWSTR) -> Option<String> {
    let mut buffer = [0u16; 32];
    let mut size = std::mem::size_of_val(&buffer) as u32;
    // SAFETY: `buffer`/`size` describe the same allocation, and `RRF_RT_REG_SZ`
    // makes the call reject any value that is not a string.
    let status = unsafe {
        RegGetValueW(
            HKEY_CURRENT_USER,
            key,
            w!("Value"),
            RRF_RT_REG_SZ,
            None,
            Some(buffer.as_mut_ptr().cast()),
            Some(&mut size),
        )
    };
    if status.is_err() {
        return None;
    }
    let chars = (size as usize / 2).min(buffer.len());
    Some(
        String::from_utf16_lossy(&buffer[..chars])
            .trim_end_matches('\0')
            .to_owned(),
    )
}

/// Hand a URI to the shell, from an apartment the shell will talk to.
///
/// `ShellExecuteW` rather than spawning `cmd /c start`: the latter flashes a
/// console window, which on a menu-bar app looks like a crash.
///
/// It goes through [`super::on_sta`] because `open_settings` is reached from an
/// async Tauri command — a tokio worker, which is in this process's implicit
/// MTA — and the shell cannot marshal its objects into one. That failure is
/// silent apart from the return value, and it would take away the only route a
/// user has to a microphone toggle they have already been told is off.
fn open_uri(uri: &str) {
    use windows::Win32::UI::Shell::ShellExecuteW;
    use windows::Win32::UI::WindowsAndMessaging::SW_SHOWNORMAL;

    let wide: Vec<u16> = uri.encode_utf16().chain(std::iter::once(0)).collect();
    let opened = super::on_sta("shell-open", SHELL_OPEN_BUDGET, move || {
        // SAFETY: `wide` is NUL-terminated and outlives the call; a null hwnd
        // is valid for a shell verb with no owner window.
        let result = unsafe {
            ShellExecuteW(
                None,
                w!("open"),
                windows::core::PCWSTR(wide.as_ptr()),
                None,
                None,
                SW_SHOWNORMAL,
            )
        };
        // ShellExecuteW reports failure as a value <= 32 in the returned
        // pseudo-handle.
        result.0 as isize > 32
    });
    if opened != Some(true) {
        tracing::warn!(uri, "the shell refused to open the settings page");
    }
}

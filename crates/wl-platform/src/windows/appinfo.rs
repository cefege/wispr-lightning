//! Identity of the frontmost application.
//!
//! `NSWorkspace.frontmostApplication` has no direct analogue, so this walks
//! `GetForegroundWindow` → `GetWindowThreadProcessId` → `OpenProcess` →
//! `QueryFullProcessImageNameW`. Two details matter:
//!
//! * `PROCESS_QUERY_LIMITED_INFORMATION` rather than the full-information
//!   right: it is the one that still succeeds against a higher-integrity
//!   process, which is most of what a user has open.
//! * Windows has no bundle identifier, so `bundle_id` carries the lowercased
//!   executable basename. The transcription backend keys personalisation off
//!   that field, so it has to be stable and it has to be *something*.
//!
//! The display name comes from the executable's version resource
//! (`FileDescription`), which is what Explorer shows and the nearest thing to
//! `localizedName`. It is cached per path because it is a disk read.

use std::collections::HashMap;

use parking_lot::RwLock;
use uiautomation::controls::ControlType;
use uiautomation::patterns::UIValuePattern;
use uiautomation::types::Handle;
use windows::core::PWSTR;
use windows::Win32::Foundation::{CloseHandle, HANDLE, HWND, MAX_PATH};
use windows::Win32::Storage::FileSystem::{
    GetFileVersionInfoSizeW, GetFileVersionInfoW, VerQueryValueW,
};
use windows::Win32::System::Threading::{
    OpenProcess, QueryFullProcessImageNameW, PROCESS_NAME_WIN32, PROCESS_QUERY_LIMITED_INFORMATION,
};
use windows::Win32::UI::WindowsAndMessaging::{GetForegroundWindow, GetWindowThreadProcessId};

use super::classify::{classify, exe_basename, is_browser, normalize_browser_url};
use super::uia;
use crate::{AppInfo, ForegroundApp, ACCESSIBILITY_TIMEOUT};

/// Localised names Chromium and Firefox give their address bar. A fast path
/// only: the strings move between versions and locales, so failing to match
/// one falls through to a structural search.
const ADDRESS_BAR_NAMES: &[&str] = &[
    "Address and search bar",
    "Address bar",
    "Search or enter web address",
    "Search with Google or enter address",
];

/// How deep to search a browser window for its address bar. The omnibox sits
/// within a handful of levels; an unbounded walk would visit the whole page.
const ADDRESS_BAR_DEPTH: u32 = 12;

pub struct WindowsForegroundApp {
    /// Executable path to display name, because reading a version resource
    /// touches disk and this runs at the start of every recording.
    names: RwLock<HashMap<String, String>>,
}

/// Cache ceiling. The realistic working set is a handful of applications;
/// the bound only exists so a pathological session cannot grow the map
/// forever, and starting over costs one file read per app.
const NAME_CACHE_LIMIT: usize = 128;

impl WindowsForegroundApp {
    pub fn new() -> Self {
        Self {
            names: RwLock::new(HashMap::new()),
        }
    }

    fn display_name(&self, path: &str) -> String {
        if let Some(name) = self.names.read().get(path) {
            return name.clone();
        }
        let name = file_description(path).unwrap_or_else(|| stem_of(path));
        let mut names = self.names.write();
        if names.len() >= NAME_CACHE_LIMIT {
            names.clear();
        }
        names.insert(path.to_owned(), name.clone());
        name
    }
}

impl Default for WindowsForegroundApp {
    fn default() -> Self {
        Self::new()
    }
}

impl ForegroundApp for WindowsForegroundApp {
    fn current(&self) -> AppInfo {
        // SAFETY: no preconditions. Null when a secure desktop is up or the
        // foreground window belongs to another desktop, which we treat the
        // same way the Swift version treats a nil frontmost application.
        let window = unsafe { GetForegroundWindow() };
        if window.0.is_null() {
            return AppInfo::default();
        }
        let Some(path) = foreground_executable(window) else {
            return AppInfo::default();
        };
        let bundle_id = exe_basename(&path);
        let url = if is_browser(&bundle_id) {
            browser_url(window).unwrap_or_default()
        } else {
            String::new()
        };
        AppInfo {
            name: self.display_name(&path),
            kind: classify(&bundle_id),
            bundle_id,
            url,
        }
    }
}

/// Full image path of the process owning `window`.
fn foreground_executable(window: HWND) -> Option<String> {
    let mut pid = 0u32;
    // SAFETY: `window` is a live foreground window handle; the out-parameter
    // is a valid `u32`.
    unsafe { GetWindowThreadProcessId(window, Some(&mut pid)) };
    if pid == 0 {
        return None;
    }
    // SAFETY: the handle is closed on every path by `Process`.
    let process = unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid) }.ok()?;
    let process = Process(process);

    let mut buffer = [0u16; MAX_PATH as usize];
    let mut length = buffer.len() as u32;
    // SAFETY: `buffer`/`length` describe the same allocation, and the handle
    // carries the query-limited right this call requires.
    unsafe {
        QueryFullProcessImageNameW(
            process.0,
            PROCESS_NAME_WIN32,
            PWSTR(buffer.as_mut_ptr()),
            &mut length,
        )
    }
    .ok()?;
    Some(String::from_utf16_lossy(&buffer[..length as usize]))
}

/// Closes a process handle on drop.
struct Process(HANDLE);

impl Drop for Process {
    fn drop(&mut self) {
        // SAFETY: obtained from `OpenProcess` and not closed elsewhere.
        let _ = unsafe { CloseHandle(self.0) };
    }
}

fn stem_of(path: &str) -> String {
    let base = path.rsplit(['\\', '/']).next().unwrap_or(path);
    base.strip_suffix(".exe")
        .or_else(|| base.strip_suffix(".EXE"))
        .unwrap_or(base)
        .to_owned()
}

/// `FileDescription` from the executable's version resource — "Slack",
/// "Microsoft Outlook", "Visual Studio Code".
fn file_description(path: &str) -> Option<String> {
    let wide: Vec<u16> = path.encode_utf16().chain(std::iter::once(0)).collect();
    let file = PWSTR(wide.as_ptr() as *mut u16);

    // SAFETY: `file` is a NUL-terminated wide string that outlives the call.
    let size = unsafe { GetFileVersionInfoSizeW(file, None) };
    if size == 0 {
        return None;
    }
    let mut block = vec![0u8; size as usize];
    // SAFETY: `block` is `size` bytes, exactly what the call was sized for.
    unsafe { GetFileVersionInfoW(file, None, size, block.as_mut_ptr().cast()) }.ok()?;

    // The resource is keyed by language and codepage, and there is no fixed
    // value to guess: read the translation table and use its first entry.
    let (language, codepage) = {
        let key: Vec<u16> = "\\VarFileInfo\\Translation"
            .encode_utf16()
            .chain(std::iter::once(0))
            .collect();
        let mut value = std::ptr::null_mut();
        let mut len = 0u32;
        // SAFETY: `block` is a valid version-info block; the out-parameters
        // borrow from it and stay valid while `block` lives.
        if !unsafe {
            VerQueryValueW(
                block.as_ptr().cast(),
                PWSTR(key.as_ptr() as *mut u16),
                &mut value,
                &mut len,
            )
        }
        .as_bool()
            || len < 4
        {
            return None;
        }
        // SAFETY: the translation table is pairs of `u16` and `len >= 4`.
        let pair = unsafe { std::slice::from_raw_parts(value as *const u16, 2) };
        (pair[0], pair[1])
    };

    let key: Vec<u16> = format!("\\StringFileInfo\\{language:04x}{codepage:04x}\\FileDescription")
        .encode_utf16()
        .chain(std::iter::once(0))
        .collect();
    let mut value = std::ptr::null_mut();
    let mut len = 0u32;
    // SAFETY: as above.
    if !unsafe {
        VerQueryValueW(
            block.as_ptr().cast(),
            PWSTR(key.as_ptr() as *mut u16),
            &mut value,
            &mut len,
        )
    }
    .as_bool()
        || len == 0
    {
        return None;
    }
    // SAFETY: `len` is the length in characters, NUL included.
    let text = unsafe { std::slice::from_raw_parts(value as *const u16, len as usize) };
    let description = String::from_utf16_lossy(text)
        .trim_end_matches('\0')
        .trim()
        .to_owned();
    (!description.is_empty()).then_some(description)
}

/// URL of the focused tab in a browser window.
///
/// The document element is tried first because its `ValuePattern` carries the
/// *committed* URL: the omnibox shows whatever the user is currently typing,
/// which would report a half-written search as the page they are dictating
/// into. The localised address-bar names are a cheap second attempt, and a
/// structural search over edit controls is the fallback for the locales and
/// versions those names do not cover.
fn browser_url(window: HWND) -> Option<String> {
    // The window handle crosses to the UI Automation thread as a plain
    // integer: `uiautomation::types::Handle` wraps a raw pointer and so is
    // not `Send`, even though the value it carries is just a window id.
    let handle = window.0 as isize;
    uia::with_uia("browser-url", ACCESSIBILITY_TIMEOUT, move |automation| {
        let root = automation.element_from_handle(Handle::from(handle)).ok()?;
        let budget = ACCESSIBILITY_TIMEOUT.as_millis() as u64;

        let document = automation
            .create_matcher()
            .from_ref(&root)
            .control_type(ControlType::Document)
            .depth(ADDRESS_BAR_DEPTH)
            .timeout(budget)
            .find_first()
            .ok();
        if let Some(url) = document
            .as_ref()
            .and_then(value_of)
            .and_then(|raw| normalize_browser_url(&raw))
        {
            return Some(url);
        }

        for name in ADDRESS_BAR_NAMES {
            let bar = automation
                .create_matcher()
                .from_ref(&root)
                .control_type(ControlType::Edit)
                .name(*name)
                .depth(ADDRESS_BAR_DEPTH)
                .timeout(budget)
                .find_first()
                .ok();
            if let Some(url) = bar
                .as_ref()
                .and_then(value_of)
                .and_then(|raw| normalize_browser_url(&raw))
            {
                return Some(url);
            }
        }

        automation
            .create_matcher()
            .from_ref(&root)
            .control_type(ControlType::Edit)
            .depth(ADDRESS_BAR_DEPTH)
            .timeout(budget)
            .find_all()
            .ok()?
            .iter()
            .filter_map(value_of)
            .find_map(|raw| normalize_browser_url(&raw))
    })
}

fn value_of(element: &uiautomation::UIElement) -> Option<String> {
    element
        .get_pattern::<UIValuePattern>()
        .ok()?
        .get_value()
        .ok()
        .filter(|value| !value.is_empty())
}

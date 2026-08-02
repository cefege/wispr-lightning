//! Runs the Windows backend's pure logic on a non-Windows host.
//!
//! `wl_platform::windows` is behind `#[cfg(target_os = "windows")]`, so on a
//! macOS development machine its unit tests would never be compiled, let alone
//! run — and the parts worth testing (the executable classification table, the
//! UTF-16 expansion `KEYEVENTF_UNICODE` needs, the typing timing envelope, and
//! the whole hotkey matching state machine) contain no Win32 at all.
//!
//! Those modules are therefore kept free of `windows` imports and included
//! here directly. On Windows they compile and run as ordinary unit tests
//! inside the library, which is why this shim excludes itself there.

#![cfg(not(target_os = "windows"))]

// The included modules refer to their siblings and to the crate root as
// `crate::…`. Inside this test binary the crate root is this file, so the
// items they reach for are re-exported here.
pub(crate) use wl_platform::chord;
pub(crate) use wl_platform::hotkey;
pub(crate) use wl_platform::AppKind;

#[path = "../src/windows/classify.rs"]
mod classify;

#[path = "../src/windows/keystrokes.rs"]
mod keystrokes;

#[path = "../src/windows/matching.rs"]
mod matching;

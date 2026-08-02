//! Clipboard snapshot and restore around text injection.
//!
//! `arboard` only understands text, HTML, images and file lists, so a naive
//! save/overwrite/restore cycle through it destroys RTF, Office/OLE payloads
//! and every app-private format. This module goes to the raw API instead:
//! `EnumClipboardFormats` for everything on the board, `HGLOBAL` copies both
//! ways.
//!
//! Two limits are inherent rather than laziness, and both are documented in
//! `docs/parity/platform-spec.md` §10.7:
//!
//! * Handle-typed formats (`CF_BITMAP`, metafiles, `CF_OWNERDISPLAY`, the
//!   GDI-object range) are not memory blocks and cannot be duplicated this
//!   way. They are counted and reported, not silently dropped.
//! * macOS pasteboards hold N items × M types; Windows holds one item with M
//!   formats, so the nested Swift snapshot flattens to a single list.
//!
//! Another process holding the clipboard open is normal and transient — every
//! Office app does it — so `OpenClipboard` is retried with a short backoff
//! rather than treated as a failure.

use std::sync::Arc;
use std::time::Duration;

use parking_lot::Mutex;
use windows::Win32::Foundation::{GlobalFree, HANDLE, HGLOBAL};
use windows::Win32::System::DataExchange::{
    CloseClipboard, EmptyClipboard, EnumClipboardFormats, GetClipboardData,
    GetClipboardSequenceNumber, OpenClipboard, SetClipboardData,
};
use windows::Win32::System::Memory::{
    GlobalAlloc, GlobalLock, GlobalSize, GlobalUnlock, GMEM_MOVEABLE,
};

use crate::{PlatformError, Result};

/// `CF_UNICODETEXT`. Declared here rather than pulled in from
/// `Win32_System_Ole`: one frozen constant is not worth another metadata
/// feature, and the clipboard entry points take a plain `u32`.
const CF_UNICODETEXT: u32 = 13;

/// Attempts and pause between `OpenClipboard` retries. Roughly 90 ms of
/// patience in total, which comfortably outlasts another app's read.
const OPEN_ATTEMPTS: u32 = 6;
const OPEN_BACKOFF: Duration = Duration::from_millis(15);

/// Refuse to duplicate anything larger than this. A snapshot costs twice the
/// clipboard's size in RAM, and a 64-megapixel screenshot on the board is not
/// worth stalling a dictation over.
const MAX_FORMAT_BYTES: usize = 32 * 1024 * 1024;
const MAX_TOTAL_BYTES: usize = 64 * 1024 * 1024;

/// A flattened snapshot of the clipboard.
///
/// Stored behind `ClipboardSnapshot`'s `Any`, so it has to be `Send + Sync`;
/// keeping it as plain bytes rather than OS handles is what makes that true,
/// and also what lets the restore run on a delay from another thread.
#[derive(Debug, Default)]
pub(crate) struct Snapshot {
    formats: Vec<(u32, Vec<u8>)>,
    /// Formats we saw but could not copy.
    skipped: usize,
    /// Sequence number expected at restore time — see [`restore`].
    expect_sequence: Option<u32>,
}

/// RAII wrapper: an open clipboard must always be closed, including on the
/// error paths, or every other process on the desktop blocks.
struct Session;

impl Session {
    fn open() -> Result<Self> {
        let mut last = String::new();
        for attempt in 0..OPEN_ATTEMPTS {
            // SAFETY: `None` opens the clipboard for the current task, which
            // is what we want; it is released by `Drop`.
            match unsafe { OpenClipboard(None) } {
                Ok(()) => return Ok(Self),
                Err(e) => {
                    last = e.to_string();
                    if attempt + 1 < OPEN_ATTEMPTS {
                        std::thread::sleep(OPEN_BACKOFF);
                    }
                }
            }
        }
        Err(PlatformError::Clipboard(format!(
            "another process held the clipboard for {OPEN_ATTEMPTS} attempts: {last}"
        )))
    }
}

impl Drop for Session {
    fn drop(&mut self) {
        // SAFETY: paired with the successful `OpenClipboard` above.
        let _ = unsafe { CloseClipboard() };
    }
}

/// Whether a clipboard format is a memory block we can duplicate.
///
/// The excluded values are GDI or owner-rendered handles; passing one to
/// `GlobalSize` is undefined, and copying the handle itself would hand the
/// next owner a dangling object.
fn is_memory_format(format: u32) -> bool {
    const CF_BITMAP: u32 = 2;
    const CF_METAFILEPICT: u32 = 3;
    const CF_PALETTE: u32 = 9;
    const CF_ENHMETAFILE: u32 = 14;
    const CF_OWNERDISPLAY: u32 = 0x0080;
    const CF_DSPBITMAP: u32 = 0x0082;
    const CF_DSPMETAFILEPICT: u32 = 0x0083;
    const CF_DSPENHMETAFILE: u32 = 0x008E;
    const CF_GDIOBJFIRST: u32 = 0x0300;
    const CF_GDIOBJLAST: u32 = 0x03FF;

    !matches!(
        format,
        CF_BITMAP
            | CF_METAFILEPICT
            | CF_PALETTE
            | CF_ENHMETAFILE
            | CF_OWNERDISPLAY
            | CF_DSPBITMAP
            | CF_DSPMETAFILEPICT
            | CF_DSPENHMETAFILE
    ) && !(CF_GDIOBJFIRST..=CF_GDIOBJLAST).contains(&format)
}

/// Copy the bytes behind an `HGLOBAL`-backed clipboard handle.
fn read_global(handle: HANDLE) -> Option<Vec<u8>> {
    let global = HGLOBAL(handle.0);
    // SAFETY: `handle` came from `GetClipboardData` for a memory-backed
    // format, so it is a valid movable global; the lock is released below.
    unsafe {
        let size = GlobalSize(global);
        if size == 0 || size > MAX_FORMAT_BYTES {
            return None;
        }
        let ptr = GlobalLock(global) as *const u8;
        if ptr.is_null() {
            return None;
        }
        let bytes = std::slice::from_raw_parts(ptr, size).to_vec();
        let _ = GlobalUnlock(global);
        Some(bytes)
    }
}

/// Allocate a movable global containing `bytes`, ready to hand to the
/// clipboard. Ownership transfers on a successful `SetClipboardData`.
fn write_global(bytes: &[u8]) -> Option<HGLOBAL> {
    // SAFETY: allocation is checked and the lock is released before return.
    unsafe {
        let global = GlobalAlloc(GMEM_MOVEABLE, bytes.len()).ok()?;
        let ptr = GlobalLock(global) as *mut u8;
        if ptr.is_null() {
            let _ = GlobalFree(Some(global));
            return None;
        }
        std::ptr::copy_nonoverlapping(bytes.as_ptr(), ptr, bytes.len());
        let _ = GlobalUnlock(global);
        Some(global)
    }
}

/// Capture every duplicable format currently on the clipboard.
pub(crate) fn snapshot() -> Result<Snapshot> {
    let _session = Session::open()?;
    let mut snapshot = Snapshot::default();
    let mut total = 0usize;
    let mut format = 0u32;

    loop {
        // SAFETY: valid inside an open clipboard session; 0 ends the walk.
        format = unsafe { EnumClipboardFormats(format) };
        if format == 0 {
            break;
        }
        if !is_memory_format(format) {
            snapshot.skipped += 1;
            continue;
        }
        // SAFETY: the returned handle stays owned by the clipboard; we read only.
        let Ok(handle) = (unsafe { GetClipboardData(format) }) else {
            // A delayed-render offer whose owner has gone away.
            snapshot.skipped += 1;
            continue;
        };
        match read_global(handle) {
            Some(bytes) if total + bytes.len() <= MAX_TOTAL_BYTES => {
                total += bytes.len();
                snapshot.formats.push((format, bytes));
            }
            _ => snapshot.skipped += 1,
        }
    }

    if snapshot.skipped > 0 {
        tracing::debug!(
            kept = snapshot.formats.len(),
            skipped = snapshot.skipped,
            "clipboard formats that cannot be duplicated will not survive the paste"
        );
    }
    Ok(snapshot)
}

/// Replace the clipboard with `text`, recording the sequence number so the
/// restore can tell whether the user copied something in the meantime.
pub(crate) fn set_text(text: &str, snapshot: &mut Snapshot) -> Result<()> {
    let mut utf16: Vec<u16> = text.encode_utf16().collect();
    utf16.push(0);
    // SAFETY: reinterpreting `[u16]` as bytes; alignment only ever relaxes.
    let bytes = unsafe { std::slice::from_raw_parts(utf16.as_ptr() as *const u8, utf16.len() * 2) };
    let global = write_global(bytes)
        .ok_or_else(|| PlatformError::Clipboard("could not allocate clipboard memory".into()))?;

    let session = Session::open()?;
    // SAFETY: inside an open session. `EmptyClipboard` makes us the owner,
    // which `SetClipboardData` requires. On failure ownership has not
    // transferred, so the block is still ours to free.
    unsafe {
        if let Err(e) = EmptyClipboard() {
            let _ = GlobalFree(Some(global));
            return Err(PlatformError::Clipboard(e.to_string()));
        }
        if let Err(e) = SetClipboardData(CF_UNICODETEXT, Some(HANDLE(global.0))) {
            let _ = GlobalFree(Some(global));
            return Err(PlatformError::Clipboard(e.to_string()));
        }
    }
    drop(session);

    // SAFETY: no preconditions.
    snapshot.expect_sequence = Some(unsafe { GetClipboardSequenceNumber() });
    Ok(())
}

/// Put the snapshot back.
///
/// Skipped when the clipboard has changed since we wrote to it: that means the
/// user copied something during the paste, and their new content outranks the
/// old one we are holding. An empty snapshot still clears the board, so the
/// transcript never lingers.
pub(crate) fn restore(snapshot: Snapshot) -> Result<()> {
    if let Some(expected) = snapshot.expect_sequence {
        // SAFETY: no preconditions.
        let current = unsafe { GetClipboardSequenceNumber() };
        if current != expected {
            tracing::debug!(
                expected,
                current,
                "clipboard changed during injection; leaving the newer content alone"
            );
            return Ok(());
        }
    }

    let _session = Session::open()?;
    // SAFETY: inside an open session; makes us the owner so we may set data.
    unsafe { EmptyClipboard() }.map_err(|e| PlatformError::Clipboard(e.to_string()))?;

    let count = snapshot.formats.len();
    for (format, bytes) in snapshot.formats {
        let Some(global) = write_global(&bytes) else {
            continue;
        };
        // SAFETY: inside an open session that we own. On success the system
        // takes the block; on failure it is still ours and must be freed.
        if unsafe { SetClipboardData(format, Some(HANDLE(global.0))) }.is_err() {
            let _ = unsafe { GlobalFree(Some(global)) };
        }
    }
    if count > 0 {
        tracing::debug!(count, "clipboard restored");
    }
    Ok(())
}

/// Restore after `delay`, off the caller's thread.
///
/// The delay gives the target application time to consume the paste before the
/// board changes under it. Unlike the Swift original this runs on every path,
/// including the failure paths — leaking a transcript into the user's
/// clipboard because event synthesis failed is deviation DV4. If the thread
/// cannot even be spawned the restore happens inline rather than not at all.
pub(crate) fn schedule_restore(snapshot: Snapshot, delay: Duration) {
    let pending = Arc::new(Mutex::new(Some(snapshot)));
    let deferred = Arc::clone(&pending);
    let spawned = std::thread::Builder::new()
        .name("wl-clipboard-restore".into())
        .spawn(move || {
            std::thread::sleep(delay);
            take_and_restore(&deferred);
        });
    if spawned.is_err() {
        tracing::error!("could not spawn clipboard restore; restoring inline");
        take_and_restore(&pending);
    }
}

fn take_and_restore(pending: &Mutex<Option<Snapshot>>) {
    let Some(snapshot) = pending.lock().take() else {
        return;
    };
    if let Err(e) = restore(snapshot) {
        tracing::warn!(error = %e, "clipboard restore failed");
    }
}

//! On-device OCR of the frontmost window, used as opportunistic transcription
//! context.
//!
//! Deliberately narrow: one window, the fast recognition level, no language
//! correction. This runs *concurrently with recording*, so it must never cost
//! more than the recording itself, and its output is a hint the backend may
//! ignore.

use std::thread;
use std::time::Duration;

use objc2::rc::Retained;
use objc2::AnyThread;
use objc2_core_foundation::{CFDictionary, CFNumber, CFNumberType, CFRetained, CFString, CGRect};
use objc2_core_graphics::{
    kCGWindowLayer, kCGWindowNumber, kCGWindowOwnerPID, CGImage, CGWindowImageOption,
    CGWindowListCopyWindowInfo, CGWindowListOption,
};
use objc2_foundation::{NSArray, NSDictionary};
use objc2_vision::{
    VNImageRequestHandler, VNRecognizeTextRequest, VNRecognizedTextObservation, VNRequest,
    VNRequestTextRecognitionLevel,
};
use tracing::debug;

use crate::ScreenText;

/// Ceiling for the whole capture-and-recognize pass.
///
/// The Swift original had none, and Vision on a 6K display can take seconds.
/// Blowing the deadline yields no context, which is a fine outcome: the
/// transcription request simply goes out without it.
const OCR_TIMEOUT: Duration = Duration::from_secs(3);

/// `CGRectNull`, which tells `CGWindowListCreateImage` to use the window's own
/// bounds. CoreGraphics defines it as an infinite origin with zero size, and
/// objc2 does not re-export the constant.
const CG_RECT_NULL: CGRect = CGRect {
    origin: objc2_core_foundation::CGPoint {
        x: f64::INFINITY,
        y: f64::INFINITY,
    },
    size: objc2_core_foundation::CGSize {
        width: 0.0,
        height: 0.0,
    },
};

pub struct MacScreenText;

impl ScreenText for MacScreenText {
    fn ocr_frontmost_window(&self, max_lines: usize) -> Vec<String> {
        if max_lines == 0 {
            return Vec::new();
        }
        // Everything below touches CoreGraphics and Vision objects, none of
        // which are `Send`, so the work happens wholly inside the worker and
        // only the finished lines cross the channel.
        let (tx, rx) = crossbeam_channel::bounded(1);
        let spawned = thread::Builder::new().name("wl-ocr".into()).spawn(move || {
            let _ = tx.send(capture_and_recognize(max_lines));
        });
        if let Err(err) = spawned {
            debug!(%err, "could not start the OCR worker");
            return Vec::new();
        }
        match rx.recv_timeout(OCR_TIMEOUT) {
            Ok(lines) => lines,
            Err(_) => {
                debug!("OCR exceeded its deadline; continuing without screen context");
                Vec::new()
            }
        }
    }
}

fn capture_and_recognize(max_lines: usize) -> Vec<String> {
    let Some(window_id) = frontmost_window_id() else {
        return Vec::new();
    };
    let Some(image) = capture(window_id) else {
        // A nil image is what a missing Screen Recording grant looks like:
        // there is no error, the pixels simply never arrive.
        debug!("window capture returned nothing; Screen Recording is likely not granted");
        return Vec::new();
    };
    recognize(&image, max_lines)
}

/// The frontmost application's first normal-layer window.
///
/// Layer 0 excludes panels, menus and the status bar, so this is the document
/// window the user is actually looking at.
fn frontmost_window_id() -> Option<u32> {
    let front_pid = objc2_app_kit::NSWorkspace::sharedWorkspace()
        .frontmostApplication()?
        .processIdentifier() as i64;

    let windows = CGWindowListCopyWindowInfo(
        CGWindowListOption::OptionOnScreenOnly | CGWindowListOption::ExcludeDesktopElements,
        // `kCGNullWindowID`: no window to be relative to.
        0,
    )?;

    for index in 0..windows.count() {
        // SAFETY: `index` is in range, and `CGWindowListCopyWindowInfo`
        // documents the array elements as CFDictionaries borrowed from the
        // still-live array.
        let entry = unsafe { windows.value_at_index(index) };
        if entry.is_null() {
            continue;
        }
        // SAFETY: as above.
        let entry = unsafe { &*(entry as *const CFDictionary) };
        // SAFETY: the window-info keys are immortal framework constants.
        let (pid_key, layer_key, number_key) =
            unsafe { (kCGWindowOwnerPID, kCGWindowLayer, kCGWindowNumber) };
        if dict_i64(entry, pid_key) != Some(front_pid) || dict_i64(entry, layer_key) != Some(0) {
            continue;
        }
        if let Some(number) = dict_i64(entry, number_key) {
            return u32::try_from(number).ok();
        }
    }
    debug!("no normal-layer window belongs to the frontmost application");
    None
}

/// Read an integer out of a window-info dictionary.
fn dict_i64(dict: &CFDictionary, key: &CFString) -> Option<i64> {
    // SAFETY: the key is a live CFString and the dictionary is live. The
    // returned value follows the Get rule, so it is borrowed, not owned.
    let value = unsafe { dict.value(key as *const CFString as *const std::ffi::c_void) };
    if value.is_null() {
        return None;
    }
    // SAFETY: every key used here is documented as mapping to a CFNumber.
    let number = unsafe { &*(value as *const CFNumber) };
    let mut out: i64 = 0;
    // SAFETY: `out` is a live `i64`, matching the requested `SInt64` type.
    let ok = unsafe {
        number.value(
            CFNumberType::SInt64Type,
            (&mut out as *mut i64).cast::<std::ffi::c_void>(),
        )
    };
    ok.then_some(out)
}

/// Grab the pixels of one window.
///
/// `CGWindowListCreateImage` is deprecated in favour of ScreenCaptureKit,
/// which cannot capture a single window without a picker prompt — a
/// non-starter for something that runs on every dictation.
#[allow(deprecated)]
fn capture(window_id: u32) -> Option<CFRetained<CGImage>> {
    objc2_core_graphics::CGWindowListCreateImage(
        CG_RECT_NULL,
        CGWindowListOption::OptionIncludingWindow,
        window_id,
        // Frame and shadow would add nothing but pixels for Vision to chew on.
        CGWindowImageOption::BoundsIgnoreFraming,
    )
}

fn recognize(image: &CGImage, max_lines: usize) -> Vec<String> {
    let request = VNRecognizeTextRequest::new();
    // Fast is roughly an order of magnitude quicker than Accurate, and this is
    // context rather than content: a few misread characters cost nothing.
    request.setRecognitionLevel(VNRequestTextRecognitionLevel::Fast);
    // Language correction would "fix" identifiers, URLs and code, which are
    // exactly the tokens worth feeding to the transcriber.
    request.setUsesLanguageCorrection(false);

    let options = NSDictionary::new();
    // SAFETY: `image` outlives the handler, and the options dictionary is
    // empty, so its value types are trivially correct.
    let handler = unsafe {
        VNImageRequestHandler::initWithCGImage_options(
            VNImageRequestHandler::alloc(),
            image,
            &options,
        )
    };

    // Two `into_super` hops: VNRecognizeTextRequest -> VNImageBasedRequest ->
    // VNRequest, which is what the handler takes.
    let requests = NSArray::from_retained_slice(&[Retained::into_super(Retained::into_super(
        request.clone(),
    ))]);
    if let Err(err) = handler.performRequests_error(&requests) {
        debug!(%err, "Vision text recognition failed");
        return Vec::new();
    }

    // SAFETY: the request has been performed, so its results are populated and
    // owned by the request we still hold.
    let Some(results) = (unsafe { VNRequest::results(&request) }) else {
        return Vec::new();
    };

    let mut lines = Vec::with_capacity(max_lines.min(results.len()));
    for observation in results.iter() {
        if lines.len() >= max_lines {
            break;
        }
        let Ok(text) = observation.downcast::<VNRecognizedTextObservation>() else {
            continue;
        };
        // One candidate is enough: the alternatives are variations on the same
        // line, and Vision already sorts them by confidence.
        if let Some(best) = text.topCandidates(1).firstObject() {
            lines.push(best.string().to_string());
        }
    }
    lines
}

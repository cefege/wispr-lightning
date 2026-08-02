//! The system accent colour.
//!
//! `NSColor.controlAccentColor` is a *dynamic* colour: it carries no components
//! of its own and resolves against the current appearance when asked, and
//! Apple's header says outright *"Do not make assumptions about the color space
//! of this color, which may change across releases."* Every read here therefore
//! goes through `colorUsingColorSpace:` with the sRGB space before touching a
//! component, because the only consumer is a CSS hex literal.
//!
//! **Threading.** Resolving a dynamic colour reads
//! `NSAppearance.currentDrawingAppearance`, which is per-thread state that only
//! the main thread has set up meaningfully, so the read is gated behind a
//! [`MainThreadMarker`] and reached through [`super::main_thread::run`] like the
//! other two main-thread-only APIs in this module tree.
//!
//! In practice that helper never has to dispatch: the seed read happens during
//! Tauri's `setup`, which runs on the main thread, and
//! `NSSystemColorsDidChangeNotification` is posted on the main thread too. That
//! is the point of caching the value rather than reading it per call — the IPC
//! command runs on a tokio worker, and a command that waited on the main run
//! loop could be waiting on a run loop that is blocked behind its own caller.

use std::sync::Arc;
use std::time::Duration;

use objc2::rc::Retained;
use objc2::runtime::{AnyObject, ProtocolObject};
use objc2::MainThreadMarker;
use objc2_app_kit::{NSColor, NSColorSpace, NSSystemColorsDidChangeNotification};
use objc2_foundation::{NSNotification, NSNotificationCenter, NSObjectProtocol};
use parking_lot::Mutex;
use tracing::{debug, warn};

use crate::{Appearance, Rgb};

/// Ceiling on the one path that could ever hop to the main thread. Generous
/// because missing it is cosmetic: the CSS fallback stands and the next
/// notification corrects it.
const READ_BUDGET: Duration = Duration::from_millis(500);

/// An opaque `NSNotificationCenter` observer token.
///
/// The token is an object we never message ourselves — it only travels back
/// into `removeObserver:` — and `NSNotificationCenter` is documented as
/// thread-safe, so holding and releasing it from any thread is sound. Wrapping
/// it is what lets [`MacAppearance`] be `Sync`, which the trait requires.
struct ObserverToken(Retained<ProtocolObject<dyn NSObjectProtocol>>);

// SAFETY: see the type's documentation.
unsafe impl Send for ObserverToken {}
// SAFETY: see the type's documentation.
unsafe impl Sync for ObserverToken {}

impl Drop for ObserverToken {
    fn drop(&mut self) {
        let center = NSNotificationCenter::defaultCenter();
        let observer: &AnyObject = self.0.as_ref();
        // SAFETY: the token came from this same notification center's
        // `addObserverForName:` and has not been removed before.
        unsafe { center.removeObserver(observer) };
    }
}

/// A subscriber to accent changes. Named because the boxed-closure vector is
/// otherwise past clippy's complexity threshold.
type AccentHandler = Box<dyn Fn(Rgb) + Send + Sync>;

/// Shared between the accessor and the notification block.
#[derive(Default)]
struct Inner {
    current: Mutex<Option<Rgb>>,
    handlers: Mutex<Vec<AccentHandler>>,
}

impl Inner {
    /// Adopt a freshly read accent, notifying subscribers only if it moved.
    ///
    /// The guard matters: `NSSystemColorsDidChangeNotification` covers every
    /// system colour, so switching appearance or highlight colour posts it too,
    /// and republishing an unchanged accent would push a pointless style write
    /// into every open window.
    fn adopt(&self, accent: Rgb) {
        {
            let mut current = self.current.lock();
            if *current == Some(accent) {
                return;
            }
            *current = Some(accent);
        }
        debug!(accent = %accent.to_hex(), "the system accent colour changed");
        for handler in self.handlers.lock().iter() {
            handler(accent);
        }
    }
}

pub struct MacAppearance {
    inner: Arc<Inner>,
    /// Unregisters on drop; never read.
    _observer: ObserverToken,
}

impl MacAppearance {
    pub fn new() -> Self {
        let inner = Arc::new(Inner::default());
        *inner.current.lock() = read();
        let observer = observe(Arc::clone(&inner));
        Self {
            inner,
            _observer: observer,
        }
    }
}

impl Default for MacAppearance {
    fn default() -> Self {
        Self::new()
    }
}

impl Appearance for MacAppearance {
    fn accent(&self) -> Option<Rgb> {
        *self.inner.current.lock()
    }

    fn on_accent_change(&self, handler: Box<dyn Fn(Rgb) + Send + Sync>) {
        self.inner.handlers.lock().push(handler);
    }
}

/// Subscribe to `NSSystemColorsDidChangeNotification` for the life of the token.
fn observe(inner: Arc<Inner>) -> ObserverToken {
    let block = block2::RcBlock::new(move |_notification: std::ptr::NonNull<NSNotification>| {
        if let Some(accent) = read() {
            inner.adopt(accent);
        }
    });

    // The default center, not `NSWorkspace`'s: this is an AppKit notification
    // about the running application's colour environment rather than a
    // workspace-wide event.
    let center = NSNotificationCenter::defaultCenter();
    // SAFETY: the name is an immortal framework constant, and `None` for the
    // queue delivers the block synchronously on the posting thread — which for
    // this notification is the main thread, so the read inside needs no hop.
    let token = unsafe {
        center.addObserverForName_object_queue_usingBlock(
            Some(NSSystemColorsDidChangeNotification),
            None,
            None,
            &block,
        )
    };
    ObserverToken(token)
}

/// Resolve the accent to sRGB, from whichever thread the caller is on.
fn read() -> Option<Rgb> {
    super::main_thread::run(resolve, READ_BUDGET).flatten()
}

fn resolve(_mtm: MainThreadMarker) -> Option<Rgb> {
    let accent = NSColor::controlAccentColor();
    let Some(srgb) = accent.colorUsingColorSpace(&NSColorSpace::sRGBColorSpace()) else {
        warn!("the system accent colour would not convert to sRGB");
        return None;
    };
    Some(Rgb::new(
        channel(srgb.redComponent()),
        channel(srgb.greenComponent()),
        channel(srgb.blueComponent()),
    ))
}

/// One 0.0–1.0 component to 8 bits.
///
/// Clamped rather than trusted: converting from a wider space can land a
/// component slightly outside the sRGB gamut, and an out-of-range cast to `u8`
/// saturates in a way that would silently distort the hue.
fn channel(value: impl Into<f64>) -> u8 {
    (value.into().clamp(0.0, 1.0) * 255.0).round() as u8
}

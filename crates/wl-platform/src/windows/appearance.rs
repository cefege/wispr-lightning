//! The system accent colour.
//!
//! `UISettings::GetColorValue(UIColorType::Accent)` is the source, which is
//! what Chromium itself reads. The registry is deliberately not consulted:
//! `HKCU\Software\Microsoft\Windows\DWM\ColorizationColor` is the accent
//! *border* colour, already blended toward `#d9d9d9`, so it is simply a
//! different colour and looks subtly wrong rather than obviously broken. When
//! `UISettings` cannot be activated at all this reports nothing and the CSS
//! fallback stands, which is a better failure than a plausible wrong answer.
//!
//! **Apartment.** Registration joins whatever apartment the process already
//! has, exactly as in [`super::devices`]: [`super::ensure_mta`] gives every
//! COM-uninitialised thread an implicit MTA, and Tauri owns the main thread as
//! an STA for WebView2. `UISettings` is agile, so `ColorValuesChanged` arrives
//! on a system thread either way and never on the UI thread.
//!
//! **Threading.** The callback takes one lock, writes a cached colour and calls
//! the subscribers. It must not re-enter the `UISettings` object:
//! `RemoveColorValuesChanged` blocks until in-flight callbacks return, so a
//! callback that waited on the registration would deadlock its own teardown.
//! Reading the colour off the `sender` argument rather than a captured clone is
//! also what keeps the delegate from owning the object that owns it.

use std::sync::Arc;

use parking_lot::Mutex;
use tracing::{debug, warn};
use windows::Foundation::TypedEventHandler;
use windows::UI::ViewManagement::{UIColorType, UISettings};
use windows_core::IInspectable;

use crate::{Appearance, Rgb};

/// Shared between the accessor and the change callback.
#[derive(Default)]
struct Inner {
    current: Mutex<Option<Rgb>>,
    handlers: Mutex<Vec<Box<dyn Fn(Rgb) + Send + Sync>>>,
}

impl Inner {
    /// Adopt a freshly read accent, notifying subscribers only if it moved.
    ///
    /// The guard matters: `ColorValuesChanged` fires for the light/dark switch
    /// and for high contrast as well as for the accent, and republishing an
    /// unchanged colour would push a pointless style write into every window.
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

pub struct WindowsAppearance {
    inner: Arc<Inner>,
    /// Held for the life of the registration: dropping the `UISettings` the
    /// event was registered on stops the event.
    registration: Option<(UISettings, i64)>,
}

impl WindowsAppearance {
    pub fn new() -> Self {
        super::ensure_mta();

        let inner = Arc::new(Inner::default());
        let settings = match UISettings::new() {
            Ok(settings) => settings,
            Err(e) => {
                warn!(error = %e, "no UISettings; the accent colour will fall back");
                return Self {
                    inner,
                    registration: None,
                };
            }
        };

        *inner.current.lock() = accent_of(&settings);

        let registration = {
            let inner = Arc::clone(&inner);
            let handler = TypedEventHandler::<UISettings, IInspectable>::new(
                move |sender, _args: windows_core::Ref<'_, IInspectable>| {
                    if let Some(accent) = sender.as_ref().and_then(accent_of) {
                        inner.adopt(accent);
                    }
                    Ok(())
                },
            );
            match settings.ColorValuesChanged(&handler) {
                Ok(token) => Some((settings, token)),
                Err(e) => {
                    warn!(error = %e, "could not observe accent colour changes");
                    None
                }
            }
        };

        Self {
            inner,
            registration,
        }
    }
}

impl Default for WindowsAppearance {
    fn default() -> Self {
        Self::new()
    }
}

impl Drop for WindowsAppearance {
    fn drop(&mut self) {
        let Some((settings, token)) = self.registration.take() else {
            return;
        };
        // Drop can run on any thread, including one that never initialised COM.
        super::ensure_mta();
        if let Err(e) = settings.RemoveColorValuesChanged(token) {
            debug!(error = %e, "could not unregister the accent colour observer");
        }
    }
}

impl Appearance for WindowsAppearance {
    fn accent(&self) -> Option<Rgb> {
        *self.inner.current.lock()
    }

    fn on_accent_change(&self, handler: Box<dyn Fn(Rgb) + Send + Sync>) {
        self.inner.handlers.lock().push(handler);
    }
}

/// The accent as sRGB. `UIColorType::Accent` is already sRGB and opaque; the
/// alpha channel it comes with is discarded rather than blended.
fn accent_of(settings: &UISettings) -> Option<Rgb> {
    match settings.GetColorValue(UIColorType::Accent) {
        Ok(color) => Some(Rgb::new(color.R, color.G, color.B)),
        Err(e) => {
            warn!(error = %e, "could not read the system accent colour");
            None
        }
    }
}

//! Process lifecycle hooks.

use objc2::rc::Retained;
use objc2::runtime::{AnyObject, ProtocolObject};
use objc2_app_kit::{NSWorkspace, NSWorkspaceWillSleepNotification};
use objc2_foundation::{NSNotification, NSNotificationCenter, NSObjectProtocol};
use parking_lot::Mutex;
use tracing::info;

use crate::{Lifecycle, PlatformError, Result};

/// An opaque `NSNotificationCenter` observer token.
///
/// The token is an object we never message ourselves — it only ever travels
/// back into `removeObserver:` — and `NSNotificationCenter` is documented as
/// thread-safe, so holding and releasing it from any thread is sound. Wrapping
/// it is what lets [`MacLifecycle`] be `Sync`, which the trait requires.
struct ObserverToken(Retained<ProtocolObject<dyn NSObjectProtocol>>);

// SAFETY: see the type's documentation.
unsafe impl Send for ObserverToken {}
// SAFETY: see the type's documentation.
unsafe impl Sync for ObserverToken {}

impl Drop for ObserverToken {
    fn drop(&mut self) {
        let center = NSWorkspace::sharedWorkspace().notificationCenter();
        let observer: &AnyObject = self.0.as_ref();
        // SAFETY: the token came from this same notification center's
        // `addObserverForName:` and has not been removed before.
        unsafe { center.removeObserver(observer) };
    }
}

#[derive(Default)]
pub struct MacLifecycle {
    observers: Mutex<Vec<ObserverToken>>,
}

impl MacLifecycle {
    pub fn new() -> Self {
        Self::default()
    }
}

impl Lifecycle for MacLifecycle {
    fn on_sleep(&self, handler: Box<dyn Fn() + Send + Sync>) {
        let block =
            block2::RcBlock::new(move |_notification: std::ptr::NonNull<NSNotification>| {
                info!("the machine is going to sleep");
                handler();
            });
        // `NSWorkspace`'s own notification center, not the default one: sleep
        // and wake are workspace notifications and never appear on
        // `NSNotificationCenter::defaultCenter`.
        let center: Retained<NSNotificationCenter> =
            NSWorkspace::sharedWorkspace().notificationCenter();
        // SAFETY: the name is an immortal framework constant; passing `None`
        // for the queue delivers the block synchronously on the posting
        // thread, which is what we want — the handler must run before the
        // machine actually suspends.
        let token = unsafe {
            center.addObserverForName_object_queue_usingBlock(
                Some(NSWorkspaceWillSleepNotification),
                None,
                None,
                &block,
            )
        };
        self.observers.lock().push(ObserverToken(token));
    }

    fn set_launch_at_login(&self, _enabled: bool) -> Result<()> {
        // Owned by `tauri-plugin-autostart` at the app layer: on macOS it
        // registers a `SMAppService` login item against the bundle, which only
        // exists once the app is packaged. Duplicating it here would fight the
        // plugin for the same registration.
        Err(PlatformError::Unsupported(
            "launch at login is managed by the autostart plugin",
        ))
    }

    fn launch_at_login(&self) -> bool {
        false
    }
}

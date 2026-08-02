//! System-wide CoreAudio device-change notifications.
//!
//! Two `AudioObjectAddPropertyListener` registrations on
//! `kAudioObjectSystemObject`, which is what `AudioRecorder.swift` does with
//! `AudioObjectAddPropertyListenerBlock`:
//!
//! * `kAudioHardwarePropertyDevices` — the device list changed.
//! * `kAudioHardwarePropertyDefaultInputDevice` — the machine default input
//!   moved.
//!
//! These are *not* the listeners cpal already installs. cpal registers
//! `kAudioDevicePropertyDeviceIsAlive` and
//! `kAudioDevicePropertyNominalSampleRate` on one open device, and they answer
//! "is my stream still good". These are registered on the HAL itself and
//! answer "what microphones exist, and which one is the machine's". Nothing
//! else in the process asks the second question, so without this module the
//! settings picker never live-updates and a hot-plug never re-arms the
//! microphone.
//!
//! **Threading.** Registration and removal happen on one dedicated thread,
//! the discipline cpal follows for the same API. Notifications do *not*
//! arrive on that thread: with no `kAudioHardwarePropertyRunLoop` set the HAL
//! delivers them on a thread of its own, so this one merely parks until the
//! watcher is dropped. The callback therefore runs concurrently with
//! everything else and must not block.

use std::ffi::c_void;
use std::panic::{catch_unwind, AssertUnwindSafe};
use std::ptr::NonNull;
use std::sync::Arc;
use std::thread::JoinHandle;

use crossbeam_channel::{bounded, Sender};
use objc2_core_audio::{
    kAudioHardwarePropertyDefaultInputDevice, kAudioHardwarePropertyDevices,
    kAudioObjectPropertyElementMain, kAudioObjectPropertyScopeGlobal, kAudioObjectSystemObject,
    AudioObjectAddPropertyListener, AudioObjectID, AudioObjectPropertyAddress,
    AudioObjectPropertySelector, AudioObjectRemovePropertyListener,
};

use crate::audio_impl::DeviceChange;
use crate::{PlatformError, Result};

/// The two properties the Swift original registers for, in its order.
const CHANGES: [DeviceChange; 2] = [DeviceChange::List, DeviceChange::DefaultInput];

/// A live pair of system-wide property listeners. Dropping it deregisters
/// both, on the thread that registered them.
pub(crate) struct DeviceWatcher {
    /// Dropping this wakes the listener thread, which is the only way to ask
    /// it to deregister.
    shutdown: Option<Sender<()>>,
    thread: Option<JoinHandle<()>>,
}

/// Observe machine-wide audio device changes until the returned watcher is
/// dropped.
///
/// `on_change` runs on a CoreAudio HAL notification thread. It may run
/// concurrently with itself and with anything else in the process, and it must
/// never block or call back into the audio stack — a HAL callback that waits
/// on the HAL deadlocks the whole audio system.
pub(crate) fn watch<F>(on_change: F) -> Result<DeviceWatcher>
where
    F: Fn(DeviceChange) + Send + Sync + 'static,
{
    // Rendezvous channel: the listener thread's `recv` fails the moment the
    // watcher drops its `Sender`, and that is the shutdown signal.
    let (shutdown, stop) = bounded::<()>(0);
    let (ready_tx, ready_rx) = bounded(1);

    let thread = std::thread::Builder::new()
        .name("wl-audio-devices".into())
        .spawn(move || {
            let on_change = Arc::new(on_change);
            let registered: Result<Vec<Listener>> = CHANGES
                .iter()
                .map(|&what| {
                    let on_change = Arc::clone(&on_change);
                    Listener::add(what, move || on_change(what))
                })
                .collect();
            match registered {
                Ok(listeners) => {
                    let _ = ready_tx.send(Ok(()));
                    // Park. Notifications land on a HAL thread, not here.
                    let _ = stop.recv();
                    drop(listeners);
                }
                Err(e) => {
                    let _ = ready_tx.send(Err(e));
                }
            }
        })
        .map_err(|e| PlatformError::AudioDevice(format!("cannot start device watcher: {e}")))?;

    match ready_rx.recv() {
        Ok(Ok(())) => Ok(DeviceWatcher {
            shutdown: Some(shutdown),
            thread: Some(thread),
        }),
        Ok(Err(e)) => Err(e),
        Err(_) => Err(PlatformError::AudioDevice(
            "device watcher thread died before registering".into(),
        )),
    }
}

impl Drop for DeviceWatcher {
    fn drop(&mut self) {
        // Drop the sender first so the thread's `recv` returns, then join:
        // deregistration can only happen on that thread, and it must be done
        // before we claim to have stopped watching.
        self.shutdown = None;
        if let Some(thread) = self.thread.take() {
            if thread.join().is_err() {
                tracing::error!("audio device watcher thread panicked");
            }
        }
    }
}

/// The HAL property selector behind each change.
///
/// Crate-visible so a test can pin the two four-character codes without
/// reaching into CoreAudio itself.
pub(crate) fn selector(what: DeviceChange) -> AudioObjectPropertySelector {
    match what {
        DeviceChange::List => kAudioHardwarePropertyDevices,
        DeviceChange::DefaultInput => kAudioHardwarePropertyDefaultInputDevice,
    }
}

fn address(what: DeviceChange) -> AudioObjectPropertyAddress {
    AudioObjectPropertyAddress {
        mSelector: selector(what),
        mScope: kAudioObjectPropertyScopeGlobal,
        mElement: kAudioObjectPropertyElementMain,
    }
}

/// The closure behind one registration, reachable through a thin pointer.
///
/// `dyn Fn` is fat and the client-data slot is a single `*mut c_void`, so the
/// closure is boxed twice. The address of the outer box is what CoreAudio
/// hands back to [`shim`] — and, together with the listener proc, is how it
/// identifies the registration at removal time. Two registrations sharing one
/// `Callback` would collapse into one and leak the other, which is the trap
/// the Swift comment calls out for blocks matching by identity.
struct Callback(Box<dyn Fn() + Send + Sync>);

/// One registration, removed on drop.
struct Listener {
    /// Kept because removal needs the address it was registered with.
    address: AudioObjectPropertyAddress,
    callback: Box<Callback>,
}

impl Listener {
    fn add<F>(what: DeviceChange, f: F) -> Result<Self>
    where
        F: Fn() + Send + Sync + 'static,
    {
        let callback = Box::new(Callback(Box::new(f)));
        let mut address = address(what);
        // SAFETY: `address` is live for the duration of the call and the HAL
        // copies it. `shim` has exactly the signature of
        // `AudioObjectPropertyListenerProc`, and the client pointer is the
        // `Callback` allocated above, which the returned `Listener` keeps
        // alive until after the matching removal has returned.
        let status = unsafe {
            AudioObjectAddPropertyListener(
                kAudioObjectSystemObject as AudioObjectID,
                NonNull::from(&mut address),
                Some(shim),
                &*callback as *const Callback as *mut c_void,
            )
        };
        if status != 0 {
            return Err(PlatformError::AudioDevice(format!(
                "cannot observe {what:?}: AudioObjectAddPropertyListener returned {status}"
            )));
        }
        Ok(Self { address, callback })
    }
}

impl Drop for Listener {
    fn drop(&mut self) {
        let mut address = self.address;
        // SAFETY: the same object, address, proc and client pointer as the
        // registration, which is the tuple CoreAudio matches on.
        let status = unsafe {
            AudioObjectRemovePropertyListener(
                kAudioObjectSystemObject as AudioObjectID,
                NonNull::from(&mut address),
                Some(shim),
                &*self.callback as *const Callback as *mut c_void,
            )
        };
        if status != 0 {
            // Leaking a listener is survivable; failing a shutdown is not.
            tracing::warn!(
                status,
                selector = self.address.mSelector,
                "AudioObjectRemovePropertyListener failed"
            );
        }
    }
}

/// The C entry point the HAL calls, on a HAL notification thread.
///
/// The address array is ignored on purpose: each registration owns a distinct
/// `Callback`, so which property fired is already encoded in `client`.
unsafe extern "C-unwind" fn shim(
    _object: AudioObjectID,
    _count: u32,
    _addresses: NonNull<AudioObjectPropertyAddress>,
    client: *mut c_void,
) -> i32 {
    // SAFETY: `client` is the `Callback` pointer given to
    // `AudioObjectAddPropertyListener`, which the owning `Listener` keeps
    // alive until `AudioObjectRemovePropertyListener` has returned.
    let callback = unsafe { &*(client as *const Callback) };
    // The ABI is `C-unwind`, so a panic here would unwind into CoreAudio and
    // take the process with it. Swallow it; the cost is one missed refresh.
    if catch_unwind(AssertUnwindSafe(|| (callback.0)())).is_err() {
        tracing::error!("audio device listener panicked");
    }
    // Apple documents the return value as unused.
    0
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicUsize, Ordering};

    use super::*;

    /// The selectors are the four-character codes `'dev#'` and `'dIn '`, which
    /// is what `AudioRecorder.swift` registers. Getting one wrong would
    /// register a listener that simply never fires, and no runtime check
    /// anywhere would notice.
    #[test]
    fn the_two_selectors_are_the_ones_swift_registers() {
        assert_eq!(selector(DeviceChange::List), u32::from_be_bytes(*b"dev#"));
        assert_eq!(
            selector(DeviceChange::DefaultInput),
            u32::from_be_bytes(*b"dIn ")
        );
        assert_ne!(
            selector(DeviceChange::List),
            selector(DeviceChange::DefaultInput),
            "two registrations on one selector would collapse into one"
        );
    }

    /// Both addresses target the whole system object, global scope, main
    /// element. A device-scoped address would never fire on the HAL.
    #[test]
    fn addresses_are_global_scope_main_element() {
        for what in CHANGES {
            let address = address(what);
            assert_eq!(address.mScope, kAudioObjectPropertyScopeGlobal);
            assert_eq!(address.mElement, kAudioObjectPropertyElementMain);
        }
    }

    /// Register against the live HAL and tear it down again. A non-zero
    /// `OSStatus` from either call fails the test, so this covers the FFI
    /// signature, the client-pointer discipline, and the fact that two
    /// distinct registrations on one object are accepted.
    ///
    /// Nothing asserts that the callback ran: making the machine's device list
    /// change from a unit test would mean editing the user's audio settings.
    /// `examples/probe.rs` covers that end.
    #[test]
    fn registers_and_deregisters_against_the_live_hal() {
        let fired = Arc::new(AtomicUsize::new(0));
        let seen = Arc::clone(&fired);
        let watcher = watch(move |_| {
            seen.fetch_add(1, Ordering::Relaxed);
        })
        .expect("the HAL must accept two system-object listeners");
        drop(watcher);
    }
}

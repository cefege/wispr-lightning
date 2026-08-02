//! System-wide WASAPI device-change notifications.
//!
//! One `IMMNotificationClient` registered on an `IMMDeviceEnumerator`, which is
//! the Windows answer to the two CoreAudio HAL listeners the macOS side
//! installs (`docs/parity/platform-spec.md` §2, MATRIX AUD-019):
//!
//! * `OnDefaultDeviceChanged` for the capture flow → the default input moved.
//! * `OnDeviceAdded` / `OnDeviceRemoved` / `OnDeviceStateChanged` → the device
//!   list changed.
//!
//! This is not what cpal's WASAPI backend already does for us. cpal builds a
//! `DefaultDeviceMonitor` per *stream*, purely to wake that stream's run loop,
//! and it never surfaces "a device appeared". The settings picker has no other
//! source for that fact.
//!
//! **Apartment.** Registration joins whatever apartment the process already
//! has: [`super::ensure_mta`] gives every COM-uninitialised thread an implicit
//! MTA, and Tauri owns the main thread as an STA for WebView2. A
//! `CoInitializeEx` here would either fail with `RPC_E_CHANGED_MODE` on the UI
//! thread or, worse, succeed on a worker and pin an apartment nobody pumps
//! messages for.
//!
//! **Threading.** Callbacks arrive on a system-owned MMDevice notification
//! thread. They set an atomic and push onto a channel — nothing else. In
//! particular they never touch the enumerator:
//! `UnregisterEndpointNotificationCallback` blocks until in-flight callbacks
//! return, so a callback that waited on the registration would deadlock its own
//! teardown.

use std::sync::Arc;

use windows::core::PCWSTR;
use windows::Win32::Foundation::PROPERTYKEY;
use windows::Win32::Media::Audio::{
    eCapture, eConsole, EDataFlow, ERole, IMMDeviceEnumerator, IMMNotificationClient,
    IMMNotificationClient_Impl, MMDeviceEnumerator, DEVICE_STATE,
};
use windows::Win32::System::Com::{CoCreateInstance, CLSCTX_ALL};

use crate::audio_impl::DeviceChange;
use crate::{PlatformError, Result};

/// A live endpoint-notification registration. Dropping it unregisters.
pub(crate) struct DeviceWatcher {
    enumerator: IMMDeviceEnumerator,
    client: IMMNotificationClient,
}

// SAFETY: both are free-threaded MMDevice objects, and this type only ever
// registers, unregisters and drops them. The notification path does not go
// through this struct at all.
unsafe impl Send for DeviceWatcher {}
unsafe impl Sync for DeviceWatcher {}

/// Observe machine-wide audio device changes until the returned watcher is
/// dropped.
///
/// `on_change` runs on a system MMDevice notification thread. It may run
/// concurrently with itself and must never block: Windows serialises endpoint
/// teardown behind these callbacks.
pub(crate) fn watch<F>(on_change: F) -> Result<DeviceWatcher>
where
    F: Fn(DeviceChange) + Send + Sync + 'static,
{
    super::ensure_mta();

    // SAFETY: in-process activation of a documented CLSID, with the apartment
    // established above.
    let enumerator: IMMDeviceEnumerator = unsafe {
        CoCreateInstance(&MMDeviceEnumerator, None, CLSCTX_ALL)
    }
    .map_err(|e| PlatformError::AudioDevice(format!("cannot create the device enumerator: {e}")))?;

    let client: IMMNotificationClient = Notifications {
        on_change: Arc::new(on_change),
    }
    .into();

    // SAFETY: `client` is owned by the returned watcher and outlives the
    // registration, which `Drop` removes.
    unsafe { enumerator.RegisterEndpointNotificationCallback(&client) }.map_err(|e| {
        PlatformError::AudioDevice(format!("cannot observe endpoint notifications: {e}"))
    })?;

    Ok(DeviceWatcher { enumerator, client })
}

impl Drop for DeviceWatcher {
    fn drop(&mut self) {
        // Drop can run on any thread, including one that never initialised COM.
        super::ensure_mta();
        // SAFETY: unregistering the exact interface pointer that was
        // registered. Synchronous — it waits for any in-flight callback, which
        // is only safe because those callbacks take no lock this thread could
        // be holding.
        if let Err(e) = unsafe {
            self.enumerator
                .UnregisterEndpointNotificationCallback(&self.client)
        } {
            // Leaking a registration is survivable; failing a shutdown is not.
            tracing::warn!(error = %e, "UnregisterEndpointNotificationCallback failed");
        }
    }
}

#[windows::core::implement(IMMNotificationClient)]
struct Notifications {
    on_change: Arc<dyn Fn(DeviceChange) + Send + Sync>,
}

impl IMMNotificationClient_Impl for Notifications_Impl {
    /// Windows fires this once per role, and for render as well as capture.
    ///
    /// macOS has no role concept and `kAudioHardwarePropertyDefaultInputDevice`
    /// is input-only, so narrowing to `eCapture` + `eConsole` is what makes both
    /// platforms report the same fact exactly once. `eConsole` is also the role
    /// cpal's `default_input_device` resolves, so this is the change that
    /// actually moves our microphone.
    fn OnDefaultDeviceChanged(
        &self,
        flow: EDataFlow,
        role: ERole,
        _id: &PCWSTR,
    ) -> windows::core::Result<()> {
        if flow == eCapture && role == eConsole {
            (self.on_change)(DeviceChange::DefaultInput);
        }
        Ok(())
    }

    fn OnDeviceAdded(&self, _id: &PCWSTR) -> windows::core::Result<()> {
        (self.on_change)(DeviceChange::List);
        Ok(())
    }

    fn OnDeviceRemoved(&self, _id: &PCWSTR) -> windows::core::Result<()> {
        (self.on_change)(DeviceChange::List);
        Ok(())
    }

    /// Unplugged, disabled and not-present endpoints never raise
    /// `OnDeviceRemoved`, so a jack disconnect is only visible here.
    ///
    /// Filtered by neither data flow nor target state: macOS's
    /// `kAudioHardwarePropertyDevices` covers every audio object on the
    /// machine, and re-enabling a device has to reach the picker just as
    /// disabling it does.
    fn OnDeviceStateChanged(
        &self,
        _id: &PCWSTR,
        _state: DEVICE_STATE,
    ) -> windows::core::Result<()> {
        (self.on_change)(DeviceChange::List);
        Ok(())
    }

    /// Ignored: a friendly-name or format change is not a change to the set of
    /// devices, and it fires often enough that forwarding it would thrash the
    /// microphone re-arm.
    fn OnPropertyValueChanged(
        &self,
        _id: &PCWSTR,
        _key: &PROPERTYKEY,
    ) -> windows::core::Result<()> {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Mutex;

    use windows::Win32::Media::Audio::{
        eCommunications, eMultimedia, eRender, DEVICE_STATE_ACTIVE, DEVICE_STATE_UNPLUGGED,
    };

    use super::*;

    /// Build the COM object without an enumerator or any hardware, so every
    /// callback can be invoked directly and its mapping pinned.
    fn client() -> (
        IMMNotificationClient,
        Arc<Mutex<Vec<DeviceChange>>>,
        Arc<AtomicUsize>,
    ) {
        let seen = Arc::new(Mutex::new(Vec::new()));
        let count = Arc::new(AtomicUsize::new(0));
        let (sink, hits) = (Arc::clone(&seen), Arc::clone(&count));
        let client: IMMNotificationClient = Notifications {
            on_change: Arc::new(move |change| {
                sink.lock().expect("test sink poisoned").push(change);
                hits.fetch_add(1, Ordering::Relaxed);
            }),
        }
        .into();
        (client, seen, count)
    }

    fn drained(seen: &Arc<Mutex<Vec<DeviceChange>>>) -> Vec<DeviceChange> {
        std::mem::take(&mut *seen.lock().expect("test sink poisoned"))
    }

    /// Every hot-plug shape reports a list change and nothing stronger: the
    /// running stream is still valid, so reporting a default move here would
    /// rebuild it for no reason.
    #[test]
    fn hot_plug_callbacks_report_a_list_change() {
        let (client, seen, _) = client();
        // SAFETY: calling our own in-process COM object; the device id is
        // ignored by every implementation here.
        unsafe {
            client.OnDeviceAdded(PCWSTR::null()).unwrap();
            client.OnDeviceRemoved(PCWSTR::null()).unwrap();
            client
                .OnDeviceStateChanged(PCWSTR::null(), DEVICE_STATE_UNPLUGGED)
                .unwrap();
            client
                .OnDeviceStateChanged(PCWSTR::null(), DEVICE_STATE_ACTIVE)
                .unwrap();
        }
        assert_eq!(drained(&seen), vec![DeviceChange::List; 4]);
    }

    #[test]
    fn the_console_capture_default_is_the_only_default_that_counts() {
        let (client, seen, count) = client();
        // SAFETY: as above.
        unsafe {
            client
                .OnDefaultDeviceChanged(eCapture, eConsole, PCWSTR::null())
                .unwrap();
        }
        assert_eq!(drained(&seen), vec![DeviceChange::DefaultInput]);
        assert_eq!(count.load(Ordering::Relaxed), 1);
    }

    /// Render defaults and the non-console roles must stay silent. macOS
    /// reports an input-default move exactly once; forwarding all three roles
    /// would re-arm the microphone three times for one user action.
    #[test]
    fn other_flows_and_roles_are_ignored() {
        let (client, _, count) = client();
        // SAFETY: as above.
        unsafe {
            client
                .OnDefaultDeviceChanged(eRender, eConsole, PCWSTR::null())
                .unwrap();
            client
                .OnDefaultDeviceChanged(eCapture, eMultimedia, PCWSTR::null())
                .unwrap();
            client
                .OnDefaultDeviceChanged(eCapture, eCommunications, PCWSTR::null())
                .unwrap();
        }
        assert_eq!(count.load(Ordering::Relaxed), 0);
    }

    /// A property-value change is not a change to the set of devices.
    #[test]
    fn property_changes_do_not_disturb_the_picker() {
        let (client, _, count) = client();
        // SAFETY: as above.
        unsafe {
            client
                .OnPropertyValueChanged(PCWSTR::null(), PROPERTYKEY::default())
                .unwrap();
        }
        assert_eq!(count.load(Ordering::Relaxed), 0);
    }
}

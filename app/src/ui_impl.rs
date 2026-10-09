//! The pipeline's [`Ui`], realised as bus publications.
//!
//! Called from pipeline tasks at up to 25 Hz, so every method is a channel
//! send and nothing more; the overlay component and the tray consume on the
//! main thread.

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

use wl_platform::audio::AudioCapture;
use wl_shell::overlay_geometry::ERROR_DISMISS;
use wl_shell::ui::{Elapsed, OverlayState, Ui};

use crate::bus::{Bus, TrayCommand, DEVICES_CHANGED};

pub struct DioxusUi {
    bus: Arc<Bus>,
    audio: Arc<dyn AudioCapture>,
    /// The pipeline may call in from threads outside the runtime (the hotkey
    /// hook, the audio callback), so the dismiss timer is spawned on a stored
    /// handle rather than on "the current runtime".
    runtime: tokio::runtime::Handle,
    /// Bumped on every state change. A transient error's auto-dismiss only
    /// fires if nothing has happened since it was scheduled (OVL-024).
    generation: Arc<AtomicU64>,
}

impl DioxusUi {
    pub fn new(
        bus: Arc<Bus>,
        audio: Arc<dyn AudioCapture>,
        runtime: tokio::runtime::Handle,
    ) -> Self {
        Self {
            bus,
            audio,
            runtime,
            generation: Arc::new(AtomicU64::new(0)),
        }
    }
}

impl Ui for DioxusUi {
    fn set_overlay(&self, state: OverlayState) {
        // First, so a pending auto-dismiss sees the new generation.
        let generation = self.generation.fetch_add(1, Ordering::AcqRel) + 1;
        let transient = matches!(state, OverlayState::Error { .. });
        self.bus.overlay_state.send_replace(state);

        // OVL-024 / OVL-025: transient errors dismiss themselves, recoverable
        // ones persist until the user acts.
        if transient {
            let bus = Arc::clone(&self.bus);
            let current = Arc::clone(&self.generation);
            self.runtime.spawn(async move {
                tokio::time::sleep(ERROR_DISMISS).await;
                if current.load(Ordering::Acquire) == generation {
                    bus.overlay_state.send_replace(OverlayState::Hidden);
                }
            });
        }
    }

    fn set_elapsed(&self, elapsed: Elapsed) {
        self.bus.elapsed.send_replace(elapsed);
    }

    fn set_level(&self, level: f32) {
        self.bus.level.send_replace(level);
    }

    fn set_recording_indicator(&self, recording: bool) {
        self.bus.tray(TrayCommand::Recording(recording));
    }

    fn set_last_transcription(&self, text: &str) {
        self.bus
            .tray(TrayCommand::LastTranscription(text.to_string()));
    }

    fn notify_changed(&self, topic: &str) {
        let Some(topic) = Bus::topic(topic) else {
            tracing::warn!(topic, "dropping a change notification nothing listens for");
            return;
        };
        if topic == DEVICES_CHANGED {
            // TRY-006: the device submenu must follow an unplug, even
            // mid-recording.
            match self.audio.list_devices() {
                Ok(devices) => self.bus.tray(TrayCommand::Devices(devices)),
                Err(e) => tracing::warn!(error = %e, "could not re-enumerate devices"),
            }
        }
        let _ = self.bus.changed.send(topic);
    }
}

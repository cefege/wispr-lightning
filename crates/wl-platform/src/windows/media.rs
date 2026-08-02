//! Pausing whatever is playing while a dictation is in flight.
//!
//! The Swift version drives Apple Music and Spotify by AppleScript and knows
//! about nothing else. The System Media Transport Controls session manager
//! covers every player that registers with the shell — browsers included — so
//! this is a strict superset, and it needs no per-app scripting.
//!
//! One behaviour is preserved deliberately and one bug is not. Preserved: we
//! resume only what we paused, so a player the user had already stopped stays
//! stopped. Fixed: the Swift `pauseMusic` is fired and forgotten on a
//! background queue, so a very short recording can resume before the pause has
//! recorded its flag and leave the music paused forever. Here `pause` is
//! synchronous and its return value is the caller's signal.

use std::time::Duration;

use parking_lot::Mutex;
use windows::Media::Control::{
    GlobalSystemMediaTransportControlsSessionManager as SessionManager,
    GlobalSystemMediaTransportControlsSessionPlaybackStatus as PlaybackStatus,
};

use crate::MediaControl;

/// Ceiling for one SMTC round trip. Generous: the manager is a shell service
/// and each `TryPauseAsync` is a cross-process call to the player.
const SMTC_BUDGET: Duration = Duration::from_secs(2);

pub struct WindowsMedia {
    /// `SourceAppUserModelId` of every session we paused, so resume touches
    /// nothing else.
    paused: Mutex<Vec<String>>,
}

impl WindowsMedia {
    pub fn new() -> Self {
        Self {
            paused: Mutex::new(Vec::new()),
        }
    }
}

impl Default for WindowsMedia {
    fn default() -> Self {
        Self::new()
    }
}

impl MediaControl for WindowsMedia {
    fn pause(&self) -> bool {
        let paused = super::bounded("smtc-pause", SMTC_BUDGET, pause_playing).unwrap_or_default();
        let any = !paused.is_empty();
        if any {
            tracing::debug!(count = paused.len(), "paused media sessions");
        }
        *self.paused.lock() = paused;
        any
    }

    fn resume(&self) {
        let sessions = std::mem::take(&mut *self.paused.lock());
        if sessions.is_empty() {
            return;
        }
        super::bounded("smtc-resume", SMTC_BUDGET, move || {
            resume_sessions(&sessions)
        });
    }
}

fn manager() -> Option<SessionManager> {
    super::ensure_mta();
    SessionManager::RequestAsync()
        .and_then(|operation| operation.get())
        .inspect_err(|e| tracing::debug!(error = %e, "no media session manager"))
        .ok()
}

fn pause_playing() -> Vec<String> {
    let Some(manager) = manager() else {
        return Vec::new();
    };
    let Ok(sessions) = manager.GetSessions() else {
        return Vec::new();
    };
    let mut paused = Vec::new();
    for session in sessions {
        let playing = session
            .GetPlaybackInfo()
            .and_then(|info| info.PlaybackStatus())
            .map(|status| status == PlaybackStatus::Playing)
            .unwrap_or(false);
        if !playing {
            continue;
        }
        let accepted = session
            .TryPauseAsync()
            .and_then(|operation| operation.get())
            .unwrap_or(false);
        if accepted {
            if let Ok(id) = session.SourceAppUserModelId() {
                paused.push(id.to_string());
            }
        }
    }
    paused
}

fn resume_sessions(wanted: &[String]) {
    let Some(manager) = manager() else {
        return;
    };
    let Ok(sessions) = manager.GetSessions() else {
        return;
    };
    for session in sessions {
        let Ok(id) = session.SourceAppUserModelId() else {
            continue;
        };
        // Converted once per session rather than once per candidate: `id` is a
        // UTF-16 `HSTRING`, so the comparison cannot borrow it as a `str`.
        let id = id.to_string();
        if !wanted.contains(&id) {
            continue;
        }
        if let Err(e) = session.TryPlayAsync().and_then(|operation| operation.get()) {
            tracing::debug!(error = %e, "media session refused to resume");
        }
    }
}

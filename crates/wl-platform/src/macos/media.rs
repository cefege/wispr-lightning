//! Pause and resume the user's music while they dictate.
//!
//! AppleScript is the only way to reach Apple Music's and Spotify's player
//! state: neither publishes it through `MPNowPlayingInfoCenter`, and the
//! system-wide remote-control APIs are private. Windows has no equivalent
//! problem — SMTC (`GlobalSystemMediaTransportControlsSessionManager`) reports
//! `PlaybackStatus` for every player — so `MediaControl` there is a superset
//! of what this can see.
//!
//! `NSAppleScript` is the one class Apple's Thread Safety Summary lists under
//! *Main Thread Only Classes*, and the rule has teeth: since the December 2025
//! XProtect update a script object first created off the main thread hangs the
//! process. Both scripts therefore run inside [`main_thread::run`], which is
//! also why they run one after another rather than side by side — the main
//! queue is serial, so fanning out would buy nothing.

use std::sync::Arc;
use std::time::Duration;

use objc2::rc::Retained;
use objc2::runtime::AnyObject;
use objc2::{msg_send, AnyThread, MainThreadMarker};
use objc2_app_kit::NSWorkspace;
use objc2_foundation::{NSAppleEventDescriptor, NSAppleScript, NSString};
use parking_lot::Mutex;
use tracing::debug;

use super::main_thread;
use crate::MediaControl;

/// One controllable player: its bundle id and the two scripts that drive it.
struct Player {
    bundle_id: &'static str,
    /// Pauses only if something is playing, and says so. The conditional lives
    /// inside the script because a separate "is it playing?" query would race
    /// with the pause.
    pause: &'static str,
    resume: &'static str,
}

const PLAYERS: [Player; 2] = [
    Player {
        bundle_id: "com.apple.Music",
        pause: "tell application \"Music\" to if player state is playing then\npause\nreturn \"paused\"\nend if",
        resume: "tell application \"Music\" to play",
    },
    Player {
        bundle_id: "com.spotify.client",
        pause: "tell application \"Spotify\" to if player state is playing then\npause\nreturn \"paused\"\nend if",
        resume: "tell application \"Spotify\" to play",
    },
];

/// The reply the pause scripts return when they actually paused something.
const PAUSED: &str = "paused";

/// How long a caller waits for the main thread to finish a round of scripts.
///
/// Generous, because the very first script raises the Automation consent
/// prompt and does not return until the user answers it. Overshooting is
/// harmless: the block is not cancelled, it stays queued and still runs, and
/// it latches the pause flags itself — so a caller that gives up waiting can
/// only lose the return value, never the knowledge that music must be resumed.
const SCRIPT_TIMEOUT: Duration = Duration::from_secs(5);

/// Which players we paused, so `resume` never starts music the user had
/// stopped themselves.
///
/// Shared rather than owned so the main-thread blocks can latch into it: they
/// outlive the call that queued them.
type PausedFlags = Arc<Mutex<[bool; PLAYERS.len()]>>;

pub struct MacMedia {
    paused: PausedFlags,
}

impl Default for MacMedia {
    fn default() -> Self {
        Self::new()
    }
}

impl MacMedia {
    pub fn new() -> Self {
        Self {
            paused: Arc::new(Mutex::new([false; PLAYERS.len()])),
        }
    }
}

/// Consume the "we paused this" flags.
///
/// Cleared on read, so a duplicate resume is a no-op — the same bookkeeping
/// the Swift original had, and the reason resuming never starts music the user
/// stopped themselves.
fn take_paused(flags: &Mutex<[bool; PLAYERS.len()]>) -> [bool; PLAYERS.len()] {
    std::mem::take(&mut *flags.lock())
}

impl MediaControl for MacMedia {
    fn pause(&self) -> bool {
        // Which players exist is an `NSWorkspace` query with no main-thread
        // affinity, so it stays here and keeps the main thread holding nothing
        // but the scripts. It also short-circuits the common case: with no
        // music player running there is no hop at all.
        let live = PLAYERS.map(|player| running(player.bundle_id));
        if !live.iter().any(|&r| r) {
            return false;
        }

        // Unlike the Swift original this is synchronous end to end: there the
        // pause was fired and forgotten, so a very short recording could call
        // resume before the pause flag had been stored and leave the music off.
        let flags = Arc::clone(&self.paused);
        let any = main_thread::run(
            move |mtm| {
                let mut any = false;
                for (index, player) in PLAYERS.iter().enumerate() {
                    if !live[index] || run_script(mtm, player.pause).as_deref() != Some(PAUSED) {
                        continue;
                    }
                    // Latch rather than overwrite: a second pause before a
                    // resume must not forget that we already stopped a player.
                    flags.lock()[index] = true;
                    any = true;
                }
                any
            },
            SCRIPT_TIMEOUT,
        );

        match any {
            Some(true) => {
                debug!("paused music for the duration of the recording");
                true
            }
            Some(false) => false,
            None => {
                // The block is still queued. Reporting `false` is honest about
                // what we know; the flags it sets are what `resume` acts on.
                debug!("the main thread has not run the pause scripts yet");
                false
            }
        }
    }

    fn resume(&self) {
        // The flags are read inside the block rather than out here, so a
        // `pause` that gave up waiting for its own block cannot be overtaken:
        // both hop onto the same serial main queue, in call order, so by the
        // time this runs the pause has finished latching. Both directions of
        // the pipeline call this from a worker, which is what makes that hold
        // — a resume issued from the main thread would run inline and skip the
        // queue, so if one is ever added it must hop as well.
        let flags = Arc::clone(&self.paused);
        main_thread::run(
            move |mtm| {
                for (player, was_paused) in PLAYERS.iter().zip(take_paused(&flags)) {
                    // `tell application "Music" to play` *launches* Music if it
                    // is not running, so a player the user quit mid-dictation
                    // must be left alone rather than resurrected.
                    if was_paused && running(player.bundle_id) {
                        run_script(mtm, player.resume);
                    }
                }
            },
            SCRIPT_TIMEOUT,
        );
    }
}

fn running(bundle_id: &str) -> bool {
    NSWorkspace::sharedWorkspace()
        .runningApplications()
        .iter()
        .any(|app| {
            app.bundleIdentifier()
                .is_some_and(|id| id.to_string() == bundle_id)
        })
}

/// Execute an AppleScript, returning its string result.
///
/// Takes a [`MainThreadMarker`] rather than trusting the caller: `NSAppleScript`
/// is main-thread-only, and the marker is the proof. It is also why the script
/// object never leaves this function — a `Retained<NSAppleScript>` handed back
/// to a worker would put the requirement right back in a comment.
///
/// Failures are swallowed: a player that is quitting, mid-launch, or blocked
/// behind the Automation consent prompt must not fail a dictation.
///
/// `executeAndReturnError:` is invoked by hand because it returns nil on
/// failure, which is exactly the common case here — the generated binding
/// types the result as non-null and panics instead.
fn run_script(_mtm: MainThreadMarker, source: &str) -> Option<String> {
    let script =
        NSAppleScript::initWithSource(NSAppleScript::alloc(), &NSString::from_str(source))?;
    let mut error: *mut AnyObject = std::ptr::null_mut();
    // SAFETY: the selector takes one `NSDictionary **` out-parameter, `error`
    // is a live slot for it, and the result is an autoreleased descriptor or
    // nil — which the `Option` return type accounts for.
    let descriptor: Option<Retained<NSAppleEventDescriptor>> =
        unsafe { msg_send![&*script, executeAndReturnError: &mut error] };
    // SAFETY: on failure AppleScript hands back an autoreleased error
    // dictionary; retaining it keeps it alive past any pool drain.
    if let Some(error) = unsafe { Retained::retain(error) } {
        debug!("AppleScript failed: {error:?}");
        return None;
    }
    descriptor?.stringValue().map(|s| s.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `resume` must only touch players `pause` actually stopped, or dictating
    /// would start music the user had deliberately paused.
    ///
    /// Exercises the bookkeeping alone: driving the AppleScript half needs a
    /// running music player, so that lives in `examples/probe.rs`.
    #[test]
    fn the_paused_flags_are_consumed_exactly_once() {
        let media = MacMedia::new();
        *media.paused.lock() = [true, false];
        assert_eq!(take_paused(&media.paused), [true, false]);
        assert_eq!(
            take_paused(&media.paused),
            [false, false],
            "a second resume must be a no-op"
        );
    }

    /// Pausing twice before a resume must not forget the first pause.
    #[test]
    fn a_second_pause_does_not_clear_an_outstanding_flag() {
        let media = MacMedia::new();
        *media.paused.lock() = [true, false];
        // What `pause` does with a round in which nothing was playing: it only
        // ever sets a flag, so an outstanding one survives.
        for (index, paused_now) in [false, false].into_iter().enumerate() {
            if paused_now {
                media.paused.lock()[index] = true;
            }
        }
        assert_eq!(take_paused(&media.paused), [true, false]);
    }

    /// The scripts are a wire contract with two closed-source apps; a typo
    /// here fails silently at runtime.
    #[test]
    fn every_pause_script_reports_the_sentinel_the_caller_matches_on() {
        for player in &PLAYERS {
            assert!(
                player.pause.contains(&format!("return \"{PAUSED}\"")),
                "{} pause script does not return the sentinel",
                player.bundle_id
            );
            assert!(
                player.pause.contains("if player state is playing then"),
                "{} pause script is unconditional",
                player.bundle_id
            );
        }
    }
}

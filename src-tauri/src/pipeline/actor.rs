//! The recording actor: one task, one owner of the state machine.
//!
//! Every mutation of recording state happens here and nowhere else, which
//! is what removes the lock ordering the AppKit original had to reason
//! about. Anything that can block — network, accessibility, OCR, disk — is
//! spawned off and rejoins through the mailbox or through [`Deps::pending`].

use super::transcribe::{
    finish_live_transcription, live_attempt, run_transcription, start_live_attempt, LiveAttempt,
};
use super::*;

/// State of the recording currently in progress.
struct RecordingSession {
    /// Sampled at the press, not at injection time: switching apps mid-
    /// dictation must not change where the text is reported as going (CTX-009).
    app: AppInfo,
    started: Instant,
    transcript_id: String,
    live: LiveAttempt,
}

pub(super) struct Actor {
    deps: Arc<Deps>,
    tx: mpsc::UnboundedSender<Command>,
    capturing: Arc<AtomicBool>,
    machine: Machine,
    stop_generation: u64,
    tick: Option<JoinHandle<()>>,
    rearm: Option<JoinHandle<()>>,
    session: Option<RecordingSession>,
    warning: WarningState,
    /// A device fault invalidated the capture stream; rebuild it before the
    /// next recording rather than mid-take.
    needs_stream_rebuild: bool,
}

impl Actor {
    /// Build the actor, push settings out to the backends, and hand it its
    /// mailbox. Construction and startup are one call so no half-configured
    /// actor can ever be observed.
    pub(super) fn start(
        deps: Arc<Deps>,
        tx: mpsc::UnboundedSender<Command>,
        rx: mpsc::UnboundedReceiver<Command>,
        capturing: Arc<AtomicBool>,
    ) {
        let mut actor = Self {
            deps,
            tx,
            capturing,
            machine: Machine::new(),
            stop_generation: 0,
            tick: None,
            rearm: None,
            session: None,
            warning: WarningState::default(),
            needs_stream_rebuild: false,
        };
        // Bindings, sound pack and microphone come from settings on the way up,
        // exactly as they do on every later change.
        actor.apply_settings();
        actor.prewarm_microphone();
        tokio::spawn(actor.run(rx));
    }

    async fn run(mut self, mut rx: mpsc::UnboundedReceiver<Command>) {
        while let Some(command) = rx.recv().await {
            match command {
                Command::Hotkey(event) => self.on_hotkey(event),
                Command::StopTimer(generation) => {
                    if generation == self.stop_generation {
                        self.feed(Event::StopTimerFired);
                    }
                }
                Command::Tick => self.on_tick(),
                Command::Faults(faults) => self.on_faults(&faults),
                Command::Rearm => self.rearm_microphone(),
                Command::Overlay(action) => self.on_overlay_action(action),
                Command::SettingsChanged => self.on_settings_changed(),
                Command::Recovery(recovered) => self.on_recovery(*recovered),
                Command::Abort => self.feed(Event::Abort),
            }
        }
    }

    // -- Hotkeys ---------------------------------------------------------

    fn on_hotkey(&mut self, event: wl_platform::hotkey::HotkeyEvent) {
        if self.capturing.load(Ordering::SeqCst) {
            return;
        }
        match (event.binding, event.transition) {
            (Binding::Dictate, Transition::Pressed) => self.feed(Event::Press(Instant::now())),
            (Binding::Dictate, Transition::Released) => self.feed(Event::Release(Instant::now())),
            // The chord guard cancelled a push-to-talk hold: the user was
            // reaching for a shortcut, so the take is thrown away.
            //
            // The `Listening` test is not a missing case, so please do not
            // "complete" it. `Event::Abort` is deliberately state-agnostic —
            // every other caller depends on that — and the guard must not
            // reach hands-free recording, where the modifier is not held and a
            // `Ctrl+C` is an ordinary shortcut. The one instant where the
            // backend can still see a held modifier in `Locked` is the locking
            // press itself, and this is what makes that instant inert. Putting
            // the condition inside the machine instead would make every other
            // abort lie.
            (Binding::Dictate, Transition::Aborted) => {
                if matches!(self.machine.state(), State::Listening { .. }) {
                    self.feed(Event::Abort);
                }
            }
        }
    }

    fn feed(&mut self, event: Event) {
        // Read the behaviour per event rather than caching it: the settings
        // window can change the picker while the key is still down.
        let behavior = self.deps.settings.read().press_behavior();
        for action in self.machine.handle(event, behavior) {
            match action {
                Action::StartRecording => self.start_recording(),
                Action::StopRecording => self.stop_recording(),
                Action::AbortRecording => self.abort_recording(),
                Action::ShowLocked => self.deps.ui.set_overlay(OverlayState::Locked),
                Action::ScheduleStop(delay) => self.schedule_stop(delay),
                Action::CancelScheduledStop => self.stop_generation += 1,
            }
        }
    }

    fn schedule_stop(&mut self, delay: Duration) {
        self.stop_generation += 1;
        let generation = self.stop_generation;
        let tx = self.tx.clone();
        tokio::spawn(async move {
            tokio::time::sleep(delay).await;
            let _ = tx.send(Command::StopTimer(generation));
        });
    }

    /// Drop back to idle without emitting the FSM's cleanup actions, for paths
    /// that have already cleaned up (a failed start, an auto-stop).
    fn force_idle(&mut self) {
        // Abort is behaviour-independent, so the argument here is inert.
        let _ = self.machine.handle(Event::Abort, PressBehavior::default());
        self.stop_generation += 1;
    }

    // -- Start -----------------------------------------------------------

    fn start_recording(&mut self) {
        // Before anything else: the frontmost app must be whatever the user was
        // looking at when they pressed the key.
        let app = self.deps.platform.foreground.current();
        let settings = self.deps.settings.read().clone();

        // Refuse before the microphone opens, not after the user has spoken.
        //
        // `start()` would raise `NotConfigured` anyway, but only once there is
        // audio to send — so the user dictates a paragraph and is then told
        // there is no API key, with the take discarded. Asking the provider
        // turns a wasted recording into an answerable message.
        //
        // `is_ready` is the cheap, no-network credential check.
        let provider = self.deps.provider.read().clone();
        if !provider.is_ready() {
            let message = wl_providers::ProviderError::NotConfigured {
                provider: "Deepgram",
            }
            .user_message();
            tracing::info!(provider = "deepgram", "refusing to record: not configured");
            self.deps.ui.set_overlay(OverlayState::Error { message });
            self.force_idle();
            return;
        }

        self.deps.sound.play(Cue::Start);

        if self.needs_stream_rebuild {
            // A device went away since the last take; a pre-warmed stream
            // bound to it would produce silence.
            if let Err(error) = self.deps.audio.release() {
                tracing::warn!(%error, "could not release the stale capture stream");
            }
            self.needs_stream_rebuild = false;
        }

        // Feed the pill's VU strip for the duration of the take. Installed
        // before `start()` so no early buffer is dropped, and cleared in
        // `stop_recording`/`abort_recording` so a late level cannot repaint a
        // hidden overlay. The closure runs on the audio worker at ~25 Hz, so it
        // does nothing but hand the value to the UI layer, which forwards it as
        // an event — no locks the audio path also wants, no window geometry.
        let ui = self.deps.ui.clone();
        self.deps
            .audio
            .set_level_sink(Some(std::sync::Arc::new(move |level: f32| {
                ui.set_level(level);
            })));

        // Install the packet relay before arming capture. The provider handshake
        // runs asynchronously, so early packets queue without leaving a gap at
        // the beginning of the sentence.
        let (live, ingress) = live_attempt();
        self.deps.audio.set_packet_sink(Some(live.packet_sink()));

        match self.deps.audio.start() {
            Ok(StartOutcome::Started) => {}
            Ok(StartOutcome::StartedWithFallback { requested }) => {
                // Deliberately silent to the user, as in the original.
                tracing::warn!(
                    %requested,
                    "recording started with fallback mic (requested device unavailable)"
                );
            }
            Err(error) => {
                self.deps.audio.set_packet_sink(None);
                self.deps.audio.set_level_sink(None);
                live.cancel();
                tracing::error!(%error, "failed to start recording");
                self.force_idle();
                // A permission denial already names the exact settings pane
                // (and on Windows the `ms-settings:` URI); replacing that with
                // "Mic unavailable" would strand the user. Everything else
                // keeps the original's wording.
                let message = match error {
                    wl_platform::PlatformError::PermissionDenied(_) => error.to_string(),
                    _ => MSG_MIC_UNAVAILABLE.to_string(),
                };
                self.deps.ui.set_overlay(OverlayState::Error { message });
                resume_music(&self.deps);
                return;
            }
        }
        let started = Instant::now();

        // Pausing media is an out-of-process round trip (AppleScript / SMTC)
        // and must not sit between the key press and the first sample.
        if settings.mute_music {
            let media = self.deps.platform.media.clone();
            tokio::task::spawn_blocking(move || media.pause());
        }

        let ax = settings.use_accessibility_context.then(|| {
            let injector = self.deps.platform.injector.clone();
            tokio::task::spawn_blocking(move || injector.read_focused_text())
        });
        let ocr = settings.use_screen_context.then(|| {
            let screen = self.deps.platform.screen.clone();
            tokio::task::spawn_blocking(move || screen.ocr_frontmost_window(OCR_MAX_LINES))
        });

        let transcript_id = uuid::Uuid::new_v4().to_string().to_uppercase();
        start_live_attempt(
            self.deps.clone(),
            app.clone(),
            ax,
            ocr,
            transcript_id.clone(),
            ingress,
        );

        self.deps.ui.set_recording_indicator(true);
        self.deps.ui.set_overlay(OverlayState::Recording);
        self.warning.reset();
        self.deps.ui.set_elapsed(Elapsed::default());
        self.session = Some(RecordingSession {
            app,
            started,
            transcript_id,
            live,
        });
        self.start_tick();
        tracing::info!("recording started");
    }

    fn start_tick(&mut self) {
        self.cancel_tick();
        let tx = self.tx.clone();
        let period = self.deps.timings.tick;
        self.tick = Some(tokio::spawn(async move {
            let mut interval = tokio::time::interval(period);
            // `interval` fires immediately; the first elapsed readout is due a
            // full period after the press.
            interval.tick().await;
            loop {
                interval.tick().await;
                if tx.send(Command::Tick).is_err() {
                    break;
                }
            }
        }));
    }

    fn cancel_tick(&mut self) {
        if let Some(handle) = self.tick.take() {
            handle.abort();
        }
    }

    fn on_tick(&mut self) {
        let Some(session) = &self.session else { return };
        let elapsed = session.started.elapsed().as_secs();
        match self.warning.tick(elapsed) {
            Tick::AutoStop => {
                tracing::info!(elapsed, "max recording duration reached, auto-stopping");
                self.force_idle();
                self.stop_recording();
            }
            Tick::Continue { warning } => self.deps.ui.set_elapsed(Elapsed {
                label: elapsed_label(elapsed, warning),
                warning: warning.level(),
            }),
        }
    }

    // -- Stop ------------------------------------------------------------

    fn stop_recording(&mut self) {
        self.cancel_tick();
        // The take is over: no further levels, so a stale one cannot land on
        // the Processing pill or survive into the next recording's first frame.
        self.deps.audio.set_level_sink(None);
        let Some(RecordingSession {
            app,
            started,
            transcript_id,
            live,
        }) = self.session.take()
        else {
            return;
        };
        let elapsed = started.elapsed();

        let packets = self.deps.audio.stop();
        self.deps.audio.set_packet_sink(None);
        self.deps.sound.play(Cue::Stop);
        self.deps.ui.set_recording_indicator(false);

        if packets.len() < MIN_PACKETS {
            // Under 200 ms. Almost always a mis-press, and the backend would
            // charge us for it.
            live.cancel();
            self.deps.provider.read().reset();
            if packets.is_empty() && elapsed > DEAD_MIC_THRESHOLD {
                tracing::warn!(
                    seconds = elapsed.as_secs_f32(),
                    "recording captured 0 packets; the mic is probably gone"
                );
                self.deps.ui.set_overlay(OverlayState::Error {
                    message: MSG_MIC_NOT_RESPONDING.into(),
                });
            } else {
                tracing::info!(packets = packets.len(), "too short, ignoring");
                self.deps.ui.set_overlay(OverlayState::Hidden);
            }
            resume_music(&self.deps);
            return;
        }

        self.deps.ui.set_overlay(OverlayState::Processing);

        let packets = Arc::new(packets);
        *self.deps.pending.lock() = Some(Pending {
            packets: packets.clone(),
            app,
            ocr: Vec::new(),
            ax: Vec::new(),
            transcript_id,
            spool_path: None,
        });

        // Get the audio onto disk before the network is involved, so a crash
        // or a failed retry sequence still leaves something to recover.
        let deps = self.deps.clone();
        let to_spool = packets.clone();
        tokio::task::spawn_blocking(move || match deps.spool.save(&to_spool) {
            Ok(path) => {
                let mut pending = deps.pending.lock();
                // Only if this is still the same recording: a fast second
                // dictation must not adopt the previous take's file.
                if let Some(pending) = pending.as_mut() {
                    if Arc::ptr_eq(&pending.packets, &to_spool) {
                        pending.spool_path = Some(path);
                        return;
                    }
                }
                drop(pending);
                deps.spool.delete(&path);
            }
            Err(error) => tracing::error!(%error, "could not spool the recording"),
        });

        let deps = self.deps.clone();
        tokio::spawn(finish_live_transcription(deps, live.finish()));
    }

    fn abort_recording(&mut self) {
        self.cancel_tick();
        if let Some(session) = self.session.take() {
            session.live.cancel();
        }
        // Detach the VU sink before stopping, so a level already in flight
        // cannot repaint a pill we are about to hide.
        self.deps.audio.set_packet_sink(None);
        self.deps.audio.set_level_sink(None);
        let _ = self.deps.audio.stop();
        self.deps.provider.read().reset();
        discard_pending(&self.deps);
        self.deps.ui.set_recording_indicator(false);
        self.deps.ui.set_overlay(OverlayState::Hidden);
        resume_music(&self.deps);
        self.deps.hotkeys.reset();
        tracing::info!("recording aborted");
    }

    /// The ✕ on the recording pill.
    ///
    /// Routed through the state machine's `Abort` rather than calling
    /// [`Self::abort_recording`] directly, so the FSM lands in `Idle` and the
    /// scheduled-stop timer is cancelled with it. Calling the effect without
    /// the transition would leave the machine believing it is still recording,
    /// and the next hotkey press would be read as "stop" instead of "start".
    ///
    /// Discarding the audio is correct here and only here: the user asked for
    /// this dictation to go away. `abort_recording` already deletes the spooled
    /// artifact via `discard_pending`, which is what stops the next launch
    /// offering to recover the take they just cancelled.
    fn cancel_recording(&mut self) {
        if !self.machine.is_recording() {
            return;
        }
        tracing::info!("recording cancelled by the user");
        self.feed(Event::Abort);
    }

    // -- Devices ---------------------------------------------------------

    fn on_faults(&mut self, faults: &[CaptureFault]) {
        let recording = self.machine.is_recording();
        for fault in faults {
            match fault {
                CaptureFault::DeviceLost | CaptureFault::StreamInvalidated => {
                    // Matching the original: an in-flight recording is not
                    // stopped, packets simply stop arriving and the take ends
                    // where the user ends it.
                    tracing::warn!(?fault, recording, "capture device fault");
                    self.needs_stream_rebuild = true;
                }
                CaptureFault::SilentInput => {
                    tracing::warn!("capture produced only silence");
                    // Never over a Processing overlay: clobbering it would hide
                    // a transcription the user is still waiting on.
                    if !self.deps.transcribing.load(Ordering::SeqCst) {
                        self.deps.ui.set_overlay(OverlayState::Error {
                            message: MSG_SILENT_INPUT.into(),
                        });
                    }
                }
                CaptureFault::DefaultChanged => {
                    tracing::info!("default input device changed");
                    self.needs_stream_rebuild = true;
                }
                CaptureFault::DevicesChanged => {
                    // Deliberately no rebuild: a microphone appearing or
                    // disappearing elsewhere on the machine says nothing about
                    // the stream we are holding. The picker below is refreshed
                    // and the re-arm re-resolves the configured device; if the
                    // stream really did die, the capture layer says so with
                    // DeviceLost.
                    tracing::info!("audio device list changed");
                }
                CaptureFault::Overrun => tracing::debug!("capture overrun"),
            }
        }

        // An overrun says nothing about the device set, so it must not drag the
        // microphone through a re-open the user would hear as a gap.
        let device_changed = faults.iter().any(|fault| {
            matches!(
                fault,
                CaptureFault::DeviceLost
                    | CaptureFault::StreamInvalidated
                    | CaptureFault::DefaultChanged
                    | CaptureFault::DevicesChanged
            )
        });
        if !device_changed {
            return;
        }

        // The picker is stale either way, so the menu refreshes even mid-
        // recording; only the re-arm waits for the take to finish.
        self.deps.ui.notify_changed("devices");
        if !recording {
            self.schedule_rearm();
        }
    }

    /// Coalesce the burst of notifications a single unplug produces, then
    /// re-open the microphone once. Straight from `rearmMicrophone()`.
    fn schedule_rearm(&mut self) {
        if let Some(handle) = self.rearm.take() {
            handle.abort();
        }
        let tx = self.tx.clone();
        let delay = self.deps.timings.rearm_debounce;
        self.rearm = Some(tokio::spawn(async move {
            tokio::time::sleep(delay).await;
            let _ = tx.send(Command::Rearm);
        }));
    }

    fn rearm_microphone(&mut self) {
        if self.machine.is_recording() {
            return;
        }
        if let Err(error) = self.deps.audio.release() {
            tracing::warn!(%error, "could not release the microphone");
        }
        self.needs_stream_rebuild = false;
        self.prewarm_microphone();
    }

    /// Hold the microphone open between dictations, when the user asked for
    /// it. Opt-in because it keeps the OS recording indicator lit.
    fn prewarm_microphone(&mut self) {
        if !self.deps.settings.read().keep_microphone_active {
            return;
        }
        if let Err(error) = self.deps.audio.prewarm() {
            tracing::warn!(%error, "could not pre-warm the microphone");
        }
    }

    // -- Settings --------------------------------------------------------

    fn on_settings_changed(&mut self) {
        self.apply_settings();
        self.schedule_rearm();
    }

    /// Push the current settings out to the backends that cache them. Runs at
    /// launch and on every save, so there is one code path, not two.
    fn apply_settings(&mut self) {
        let settings = self.deps.settings.read().clone();

        if let Err(error) = self.deps.hotkeys.rebind(&settings.hotkeys) {
            tracing::error!(%error, "could not rebind hotkeys");
        }
        self.deps.hotkeys.set_paused(settings.hotkey_paused);

        self.deps.sound.set_enabled(settings.enable_sounds);
        if let Err(error) = self
            .deps
            .sound
            .set_pack(settings.selected_sound_pack.as_deref())
        {
            tracing::warn!(%error, "could not load the selected sound pack");
        }

        if let Err(error) = self
            .deps
            .audio
            .set_device(settings.mic_device_id.as_deref())
        {
            tracing::warn!(%error, "could not select the configured microphone");
        }
    }

    // -- Recovery --------------------------------------------------------

    fn on_recovery(&mut self, recovered: Recovered) {
        tracing::info!(
            packets = recovered.packets.len(),
            "offering a recovered recording"
        );
        *self.deps.pending.lock() = Some(Pending {
            packets: Arc::new(recovered.packets),
            // The app that was focused when this was recorded died with the
            // previous process; claiming to know it would be a lie.
            app: AppInfo {
                name: "Unknown".into(),
                ..AppInfo::default()
            },
            ocr: Vec::new(),
            ax: Vec::new(),
            transcript_id: uuid::Uuid::new_v4().to_string().to_uppercase(),
            spool_path: Some(recovered.path),
        });
        self.deps.ui.set_overlay(OverlayState::Recoverable {
            message: MSG_RECOVERED.into(),
        });
    }

    fn on_overlay_action(&mut self, action: OverlayAction) {
        match action {
            OverlayAction::Retry => {
                if self.deps.pending.lock().is_none() {
                    return;
                }
                self.deps.ui.set_overlay(OverlayState::Processing);
                tokio::spawn(run_transcription(self.deps.clone()));
            }
            OverlayAction::Save => {
                let Some(packets) = self
                    .deps
                    .pending
                    .lock()
                    .as_ref()
                    .map(|pending| pending.packets.clone())
                else {
                    return;
                };
                let dest = self.deps.downloads_dir.join(Spool::export_filename());
                tokio::task::spawn_blocking(move || match Spool::export_wav(&packets, &dest) {
                    Ok(()) => tracing::info!(file = %dest.display(), "exported recording"),
                    Err(error) => tracing::error!(%error, "could not export the recording"),
                });
            }
            OverlayAction::Discard => {
                discard_pending(&self.deps);
                self.deps.ui.set_overlay(OverlayState::Hidden);
            }
            OverlayAction::Cancel => self.cancel_recording(),
        }
    }
}

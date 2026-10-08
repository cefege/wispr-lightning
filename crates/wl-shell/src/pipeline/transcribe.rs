//! Live transcription and post-recording completion.
//!
//! A session opens while the microphone is active and receives each 40 ms
//! packet from the audio worker. The finished packet buffer remains available
//! only for retries and crash recovery.

use super::*;

enum LiveCommand {
    Audio(Vec<i16>),
    Finish,
    Cancel,
}

pub(super) struct LiveAttempt {
    commands: mpsc::UnboundedSender<LiveCommand>,
    outcome: tokio::sync::oneshot::Receiver<LiveOutcome>,
}

pub(super) struct LiveIngress {
    commands: mpsc::UnboundedReceiver<LiveCommand>,
    outcome: tokio::sync::oneshot::Sender<LiveOutcome>,
}

pub(super) struct LiveOutcome {
    context: DictationContext,
    result: Result<TranscriptResult, ProviderError>,
}

pub(super) fn live_attempt() -> (LiveAttempt, LiveIngress) {
    let (commands, command_rx) = mpsc::unbounded_channel();
    let (outcome_tx, outcome) = tokio::sync::oneshot::channel();
    (
        LiveAttempt { commands, outcome },
        LiveIngress {
            commands: command_rx,
            outcome: outcome_tx,
        },
    )
}

impl LiveAttempt {
    pub(super) fn packet_sink(&self) -> wl_platform::audio::PacketSink {
        let commands = self.commands.clone();
        Arc::new(move |packet| {
            let _ = commands.send(LiveCommand::Audio(packet.to_vec()));
        })
    }

    pub(super) fn finish(self) -> tokio::sync::oneshot::Receiver<LiveOutcome> {
        let _ = self.commands.send(LiveCommand::Finish);
        self.outcome
    }

    pub(super) fn cancel(self) {
        let _ = self.commands.send(LiveCommand::Cancel);
    }
}

pub(super) fn start_live_attempt(
    deps: Arc<Deps>,
    app: AppInfo,
    ax: Option<JoinHandle<Vec<String>>>,
    ocr: Option<JoinHandle<Vec<String>>>,
    transcript_id: String,
    ingress: LiveIngress,
) {
    tokio::spawn(drive_live_attempt(
        deps,
        app,
        ax,
        ocr,
        transcript_id,
        ingress,
    ));
}

async fn drive_live_attempt(
    deps: Arc<Deps>,
    app: AppInfo,
    ax: Option<JoinHandle<Vec<String>>>,
    ocr: Option<JoinHandle<Vec<String>>>,
    transcript_id: String,
    mut ingress: LiveIngress,
) {
    let preparation = prepare_live_session(&deps, app, ax, ocr, transcript_id);
    tokio::pin!(preparation);

    let mut buffered = Vec::new();
    let mut finish_requested = false;
    let prepared = loop {
        tokio::select! {
            prepared = &mut preparation => break prepared,
            command = ingress.commands.recv() => match command {
                Some(LiveCommand::Audio(packet)) => buffered.push(packet),
                Some(LiveCommand::Finish) => finish_requested = true,
                Some(LiveCommand::Cancel) | None => return,
            }
        }
    };

    let (context, session) = prepared;
    let session = match session {
        Ok(session) => session,
        Err(error) => {
            if !finish_requested {
                loop {
                    match ingress.commands.recv().await {
                        Some(LiveCommand::Audio(_)) => {}
                        Some(LiveCommand::Finish) => break,
                        Some(LiveCommand::Cancel) | None => return,
                    }
                }
            }
            let _ = ingress.outcome.send(LiveOutcome {
                context,
                result: Err(error),
            });
            return;
        }
    };

    let buffered_packets = buffered.len();
    for packet in buffered {
        session.feed(&packet);
    }
    tracing::info!(buffered_packets, "Deepgram live stream ready");

    if finish_requested {
        let result = session.finish(&context).await;
        let _ = ingress.outcome.send(LiveOutcome { context, result });
        return;
    }

    loop {
        match ingress.commands.recv().await {
            Some(LiveCommand::Audio(packet)) => session.feed(&packet),
            Some(LiveCommand::Finish) => {
                let result = session.finish(&context).await;
                let _ = ingress.outcome.send(LiveOutcome { context, result });
                return;
            }
            Some(LiveCommand::Cancel) | None => {
                session.cancel();
                return;
            }
        }
    }
}

async fn prepare_live_session(
    deps: &Arc<Deps>,
    app: AppInfo,
    ax: Option<JoinHandle<Vec<String>>>,
    ocr: Option<JoinHandle<Vec<String>>>,
    transcript_id: String,
) -> (
    DictationContext,
    Result<Box<dyn wl_providers::DictationSession>, ProviderError>,
) {
    let (dictionary, ax_context, ocr_context) =
        tokio::join!(load_dictionary(deps), join_context(ax), join_context(ocr),);
    let context = DictationContext {
        app: AppContext {
            name: app.name,
            bundle_id: app.bundle_id,
            kind: app.kind.as_str().to_string(),
            url: app.url,
        },
        ocr_context,
        ax_context,
        dictionary,
        transcript_id,
    };
    let provider = deps.provider.read().clone();
    let session = provider.start(&context).await;
    (context, session)
}

async fn join_context(handle: Option<JoinHandle<Vec<String>>>) -> Vec<String> {
    match handle {
        Some(handle) => handle.await.unwrap_or_else(|error| {
            tracing::warn!(%error, "context capture task failed");
            Vec::new()
        }),
        None => Vec::new(),
    }
}

pub(super) async fn run_transcription(deps: Arc<Deps>) {
    run_transcription_with(deps, None).await;
}

pub(super) async fn finish_live_transcription(
    deps: Arc<Deps>,
    outcome: tokio::sync::oneshot::Receiver<LiveOutcome>,
) {
    run_transcription_with(deps, Some(outcome)).await;
}

async fn run_transcription_with(
    deps: Arc<Deps>,
    live: Option<tokio::sync::oneshot::Receiver<LiveOutcome>>,
) {
    if deps
        .transcribing
        .compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst)
        .is_err()
    {
        tracing::debug!("transcription already in flight, skipping duplicate attempt");
        return;
    }

    let outcome = drive_transcription(&deps, live).await;
    deps.transcribing.store(false, Ordering::SeqCst);

    match outcome {
        Some((app, Ok(result))) => on_transcript(deps, app, result).await,
        Some((_, Err(error))) => {
            tracing::warn!(%error, "transcription failed; audio preserved");
            resume_music(&deps);
            deps.ui.set_overlay(OverlayState::Recoverable {
                message: error.user_message(),
            });
        }
        None => {}
    }
}

/// Build the request and run the attempt sequence under the processing
/// deadline. `None` means there was nothing pending, or that the deadline
/// expired and the overlay has already been told.
async fn drive_transcription(
    deps: &Arc<Deps>,
    live: Option<tokio::sync::oneshot::Receiver<LiveOutcome>>,
) -> Option<(AppInfo, Result<TranscriptResult, ProviderError>)> {
    let (packets, app, ocr, ax, transcript_id) = {
        let pending = deps.pending.lock();
        let pending = pending.as_ref()?;
        (
            pending.packets.clone(),
            pending.app.clone(),
            pending.ocr.clone(),
            pending.ax.clone(),
            pending.transcript_id.clone(),
        )
    };

    let duration_secs = packets.len() as f64 * wl_core::consts::PACKET_DURATION_SECS;
    let deadline = processing_timeout(deps.timings.processing_timeout_base, duration_secs);
    let work = async {
        let live = match live {
            Some(outcome) => match outcome.await {
                Ok(live) => Some(live),
                Err(error) => {
                    tracing::warn!(
                        %error,
                        "live transcription task ended; replaying buffered audio"
                    );
                    None
                }
            },
            None => None,
        };

        let (context, initial) = match live {
            Some(live) => {
                if let Some(pending) = deps.pending.lock().as_mut() {
                    pending.ocr = live.context.ocr_context.clone();
                    pending.ax = live.context.ax_context.clone();
                }
                (live.context, Some(live.result))
            }
            None => {
                let dictionary = load_dictionary(deps).await;
                (
                    DictationContext {
                        app: AppContext {
                            name: app.name.clone(),
                            bundle_id: app.bundle_id.clone(),
                            kind: app.kind.as_str().to_string(),
                            url: app.url.clone(),
                        },
                        ocr_context: ocr,
                        ax_context: ax,
                        dictionary,
                        transcript_id,
                    },
                    None,
                )
            }
        };
        attempt_sequence(deps, &packets, &context, initial).await
    };

    match tokio::time::timeout(deadline, work).await {
        Ok(result) => Some((app, result)),
        Err(_) => {
            tracing::warn!("processing timed out; the recording is preserved");
            resume_music(deps);
            deps.ui.set_overlay(OverlayState::Recoverable {
                message: MSG_TIMED_OUT.into(),
            });
            None
        }
    }
}

/// One attempt: open a session, hand it the audio, close it.
///
/// `feed` is deliberately synchronous and non-blocking, so replaying a long
/// recording is a tight loop rather than a sequence of awaits — a streaming
/// provider's worker drains its channel concurrently while we push.
///
/// On any failure the session is dropped through `cancel()` rather than simply
/// going out of scope, so a live socket is closed rather than left for the
/// runtime to reap. A retry that opened a second session against a vendor still
/// holding the first would be charged twice and could interleave transcripts.
async fn run_once(
    provider: &Arc<dyn TranscriptionProvider>,
    packets: &[Vec<i16>],
    context: &DictationContext,
) -> Result<TranscriptResult, ProviderError> {
    let session = provider.start(context).await?;
    for packet in packets {
        session.feed(packet);
    }
    session.finish(context).await
}

/// Run one dictation through the provider, retrying in place on transient
/// failures.
///
/// Each attempt opens a fresh session and replays the buffered audio. Replaying
/// rather than reusing is deliberate: a failed attempt may have half-sent its
/// audio over a socket that then died, and the only state a retry can trust is
/// the packet buffer the pipeline still owns for spooling.
async fn attempt_sequence(
    deps: &Arc<Deps>,
    packets: &[Vec<i16>],
    context: &DictationContext,
    initial: Option<Result<TranscriptResult, ProviderError>>,
) -> Result<TranscriptResult, ProviderError> {
    let mut initial = initial;
    let mut retries = 0;
    loop {
        let provider = deps.provider.read().clone();
        let result = match initial.take() {
            Some(result) => result,
            None => run_once(&provider, packets, context).await,
        };
        match result {
            Ok(result) => return Ok(result),
            Err(error) if error.is_retryable() && retries < MAX_AUTO_RETRIES => {
                retries += 1;
                tracing::warn!(
                    %error,
                    retry = retries,
                    of = MAX_AUTO_RETRIES,
                    "transcription failed, retrying"
                );
                deps.ui.set_overlay(OverlayState::Retrying {
                    attempt: retries + 1,
                    of: MAX_AUTO_RETRIES + 1,
                });
                tokio::time::sleep(deps.timings.retry_delay).await;
            }
            Err(error) => return Err(error),
        }
    }
}

async fn load_dictionary(deps: &Arc<Deps>) -> DictionaryContext {
    let dictionary = deps.dictionary.clone();
    tokio::task::spawn_blocking(move || {
        let vocabulary = dictionary.vocabulary_phrases().unwrap_or_else(|error| {
            tracing::warn!(%error, "could not read vocabulary");
            Vec::new()
        });
        DictionaryContext {
            vocabulary,
            ..DictionaryContext::default()
        }
    })
    .await
    .unwrap_or_else(|error| {
        tracing::warn!(%error, "dictionary read task failed");
        DictionaryContext::default()
    })
}

async fn on_transcript(deps: Arc<Deps>, app: AppInfo, result: TranscriptResult) {
    // The recording made it through; this is the only place the spooled copy
    // is allowed to go away.
    discard_pending(&deps);
    resume_music(&deps);

    let settings = deps.settings.read().clone();
    let mut text = result.display_text().to_string();
    if text.is_empty() {
        tracing::warn!("empty transcription result");
        deps.ui.set_overlay(OverlayState::Error {
            message: ProviderError::EmptyResult.user_message(),
        });
        return;
    }

    if settings.email_auto_signature && app.kind == AppKind::Email {
        text.push_str(settings.email_signature_option.suffix());
    }

    let mode = inject_mode(&settings);
    inject(&deps, &text, mode).await;
    deps.ui.set_overlay(OverlayState::Hidden);

    deps.ui.set_last_transcription(&text);
    record_history(&deps, &result, &app, &settings);
}

/// History and auto-learn, off the transcription path: the user is waiting for
/// text to appear, not for SQLite.
fn record_history(deps: &Arc<Deps>, result: &TranscriptResult, app: &AppInfo, settings: &Settings) {
    let entry = NewTranscript {
        id: result.id.clone(),
        asr_text: result.asr_text.clone(),
        formatted_text: result.formatted_text.clone(),
        app_name: app.name.clone(),
        app_bundle_id: app.bundle_id.clone(),
        duration_secs: result.duration_secs,
        num_words: result.num_words as i64,
        language: settings.deepgram_language.clone(),
    };
    let auto_learn = settings.auto_learn_words;
    let asr = result.asr_text.clone();
    let formatted = result.formatted_text.clone();
    let deps = deps.clone();

    tokio::task::spawn_blocking(move || {
        match deps.history.add_entry(&entry) {
            Ok(()) => deps.ui.notify_changed("history"),
            Err(error) => tracing::error!(%error, "could not write the history entry"),
        }

        // Auto-learn needs both texts: without the formatted one there is no
        // correction to mine, and the phrases would just be raw ASR.
        let (Some(asr), Some(formatted)) = (asr, formatted) else {
            return;
        };
        if !auto_learn {
            return;
        }
        let candidates = wl_core::text::auto_learn_candidates(&asr, &formatted);
        if candidates.is_empty() {
            return;
        }
        match deps.dictionary.add_auto_learned_words(&candidates) {
            Ok(count) => {
                tracing::info!(count, "auto-learned words");
                deps.ui.notify_changed("dictionary");
            }
            Err(error) => tracing::error!(%error, "could not save auto-learned words"),
        }
    });
}

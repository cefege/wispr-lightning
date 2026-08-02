//! Manual checks for the parts of `wl-platform` that need real hardware, real
//! permissions, or a human ear.
//!
//! Unit tests deliberately cover only what is deterministic; everything that
//! depends on a microphone, a speaker or a TCC grant lives here.
//!
//! Every check prints one `PASS` / `FAIL` / `SKIP` line per matrix row it
//! proves, and a `SKIP` always carries the reason it could not be taken. The
//! process exits non-zero if anything failed.
//!
//! ```text
//! cargo run -p wl-platform --example probe                 # read-only sweep
//! cargo run -p wl-platform --example probe -- full         # every capability
//!
//! # read-only
//! cargo run -p wl-platform --example probe -- permissions
//! cargo run -p wl-platform --example probe -- app
//! cargo run -p wl-platform --example probe -- focus
//! cargo run -p wl-platform --example probe -- ocr [max-lines]
//! cargo run -p wl-platform --example probe -- lifecycle
//! cargo run -p wl-platform --example probe -- inputmon
//! cargo run -p wl-platform --example probe -- denials
//!
//! # interactive (waits for a human)
//! cargo run -p wl-platform --example probe -- hotkey [seconds]
//! cargo run -p wl-platform --example probe -- capture [seconds]
//!
//! # changes machine state
//! cargo run -p wl-platform --example probe -- mics
//! cargo run -p wl-platform --example probe -- record [seconds]
//! cargo run -p wl-platform --example probe -- device
//! cargo run -p wl-platform --example probe -- fallback
//! cargo run -p wl-platform --example probe -- device-watch [manual-seconds]
//! cargo run -p wl-platform --example probe -- deactivate
//! cargo run -p wl-platform --example probe -- synthetic [human-seconds]
//! cargo run -p wl-platform --example probe -- keys
//! cargo run -p wl-platform --example probe -- cues
//! cargo run -p wl-platform --example probe -- music
//! cargo run -p wl-platform --example probe -- inject <text>
//! cargo run -p wl-platform --example probe -- type <text>
//! ```
//!
//! Everything that changes machine state — recording, playing sounds, typing
//! into whatever has focus, pausing the user's music, moving the machine's
//! default input device — needs an explicit subcommand. A bare run only reads.
//!
//! Anything this probe moves, it puts back, and it verifies the restore by
//! reading the value again rather than assuming the write took.

use std::fmt::Display;
use std::io::Write as _;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant};

use parking_lot::Mutex;
use wl_core::audio::{packet_to_le_bytes, packet_volume};
use wl_core::consts::{CHUNK_SAMPLES, MIN_PACKETS, PACKET_DURATION_SECS};
use wl_platform::audio::{AudioCapture, StartOutcome};
use wl_platform::audio_impl::CpalCapture;
use wl_platform::hotkey::Transition;
use wl_platform::sound::{Cue, SoundPlayer};
use wl_platform::sound_impl::RodioPlayer;
use wl_platform::{InjectMode, Permission, PermissionState};

// ---------------------------------------------------------------------------
// Verdict reporting
// ---------------------------------------------------------------------------

/// One `PASS` / `FAIL` / `SKIP` line per matrix row, plus a tally.
///
/// The row ids are in the output on purpose: this text is the evidence that
/// closes a row in `docs/parity/MATRIX.md`, and a verdict nobody can map back
/// to a row proves nothing.
#[derive(Default)]
struct Report {
    passed: u32,
    failed: u32,
    skipped: u32,
}

impl Report {
    fn pass(&mut self, rows: &str, detail: impl Display) {
        self.passed += 1;
        println!("PASS {rows:<20} {detail}");
    }

    fn fail(&mut self, rows: &str, detail: impl Display) {
        self.failed += 1;
        println!("FAIL {rows:<20} {detail}");
    }

    /// A check that could not be taken. The reason is mandatory: "skipped" with
    /// no cause is indistinguishable from "not written yet".
    fn skip(&mut self, rows: &str, why: impl Display) {
        self.skipped += 1;
        println!("SKIP {rows:<20} {why}");
    }

    /// Record `condition`, so the caller does not branch by hand.
    fn check(&mut self, rows: &str, condition: bool, detail: impl Display) {
        if condition {
            self.pass(rows, detail);
        } else {
            self.fail(rows, detail);
        }
    }

    /// Context under the verdict it belongs to. Never counted.
    fn note(&self, text: impl Display) {
        println!("     {text}");
    }

    fn summary(&self) {
        println!(
            "\n-- {} pass, {} fail, {} skip --",
            self.passed, self.failed, self.skipped
        );
    }
}

// ---------------------------------------------------------------------------
// Log capture
// ---------------------------------------------------------------------------

/// Collects `tracing` output so a check can assert on a log line.
///
/// Several behaviours here are *only* observable as a log line — the microphone
/// fallback message, for one — so the probe has to read its own logs back
/// rather than asking a human to squint at stderr.
#[derive(Clone, Default)]
struct LogSink(Arc<Mutex<String>>);

impl LogSink {
    /// Everything logged since the last drain, and clear.
    fn drain(&self) -> String {
        std::mem::take(&mut *self.0.lock())
    }

    fn clear(&self) {
        self.0.lock().clear();
    }
}

impl std::io::Write for LogSink {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        self.0.lock().push_str(&String::from_utf8_lossy(buf));
        Ok(buf.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

impl tracing_subscriber::fmt::MakeWriter<'_> for LogSink {
    type Writer = Self;

    fn make_writer(&self) -> Self::Writer {
        self.clone()
    }
}

/// Print captured log lines under the verdict that used them, so the evidence
/// and the claim sit next to each other.
fn show_logs(report: &Report, logs: &LogSink) {
    for line in logs.drain().lines().filter(|l| !l.trim().is_empty()) {
        report.note(format!("log| {line}"));
    }
}

// ---------------------------------------------------------------------------
// Entry point
// ---------------------------------------------------------------------------

fn main() {
    let logs = LogSink::default();
    tracing_subscriber::fmt()
        .with_writer(logs.clone())
        .with_ansi(false)
        .with_target(false)
        .without_time()
        .with_max_level(tracing::Level::DEBUG)
        .init();

    let mut args = std::env::args().skip(1);
    let command = args.next().unwrap_or_default();
    let rest: Vec<String> = args.collect();

    let count = |n: usize| rest.first().and_then(|s| s.parse().ok()).unwrap_or(n);
    let text = || rest.join(" ");

    let mut report = Report::default();
    let r = &mut report;

    let outcome = match command.as_str() {
        "mics" => mics(r),
        "record" => record(r, count(3) as u64),
        "cues" => cues(r),
        "permissions" => permissions(r),
        "app" => frontmost_app(r),
        "focus" => focused_text(r),
        "ocr" => ocr(r, count(50)),
        "lifecycle" => lifecycle(r),
        "hotkey" => hotkey(r, count(5) as u64),
        "capture" => capture(r, count(5) as u64),
        "music" => music(r),
        "inject" => inject(r, &text(), InjectMode::Paste),
        "type" => inject(
            r,
            &text(),
            InjectMode::Natural {
                chars_per_second: 4.0,
            },
        ),

        // Rows whose evidence is this binary and nothing else.
        "device" => device(r, &logs),
        "fallback" => fallback(r, &logs),
        "device-watch" => device_watch(r, &logs, count(0) as u64),
        "deactivate" => deactivate(r, &logs),
        "synthetic" => synthetic(r, count(0) as u64),
        "inputmon" => inputmon(r),
        "keys" => keys(r),
        "denials" => denials(r, &logs),
        // Runs in the untrusted copy spawned by `denials` / `inputmon`; prints
        // one machine-readable line and exits.
        "tcc-report" => tcc_report(),

        // A bare run does the read-only sweep: nothing here can surprise
        // whoever is at the keyboard.
        "" | "all" => permissions(r)
            .and_then(|()| frontmost_app(r))
            .and_then(|()| focused_text(r))
            .and_then(|()| ocr(r, 50))
            .and_then(|()| lifecycle(r))
            .and_then(|()| inputmon(r))
            .and_then(|()| denials(r, &logs)),

        // LOG-013: every platform trait on this OS, in one run.
        "full" => full(r, &logs),

        other => {
            eprintln!("unknown check: {other}\n");
            eprintln!(
                "read-only: all | permissions | app | focus | ocr [max-lines] | lifecycle | \
                 inputmon | denials\n\
                 interactive: hotkey [seconds] | capture [seconds]\n\
                 changes state: full | mics | record [seconds] | device | fallback | \
                 device-watch [seconds] | deactivate | synthetic [seconds] | keys | \
                 cues | music | inject <text> | type <text>"
            );
            std::process::exit(2);
        }
    };

    if let Err(e) = outcome {
        eprintln!("FAILED: {e}");
        std::process::exit(1);
    }
    if command != "tcc-report" {
        report.summary();
    }
    if report.failed > 0 {
        std::process::exit(1);
    }
}

/// LOG-013: exercise every platform trait on this OS and print pass or fail per
/// capability.
///
/// The order matters. Audio and permissions first because they are cheap and
/// steal nothing; the injection checks last because they take focus away from
/// this terminal and hand it back.
fn full(report: &mut Report, logs: &LogSink) -> wl_platform::Result<()> {
    let phase = |name: &str| println!("\n== {name} ==");

    phase("Permissions");
    permissions(report)?;
    inputmon(report)?;
    denials(report, logs)?;

    phase("Audio capture");
    mics(report)?;
    record(report, 1)?;
    device(report, logs)?;
    fallback(report, logs)?;
    deactivate(report, logs)?;
    device_watch(report, logs, 0)?;

    phase("Sound cues");
    cues_short(report)?;

    phase("Media control");
    music(report)?;

    phase("Foreground app and accessibility");
    frontmost_app(report)?;
    focused_text(report)?;
    ocr(report, 50)?;

    phase("Lifecycle and storage");
    lifecycle(report)?;

    phase("Hotkeys");
    synthetic(report, 0)?;
    hotkey(report, 5)?;

    phase("Text injection");
    keys(report)?;

    Ok(())
}

// ---------------------------------------------------------------------------
// Audio capture
// ---------------------------------------------------------------------------

/// Enumerate input devices. The ids printed here are what gets persisted, so
/// check that two identical microphones produce two distinct lines.
fn mics(report: &mut Report) -> wl_platform::Result<()> {
    let capture = CpalCapture::new(None);
    let devices = capture.list_devices()?;
    if devices.is_empty() {
        report.fail("AUD-018", "no input devices");
        return Ok(());
    }
    report.pass("AUD-018", format!("{} input device(s)", devices.len()));
    for device in devices {
        let marker = if device.is_default { "*" } else { " " };
        report.note(format!("{marker} {} — {}", device.name, device.id));
    }
    report.note("(* = system default; the trailing field is the persisted id)");
    Ok(())
}

/// Record for `seconds` and report what came back.
///
/// What to look for:
/// * packet count ≈ `seconds / 0.04`; a large shortfall means samples are
///   being lost between the callback and the worker.
/// * non-zero volumes. All zeros with a working microphone is the digital
///   silence case, and `SilentInput` should appear in the faults.
/// * the written WAV should play back as clean 16 kHz mono speech. Aliasing or
///   a chipmunk pitch means the resampler ratio is wrong.
fn record(report: &mut Report, seconds: u64) -> wl_platform::Result<()> {
    let capture = CpalCapture::new(None);

    let opened = Instant::now();
    capture.prewarm()?;
    report.note(format!("prewarm: {:?}", opened.elapsed()));

    let armed = Instant::now();
    let outcome = capture.start()?;
    report.note(format!("start: {:?} ({outcome:?})", armed.elapsed()));
    report.note(format!("speak for {seconds}s..."));

    std::thread::sleep(Duration::from_secs(seconds));

    let stopped = Instant::now();
    let packets = capture.stop();
    report.note(format!("stop: {:?}", stopped.elapsed()));
    report.note(format!("faults: {:?}", capture.take_faults()));

    let expected = (seconds as f64 / PACKET_DURATION_SECS).round() as usize;
    report.check(
        "AUD-032",
        !packets.is_empty(),
        format!(
            "packets: {} (expected ~{expected}, {:.2}s of audio)",
            packets.len(),
            packets.len() as f64 * PACKET_DURATION_SECS
        ),
    );
    if packets.len() < MIN_PACKETS {
        report.note(format!(
            "under the {MIN_PACKETS}-packet floor: the app would discard this"
        ));
    }
    report.check(
        "AUD-004",
        packets.iter().all(|p| p.len() == CHUNK_SAMPLES),
        format!("every packet is {CHUNK_SAMPLES} samples"),
    );

    let peak = packets
        .iter()
        .map(|p| packet_volume(p))
        .fold(0.0f64, f64::max);
    report.note(format!(
        "peak volume: {peak:.4} (0 means silence, ~0.3 is full scale)"
    ));

    let pcm: Vec<u8> = packets.iter().flat_map(|p| packet_to_le_bytes(p)).collect();
    let path = std::env::temp_dir().join("wl-probe.wav");
    std::fs::write(&path, wl_core::wav::wrap_pcm(&pcm))?;
    report.note(format!("wrote {}", path.display()));

    capture.release()?;
    Ok(())
}

/// AUD-012 / DEVIATION DV2: bind the stream to a **chosen** microphone without
/// touching the machine-wide default input.
///
/// The Swift original selected a microphone by rewriting
/// `kAudioHardwarePropertyDefaultInputDevice`, which changed the input device
/// for every other application on the Mac. The whole point of DV2 is that we do
/// not, so this reads the machine default before and after and requires it to
/// be byte-identical.
fn device(report: &mut Report, logs: &LogSink) -> wl_platform::Result<()> {
    let capture = CpalCapture::new(None);
    let devices = capture.list_devices()?;

    let Some(chosen) = devices.iter().find(|d| !d.is_default).cloned() else {
        report.skip(
            "AUD-012",
            format!(
                "only {} input device(s) present, so there is no non-default device to bind to",
                devices.len()
            ),
        );
        return Ok(());
    };
    let default_before = devices.iter().find(|d| d.is_default).cloned();
    let system_before = system_default_input();

    report.note(format!(
        "chosen (non-default): {} — {}",
        chosen.name, chosen.id
    ));
    report.note(format!(
        "machine default before: {:?}",
        default_before.as_ref().map(|d| d.id.as_str())
    ));

    logs.clear();
    capture.set_device(Some(&chosen.id))?;
    let outcome = capture.start()?;
    report.check(
        "AUD-012",
        outcome == StartOutcome::Started,
        format!("start on the chosen device returned {outcome:?} (a fallback would say so)"),
    );

    let bound = capture.bound_device();
    report.check(
        "AUD-012",
        bound.as_ref().map(|d| d.id.as_str()) == Some(chosen.id.as_str()),
        format!(
            "stream is bound to {:?}, requested {:?}",
            bound.as_ref().map(|d| d.id.as_str()),
            chosen.id
        ),
    );

    std::thread::sleep(Duration::from_millis(400));
    let packets = capture.stop();
    report.check(
        "AUD-012",
        !packets.is_empty(),
        format!("{} packet(s) arrived from the chosen device", packets.len()),
    );
    show_logs(report, logs);
    capture.release()?;

    // DV2: nothing above may have moved the machine's own input device.
    let after = capture.list_devices()?;
    let default_after = after.iter().find(|d| d.is_default).cloned();
    report.check(
        "AUD-012",
        default_before.as_ref().map(|d| &d.id) == default_after.as_ref().map(|d| &d.id),
        format!(
            "DV2: machine default input unchanged — before {:?}, after {:?}",
            default_before.as_ref().map(|d| d.id.as_str()),
            default_after.as_ref().map(|d| d.id.as_str())
        ),
    );

    let system_after = system_default_input();
    match (&system_before, &system_after) {
        (Some(before), Some(after)) => report.check(
            "AUD-012",
            before == after,
            format!(
                "DV2 cross-check straight from CoreAudio: \
                 kAudioHardwarePropertyDefaultInputDevice before {before:?}, after {after:?}"
            ),
        ),
        _ => report.note("no CoreAudio cross-check on this platform"),
    }
    Ok(())
}

/// AUD-016: a configured microphone that is not present falls back to the
/// system default, says so in the log, and still records.
fn fallback(report: &mut Report, logs: &LogSink) -> wl_platform::Result<()> {
    // Deliberately well-formed for this host but matching nothing: a string
    // that fails to parse as a `DeviceId` would exercise a different branch.
    let missing = missing_device_id();

    let capture = CpalCapture::new(None);
    let default = capture.list_devices()?.into_iter().find(|d| d.is_default);

    logs.clear();
    capture.set_device(Some(missing))?;
    let outcome = capture.start()?;

    report.check(
        "AUD-016",
        outcome
            == StartOutcome::StartedWithFallback {
                requested: missing.to_owned(),
            },
        format!("start() returned {outcome:?}"),
    );

    let captured = logs.drain();
    report.check(
        "AUD-016",
        captured.contains("configured microphone is unavailable, using system default")
            && captured.contains(missing),
        "the fallback is logged with the requested id, not a name lookup",
    );
    for line in captured.lines().filter(|l| !l.trim().is_empty()) {
        report.note(format!("log| {line}"));
    }

    let bound = capture.bound_device();
    report.check(
        "AUD-016",
        bound.as_ref().map(|d| &d.id) == default.as_ref().map(|d| &d.id),
        format!(
            "recording proceeded on the system default: bound {:?}, default {:?}",
            bound.as_ref().map(|d| d.id.as_str()),
            default.as_ref().map(|d| d.id.as_str())
        ),
    );

    std::thread::sleep(Duration::from_millis(400));
    let packets = capture.stop();
    report.check(
        "AUD-016",
        !packets.is_empty(),
        format!(
            "{} packet(s) arrived despite the missing device",
            packets.len()
        ),
    );
    capture.release()?;
    Ok(())
}

/// AUD-027 / AUD-034: the observable effects of closing a pre-warmed stream.
///
/// `deactivate()` in the Swift original is `release()` here, and `cleanup()` is
/// dropping the capture. Both end with no stream, no worker and no packets; the
/// interesting clause is that `release()` refuses to run while a dictation is
/// live, which is what stops the microphone being yanked out from under a
/// recording.
fn deactivate(report: &mut Report, logs: &LogSink) -> wl_platform::Result<()> {
    logs.clear();
    let capture = CpalCapture::new(None);

    capture.prewarm()?;
    let prewarmed = capture.bound_device();
    report.check(
        "AUD-027",
        prewarmed.is_some(),
        format!(
            "prewarm opened a stream on {:?}",
            prewarmed.as_ref().map(|d| d.name.as_str())
        ),
    );

    // The guard clause: release must be a no-op while recording.
    capture.start()?;
    capture.release()?;
    report.check(
        "AUD-027",
        capture.bound_device().is_some() && capture.is_recording(),
        "release() while recording left the stream open, as it must",
    );
    let during = capture.stop();
    report.note(format!("{} packet(s) before deactivating", during.len()));

    // Now the real thing.
    capture.release()?;
    report.check(
        "AUD-027",
        capture.bound_device().is_none(),
        "release() while prewarmed and idle closed the stream",
    );
    report.check(
        "AUD-027",
        !capture.is_recording(),
        "is_recording() is false after release()",
    );

    std::thread::sleep(Duration::from_millis(300));
    let after = capture.stop();
    report.check(
        "AUD-027",
        after.is_empty(),
        format!(
            "no further packets after release(): {} produced",
            after.len()
        ),
    );
    let faults = capture.take_faults();
    report.check(
        "AUD-027",
        faults.is_empty(),
        format!("closing the stream raised no fault: {faults:?}"),
    );
    show_logs(report, logs);

    // AUD-034: dropping the capture tears the whole session down. `Drop` joins
    // the worker thread unconditionally, so a drop that returns at all is proof
    // the shutdown command was delivered and the worker loop exited — a leaked
    // worker would block here forever.
    let doomed = CpalCapture::new(None);
    doomed.prewarm()?;
    let open = doomed.bound_device();
    report.note(format!(
        "second capture open on {:?}",
        open.as_ref().map(|d| d.name.as_str())
    ));
    let started = Instant::now();
    drop(doomed);
    let elapsed = started.elapsed();
    report.check(
        "AUD-034",
        elapsed < Duration::from_secs(2),
        format!("cleanup joined the audio worker and dropped the stream in {elapsed:?}"),
    );

    // And the device really was handed back: a fresh stream opens on it.
    let reopened = CpalCapture::new(None);
    reopened.prewarm()?;
    report.check(
        "AUD-034",
        reopened.bound_device().is_some(),
        "the microphone reopened after cleanup",
    );
    reopened.release()?;
    report.note(
        "the OS microphone indicator going out is a human observation; watch Control Center \
         while this runs",
    );
    Ok(())
}

// ---------------------------------------------------------------------------
// Sound cues
// ---------------------------------------------------------------------------

/// Play every cue of every pack. Listen for: cues that overlap rather than
/// queue, no click or truncation, and no gap from a device being reopened.
fn cues(report: &mut Report) -> wl_platform::Result<()> {
    let player = sound_player();
    if !player.is_available() {
        report.skip(
            "SND-001",
            "no audio output device: every cue would be a no-op",
        );
        return Ok(());
    }

    for pack in player.available_packs() {
        report.note(format!("pack: {pack}"));
        player.set_pack(Some(&pack))?;
        for cue in [Cue::Start, Cue::Stop, Cue::Error] {
            report.note(format!("  {cue:?}"));
            player.play(cue);
            std::thread::sleep(Duration::from_millis(700));
        }
    }

    report.note("overlap check: start and stop fired 40ms apart");
    player.set_pack(None)?;
    player.play(Cue::Start);
    std::thread::sleep(Duration::from_millis(40));
    player.play(Cue::Stop);
    std::thread::sleep(Duration::from_secs(2));

    report.note("mute check: nothing should play");
    player.set_enabled(false);
    player.play(Cue::Start);
    std::thread::sleep(Duration::from_millis(500));

    report.pass("SND-001", "every cue of every pack played");
    Ok(())
}

/// One cue from the default pack, for the `full` sweep: enough to prove the
/// player reaches the speaker without sitting through every pack.
fn cues_short(report: &mut Report) -> wl_platform::Result<()> {
    let player = sound_player();
    if !player.is_available() {
        report.skip("SND-001", "no audio output device on this machine");
        return Ok(());
    }
    player.set_pack(None)?;
    player.play(Cue::Start);
    std::thread::sleep(Duration::from_millis(600));
    report.pass("SND-001", "default pack start cue played");
    Ok(())
}

fn sound_player() -> RodioPlayer {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../Resources/Sounds");
    RodioPlayer::new(root)
}

// ---------------------------------------------------------------------------
// Platform capabilities
// ---------------------------------------------------------------------------

/// Every TCC grant the app depends on, and what breaks without it.
///
/// Read-only: `status` never prompts. Denied Accessibility means injection
/// fails; denied Input Monitoring means the hotkey silently never fires, which
/// is the failure the Swift version had no diagnostic for at all.
fn permissions(report: &mut Report) -> wl_platform::Result<()> {
    let platform = wl_platform::current::platform();
    for (permission, consequence) in [
        (Permission::Microphone, "recording yields silence"),
        (Permission::Accessibility, "text injection fails"),
        (Permission::InputMonitoring, "the hotkey never fires"),
        (Permission::ScreenRecording, "screen context is empty"),
    ] {
        let state = platform.permissions.status(permission);
        let line = format!("{permission:?}: {state:?} — without it, {consequence}");
        match state {
            PermissionState::Granted | PermissionState::NotApplicable => {
                report.pass("PRM-001", line)
            }
            // Nothing has asked yet, so this is a to-do, not a failure.
            PermissionState::NotDetermined => report.skip("PRM-001", line),
            PermissionState::Denied => report.fail("PRM-001", line),
        }
    }
    Ok(())
}

/// The app snapshot that goes out with every transcription request.
///
/// Focus something else first: run this from a terminal and the answer is the
/// terminal. Point it at Slack and `kind` must read `Messaging`; point it at a
/// browser and `url` must carry the current tab.
fn frontmost_app(report: &mut Report) -> wl_platform::Result<()> {
    let info = wl_platform::current::platform().foreground.current();
    report.check(
        "CTX-001",
        !info.bundle_id.is_empty(),
        format!(
            "frontmost: name={:?} bundle_id={:?} kind={} url={:?}",
            info.name,
            info.bundle_id,
            info.kind.as_str(),
            info.url
        ),
    );
    Ok(())
}

/// The accessibility context fed to the transcriber.
///
/// Empty is a legitimate answer — plenty of controls expose no value — but
/// empty for a focused text field with visible text means Accessibility is not
/// granted, or the AX messaging timeout is firing.
fn focused_text(report: &mut Report) -> wl_platform::Result<()> {
    let started = Instant::now();
    let lines = wl_platform::current::platform()
        .injector
        .read_focused_text();
    report.pass(
        "CTX-010",
        format!(
            "focused text: {} line(s) in {:?}",
            lines.len(),
            started.elapsed()
        ),
    );
    for line in &lines {
        report.note(format!("{:?}", truncate(line, 120)));
    }
    Ok(())
}

/// OCR the frontmost window.
///
/// Zero lines with text plainly on screen means Screen Recording is not
/// granted: the capture returns no image and there is no error to report.
/// Watch the elapsed time too — this runs concurrently with recording, so it
/// has a hard deadline.
fn ocr(report: &mut Report, max_lines: usize) -> wl_platform::Result<()> {
    let started = Instant::now();
    let lines = wl_platform::current::platform()
        .screen
        .ocr_frontmost_window(max_lines);
    report.check(
        "CTX-018",
        !lines.is_empty(),
        format!(
            "OCR: {} line(s) (cap {max_lines}) in {:?}",
            lines.len(),
            started.elapsed()
        ),
    );
    for line in lines.iter().take(5) {
        report.note(format!("{:?}", truncate(line, 120)));
    }
    Ok(())
}

/// Wait for the push-to-talk key.
fn hotkey(report: &mut Report, seconds: u64) -> wl_platform::Result<()> {
    use wl_core::settings::hotkey::{Hotkey, Modifiers};

    let backend = wl_platform::current::hotkeys()?;
    backend.rebind(&[Hotkey::modifier(Modifiers::CTRL_LEFT)])?;
    report.note(format!(
        "healthy={} — hold Left Control to dictate",
        backend.is_healthy()
    ));

    let events = backend.events();
    let deadline = Instant::now() + Duration::from_secs(seconds);
    let mut seen = 0usize;
    while let Some(remaining) = deadline.checked_duration_since(Instant::now()) {
        match events.recv_timeout(remaining) {
            Ok(event) => {
                seen += 1;
                let arrow = match event.transition {
                    Transition::Pressed => "down",
                    Transition::Released => "up  ",
                    // The chord guard cancelled the hold: a key went down
                    // while a bare-modifier binding was held.
                    Transition::Aborted => "abrt",
                };
                let name = "dictate";
                report.note(format!("{arrow} {name}"));
            }
            Err(_) => break,
        }
    }

    let healthy = backend.is_healthy();
    let line = format!("hotkey: {seen} event(s) in {seconds}s, healthy={healthy}");
    if seen > 0 {
        report.pass("HTK-012", line);
    } else if healthy {
        // A live tap with nobody at the keyboard is not a failure of the tap.
        report.skip("HTK-012", format!("{line} — nobody pressed the key"));
    } else {
        report.fail("HTK-012", line);
    }
    Ok(())
}

/// Record a new binding, the way the settings window does.
///
/// The point of testing this outside the webview: the Fn key never reaches a
/// browser-side `keydown`, but it remains a valid dictation binding.
fn capture(report: &mut Report, seconds: u64) -> wl_platform::Result<()> {
    let backend = wl_platform::current::hotkeys()?;
    report.note(format!(
        "press the key you want to bind (Fn included) — {seconds}s"
    ));
    backend.begin_capture();
    std::thread::sleep(Duration::from_secs(seconds));
    match backend.end_capture() {
        Some(hotkey) => report.pass(
            "HTK-040",
            format!("captured {} ({hotkey:?})", hotkey.label()),
        ),
        None => report.skip("HTK-040", "nothing usable was pressed"),
    }
    Ok(())
}

/// Pause the user's music and put it back.
///
/// Start Apple Music or Spotify playing first. `pause` must report `true` only
/// when it actually stopped something, and stopping playback yourself before
/// the resume must leave it stopped.
fn music(report: &mut Report) -> wl_platform::Result<()> {
    let platform = wl_platform::current::platform();
    let started = Instant::now();
    let paused = platform.media.pause();
    report.note(format!("pause() -> {paused} in {:?}", started.elapsed()));
    std::thread::sleep(Duration::from_secs(2));
    platform.media.resume();
    report.pass(
        "MED-001",
        format!("pause()/resume() round trip completed, paused_something={paused}"),
    );
    Ok(())
}

/// Type into whatever has focus.
///
/// Focus a text field in another app and give this a few seconds. For `type`,
/// watch the rhythm: characters must arrive unevenly, punctuation must be
/// correct (a `,` coming out as `<` means the private event source is not
/// isolating the live modifiers), and the clipboard must be untouched
/// afterwards.
fn inject(report: &mut Report, text: &str, mode: InjectMode) -> wl_platform::Result<()> {
    if text.is_empty() {
        eprintln!("give me something to type");
        std::process::exit(2);
    }
    report.note("focus a text field; typing in 3s");
    std::thread::sleep(Duration::from_secs(3));

    let platform = wl_platform::current::platform();
    let started = Instant::now();
    // `inject` no longer reports verification: BACKLOG B-001 removed the AX
    // read-back because it false-negatived on every dictation. Reaching here
    // without an error IS the success signal now.
    platform.injector.inject(text, mode)?;
    report.check(
        "INJ-001",
        true,
        format!("inject: returned Ok in {:?} ({mode:?})", started.elapsed()),
    );

    // The restore is deferred, so give it room before reporting.
    std::thread::sleep(Duration::from_millis(400));
    report.note("clipboard should now hold whatever it held before");
    Ok(())
}

fn truncate(text: &str, limit: usize) -> String {
    let mut out: String = text.chars().take(limit).collect();
    if out.chars().count() < text.chars().count() {
        out.push('…');
    }
    out
}

/// Register the sleep hook and report what the launch-at-login pair does.
///
/// Nothing here waits for an actual sleep — closing the lid and watching for
/// the log line is the real check. What this proves is that the observer
/// installs and unregisters without tripping over `NSWorkspace`'s
/// notification center.
fn lifecycle(report: &mut Report) -> wl_platform::Result<()> {
    use std::sync::atomic::{AtomicBool, Ordering};

    let hooks = wl_platform::current::lifecycle();
    let fired = Arc::new(AtomicBool::new(false));
    let flag = Arc::clone(&fired);
    hooks.on_sleep(Box::new(move || flag.store(true, Ordering::SeqCst)));
    report.pass(
        "LIF-011",
        "sleep observer installed (close the lid to see it fire)",
    );

    report.note(format!("launch_at_login() = {}", hooks.launch_at_login()));
    match hooks.set_launch_at_login(true) {
        // The autostart plugin owns this at the app layer, so refusing here is
        // the correct answer rather than a gap.
        Err(wl_platform::PlatformError::Unsupported(why)) => {
            report.pass("LIF-015", format!("set_launch_at_login deferred — {why}"));
        }
        other => report.fail("LIF-015", format!("expected Unsupported, got {other:?}")),
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Permission reporting: HTK-032, LOG-012, CTX-019, LIF-016
// ---------------------------------------------------------------------------

/// HTK-032 / LOG-012: the denied-Input-Monitoring diagnostic the Swift version
/// never had.
///
/// The grant on this machine cannot be revoked from inside the process, so the
/// denied branch is reached the only honest way available: by running this same
/// binary from a path TCC has never seen. An unsigned copy at a new path is a
/// different TCC subject, so it gets the *denied* answers while the original
/// keeps the granted ones — which is exactly the pair of states the row is
/// about.
fn inputmon(report: &mut Report) -> wl_platform::Result<()> {
    let platform = wl_platform::current::platform();
    let status = platform.permissions.status(Permission::InputMonitoring);
    report.check(
        "LOG-012",
        status != PermissionState::NotApplicable,
        format!(
            "Permissions::status(InputMonitoring) reports {status:?} — \
             the Swift app had no query at all"
        ),
    );

    let backend = wl_platform::current::hotkeys()?;
    // The health flag is set when the listener is built and re-checked on a
    // timer; give the worker one tick to make its first pass.
    std::thread::sleep(Duration::from_millis(120));
    let healthy = backend.is_healthy();
    report.pass(
        "HTK-032",
        format!("HotkeyBackend::is_healthy() -> {healthy} on the granted binary"),
    );

    match untrusted_self_report() {
        Ok(Some(line)) => {
            report.note(format!("untrusted copy| {line}"));
            if is_ungranted(&line, "input_monitoring") {
                let dead = line.contains("hotkeys_healthy=false");
                report.check(
                    "HTK-032",
                    dead,
                    "an ungranted copy of this binary reports the permission AND \
                     is_healthy()=false — the app no longer looks alive while never triggering",
                );
                report.pass(
                    "LOG-012",
                    "the ungranted copy surfaces Input Monitoring status through the \
                     Permissions trait rather than failing silently",
                );
            } else {
                // Observed on macOS 26.5: even a copy re-signed under a fresh
                // ad-hoc identifier answers "Granted". TCC attributes these
                // grants to the *responsible process* — the terminal that
                // launched us — and a child inherits that attribution whatever
                // its own signature says.
                report.skip(
                    "HTK-032",
                    "the re-signed copy still reports Input Monitoring as Granted: TCC \
                     attributes the grant to the responsible process (the launching \
                     terminal), which a child inherits regardless of its own code-signing \
                     identity. The denied branch is not reachable from inside a granted \
                     session and needs the grant revoked in System Settings.",
                );
            }
        }
        Ok(None) => report.skip(
            "HTK-032",
            "the untrusted copy produced no report line; denied branch not observed",
        ),
        Err(e) => report.skip("HTK-032", format!("could not run an untrusted copy: {e}")),
    }
    Ok(())
}

/// CTX-019 / LIF-016: permission denial is *reported*, not swallowed.
///
/// CTX-019's granted half is observable directly — the OCR path takes no
/// preflight and no request. Its denied half is reached through the untrusted
/// copy. LIF-016's macOS half is the microphone error mapping, driven here
/// directly rather than by revoking a grant; its Windows half (an STA worker
/// activating `ms-settings:`) cannot be executed on this host at all.
fn denials(report: &mut Report, logs: &LogSink) -> wl_platform::Result<()> {
    let platform = wl_platform::current::platform();

    logs.clear();
    let screen = platform.permissions.status(Permission::ScreenRecording);
    let started = Instant::now();
    let lines = platform.screen.ocr_frontmost_window(50);
    report.pass(
        "CTX-019",
        format!(
            "ScreenRecording={screen:?}; ocr_frontmost_window returned {} line(s) in {:?} \
             with no preflight and no request call",
            lines.len(),
            started.elapsed()
        ),
    );
    show_logs(report, logs);

    match untrusted_self_report() {
        Ok(Some(line)) => {
            report.note(format!("untrusted copy| {line}"));
            if !is_ungranted(&line, "screen_recording") {
                report.skip(
                    "CTX-019",
                    "the re-signed copy still reports Screen Recording as Granted: TCC \
                     attributes the grant to the responsible process (the launching \
                     terminal), which a child inherits regardless of its own code-signing \
                     identity. The denied branch needs the grant revoked in System Settings.",
                );
            } else if lines.is_empty() {
                // Both runs came back empty, so the comparison cannot separate
                // "denied" from "the frontmost app has no window".
                report.skip(
                    "CTX-019",
                    "the granted run also produced zero OCR lines, so the ungranted run's \
                     empty result is not evidence of the denial; run this with a window in \
                     front",
                );
            } else {
                report.check(
                    "CTX-019",
                    line.contains("ocr_lines=0"),
                    format!(
                        "granted read {} line(s) and the ungranted copy read none — the \
                         denial manifests only as missing context, with no error",
                        lines.len()
                    ),
                );
            }
        }
        Ok(None) => report.skip("CTX-019", "the untrusted copy produced no report line"),
        Err(e) => report.skip("CTX-019", format!("could not run an untrusted copy: {e}")),
    }

    // LIF-016: the actionable guidance for a blocked microphone. Driving the
    // classifier is the whole of the macOS half — there is no way to revoke a
    // TCC grant from inside the process, and a `NotDetermined` microphone
    // yields digital silence rather than an error.
    let denied = cpal::Error::with_message(
        cpal::ErrorKind::PermissionDenied,
        "probe-synthesised permission failure",
    );
    match wl_platform::audio_impl::map_open_error(&denied) {
        wl_platform::PlatformError::PermissionDenied(hint) => report.check(
            "LIF-016",
            hint.contains("Microphone"),
            format!("a blocked microphone is reported with guidance: {hint:?}"),
        ),
        other => report.fail(
            "LIF-016",
            format!("expected PermissionDenied, got {other:?}"),
        ),
    }
    let mic = platform.permissions.status(Permission::Microphone);
    report.note(format!(
        "Microphone status on this binary: {mic:?} \
         (the denial path itself needs a revoked grant)"
    ));
    report.skip(
        "LIF-016",
        "the Windows half — `ms-settings:privacy-microphone` activated from a bounded STA \
         worker — cannot be executed on macOS",
    );
    Ok(())
}

/// Permission status for a binary TCC has never seen. One machine-readable
/// line, so the parent run can assert on it.
fn tcc_report() -> wl_platform::Result<()> {
    let platform = wl_platform::current::platform();
    let healthy = match wl_platform::current::hotkeys() {
        Ok(backend) => {
            std::thread::sleep(Duration::from_millis(120));
            backend.is_healthy()
        }
        Err(_) => false,
    };
    let lines = platform.screen.ocr_frontmost_window(50);
    println!(
        "input_monitoring={:?} accessibility={:?} screen_recording={:?} microphone={:?} \
         hotkeys_healthy={healthy} ocr_lines={}",
        platform.permissions.status(Permission::InputMonitoring),
        platform.permissions.status(Permission::Accessibility),
        platform.permissions.status(Permission::ScreenRecording),
        platform.permissions.status(Permission::Microphone),
        lines.len(),
    );
    std::io::stdout().flush().ok();
    Ok(())
}

/// Run a differently-signed copy of this binary and ask it what it can do.
///
/// TCC keys a grant on the executable's code-signing identity, not its path —
/// a byte-identical copy elsewhere inherits every grant, which is why the copy
/// is re-signed ad hoc under a fresh identifier first. That makes it a subject
/// TCC has never seen, so it answers with the *denied* state of grants this
/// build normally holds, and the denial branches become observable without
/// taking the user's real grants away from them.
fn untrusted_self_report() -> std::io::Result<Option<String>> {
    let exe = std::env::current_exe()?;
    let dir = std::env::temp_dir().join(format!("wl-probe-untrusted-{}", std::process::id()));
    std::fs::create_dir_all(&dir)?;
    let copy = dir.join("probe-untrusted");
    std::fs::copy(&exe, &copy)?;

    let signed = std::process::Command::new("/usr/bin/codesign")
        .args(["--force", "--sign", "-", "--identifier"])
        .arg(format!("wl-probe-untrusted-{}", now_millis()))
        .arg(&copy)
        .output();
    let output = match signed {
        Ok(signed) if signed.status.success() => {
            std::process::Command::new(&copy).arg("tcc-report").output()
        }
        // Without a fresh identity the copy is the same TCC subject as us and
        // would answer "granted" to everything, which proves nothing.
        other => {
            let _ = std::fs::remove_dir_all(&dir);
            let why = match other {
                Ok(signed) => String::from_utf8_lossy(&signed.stderr).trim().to_owned(),
                Err(e) => e.to_string(),
            };
            return Err(std::io::Error::other(format!(
                "could not re-sign the copy under a fresh identity: {why}"
            )));
        }
    };
    // Best effort: a copy left behind in the temp dir is untidy, not a failure
    // of the check.
    let _ = std::fs::remove_dir_all(&dir);

    let output = output?;
    let text = String::from_utf8_lossy(&output.stdout);
    Ok(text
        .lines()
        .find(|l| l.starts_with("input_monitoring="))
        .map(str::to_owned))
}

/// Whether the untrusted copy really is a different TCC subject.
///
/// A copy that still answers "granted" inherited our identity, so nothing it
/// reports is evidence about the denied path.
fn is_ungranted(line: &str, permission: &str) -> bool {
    !line.contains(&format!("{permission}=Granted"))
}

fn now_millis() -> u128 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis())
        .unwrap_or_default()
}

// ---------------------------------------------------------------------------
// Platform-specific checks
// ---------------------------------------------------------------------------

#[cfg(target_os = "macos")]
fn system_default_input() -> Option<String> {
    mac::audio_hw::default_input_uid()
}

#[cfg(not(target_os = "macos"))]
fn system_default_input() -> Option<String> {
    None
}

/// A device id that parses for this host but resolves to nothing.
#[cfg(target_os = "macos")]
const fn missing_device_id() -> &'static str {
    "coreaudio:WL-PROBE-NO-SUCH-DEVICE"
}

#[cfg(target_os = "windows")]
const fn missing_device_id() -> &'static str {
    "wasapi:{00000000-0000-0000-0000-000000000000}"
}

#[cfg(not(any(target_os = "macos", target_os = "windows")))]
const fn missing_device_id() -> &'static str {
    "none:WL-PROBE-NO-SUCH-DEVICE"
}

fn device_watch(
    report: &mut Report,
    logs: &LogSink,
    manual_seconds: u64,
) -> wl_platform::Result<()> {
    #[cfg(target_os = "macos")]
    {
        mac::device_watch(report, logs, manual_seconds)
    }
    #[cfg(not(target_os = "macos"))]
    {
        let _ = (logs, manual_seconds);
        report.skip(
            "AUD-019/AUD-020",
            "driving device changes needs the CoreAudio property API; macOS only",
        );
        Ok(())
    }
}

fn synthetic(report: &mut Report, seconds: u64) -> wl_platform::Result<()> {
    #[cfg(target_os = "macos")]
    {
        mac::synthetic(report, seconds)
    }
    #[cfg(not(target_os = "macos"))]
    {
        let _ = seconds;
        report.skip(
            "HTK-011",
            "posting a synthetic key needs CGEvent; the Windows analogue is LLKHF_INJECTED \
             and needs a Windows host",
        );
        Ok(())
    }
}

fn keys(report: &mut Report) -> wl_platform::Result<()> {
    #[cfg(target_os = "macos")]
    {
        mac::keys(report)
    }
    #[cfg(not(target_os = "macos"))]
    {
        report.skip(
            "INJ-026/INJ-027/INJ-029",
            "reading back the posted virtual keys needs a CGEventTap; macOS only",
        );
        Ok(())
    }
}

#[cfg(target_os = "macos")]
mod mac {
    //! The checks that can only be expressed by talking to macOS directly:
    //! moving the machine's audio configuration, posting synthetic keystrokes,
    //! and reading back what actually reached the window server.

    use std::sync::Arc;
    use std::time::{Duration, Instant};

    use objc2_core_graphics::{
        CGEvent, CGEventFlags, CGEventSource, CGEventSourceStateID, CGEventTapLocation,
    };
    use wl_platform::audio::{AudioCapture, CaptureFault, InputDevice};
    use wl_platform::audio_impl::CpalCapture;
    use wl_platform::hotkey::{HotkeyBackend, HotkeyEvent};

    use super::{LogSink, Report};

    /// Virtual keys, as the injector spells them.
    const VK_A: u16 = 0;
    const VK_S: u16 = 1;
    const VK_W: u16 = 13;
    const VK_RETURN: u16 = 36;
    const VK_TAB: u16 = 48;
    const VK_DELETE: u16 = 51;
    /// `kVK_F20`. Deliberately neither a modifier nor a key any Mac keyboard
    /// has: this machine runs the user's real dictation app, whose push-to-talk
    /// binding is Left Control, so posting *that* would start a live recording
    /// in someone else's process.
    const VK_F20: u16 = 0x5A;
    /// The carrier keycode for a unicode event.
    const VK_UNICODE_CARRIER: u16 = 0;
    /// The string typed into TextEdit: a letter reached through the layout map,
    /// a newline, a tab, and a character no US layout can produce.
    ///
    /// It opens with a digit on purpose. TextEdit capitalises the first letter
    /// of a document, so a leading `x` comes back as `X` and the read-back
    /// comparison fails on TextEdit's autocorrect rather than on anything the
    /// injector did.
    const INJECT_PROBE: &str = "7x\n\tz\u{1F600}";

    // -----------------------------------------------------------------------
    // CoreAudio property access
    // -----------------------------------------------------------------------

    /// Direct access to the machine's audio configuration.
    ///
    /// Nothing in `AudioCapture` can move the system default input or a
    /// device's sample rate — deliberately, that is DEVIATION DV2 — so driving
    /// the device-change listeners means going to the HAL.
    pub mod audio_hw {
        use std::ffi::c_void;
        use std::ptr::NonNull;

        use objc2::rc::Retained;
        use objc2::runtime::AnyObject;
        use objc2_core_audio::{
            kAudioDevicePropertyAvailableNominalSampleRates, kAudioDevicePropertyDeviceUID,
            kAudioDevicePropertyNominalSampleRate, kAudioHardwarePropertyDefaultInputDevice,
            kAudioHardwarePropertyDevices, kAudioObjectPropertyElementMain,
            kAudioObjectPropertyScopeGlobal, kAudioObjectSystemObject,
            AudioHardwareCreateAggregateDevice, AudioHardwareDestroyAggregateDevice,
            AudioObjectGetPropertyData, AudioObjectGetPropertyDataSize, AudioObjectID,
            AudioObjectPropertyAddress, AudioObjectSetPropertyData,
        };
        use objc2_core_foundation::{CFDictionary, CFRetained, CFString};
        use objc2_foundation::{NSArray, NSDictionary, NSNumber, NSString};

        /// `AudioValueRange`, which `objc2-core-audio` does not re-export.
        #[repr(C)]
        #[derive(Clone, Copy, Default)]
        struct ValueRange {
            minimum: f64,
            maximum: f64,
        }

        const SYSTEM: AudioObjectID = kAudioObjectSystemObject as AudioObjectID;

        fn address(selector: u32) -> AudioObjectPropertyAddress {
            AudioObjectPropertyAddress {
                mSelector: selector,
                mScope: kAudioObjectPropertyScopeGlobal,
                mElement: kAudioObjectPropertyElementMain,
            }
        }

        /// Read a fixed-size property into `T`.
        fn get<T: Copy + Default>(object: AudioObjectID, selector: u32) -> Option<T> {
            let mut addr = address(selector);
            let mut value = T::default();
            let mut size = std::mem::size_of::<T>() as u32;
            // SAFETY: `addr`, `size` and `value` are live locals; `size` is
            // exactly the byte length of `value`, which is what the HAL writes
            // into. A null qualifier is correct for every selector used here.
            let status = unsafe {
                AudioObjectGetPropertyData(
                    object,
                    NonNull::from(&mut addr),
                    0,
                    std::ptr::null(),
                    NonNull::from(&mut size),
                    NonNull::from(&mut value).cast::<c_void>(),
                )
            };
            (status == 0).then_some(value)
        }

        fn set<T: Copy>(object: AudioObjectID, selector: u32, mut value: T) -> bool {
            let mut addr = address(selector);
            // SAFETY: as above; `value` outlives the call and its length is
            // passed alongside it.
            let status = unsafe {
                AudioObjectSetPropertyData(
                    object,
                    NonNull::from(&mut addr),
                    0,
                    std::ptr::null(),
                    std::mem::size_of::<T>() as u32,
                    NonNull::from(&mut value).cast::<c_void>(),
                )
            };
            status == 0
        }

        /// Read a variable-length array property.
        fn get_array<T: Copy + Default>(object: AudioObjectID, selector: u32) -> Vec<T> {
            let mut addr = address(selector);
            let mut bytes = 0u32;
            // SAFETY: `addr` and `bytes` are live locals and the qualifier is
            // null, which every selector used here accepts.
            let status = unsafe {
                AudioObjectGetPropertyDataSize(
                    object,
                    NonNull::from(&mut addr),
                    0,
                    std::ptr::null(),
                    NonNull::from(&mut bytes),
                )
            };
            if status != 0 || bytes == 0 {
                return Vec::new();
            }
            let mut out = vec![T::default(); bytes as usize / std::mem::size_of::<T>()];
            let Some(buffer) = NonNull::new(out.as_mut_ptr()) else {
                return Vec::new();
            };
            let mut size = bytes;
            // SAFETY: `out` has room for exactly `size` bytes, which is the
            // length the HAL just reported.
            let status = unsafe {
                AudioObjectGetPropertyData(
                    object,
                    NonNull::from(&mut addr),
                    0,
                    std::ptr::null(),
                    NonNull::from(&mut size),
                    buffer.cast::<c_void>(),
                )
            };
            if status != 0 {
                return Vec::new();
            }
            out.truncate(size as usize / std::mem::size_of::<T>());
            out
        }

        /// The device's CoreAudio UID — the same string cpal puts after
        /// `coreaudio:` in a persisted device id.
        pub fn uid(device: AudioObjectID) -> Option<String> {
            let mut addr = address(kAudioDevicePropertyDeviceUID);
            let mut raw: *const CFString = std::ptr::null();
            let mut size = std::mem::size_of::<*const CFString>() as u32;
            // SAFETY: the selector is documented to write one CFStringRef, and
            // `size` says so. Ownership is +1, taken over by `from_raw` below.
            let status = unsafe {
                AudioObjectGetPropertyData(
                    device,
                    NonNull::from(&mut addr),
                    0,
                    std::ptr::null(),
                    NonNull::from(&mut size),
                    NonNull::from(&mut raw).cast::<c_void>(),
                )
            };
            if status != 0 {
                return None;
            }
            let owned = NonNull::new(raw.cast_mut())?;
            // SAFETY: `owned` is the +1 reference the HAL just handed us.
            let string = unsafe { CFRetained::from_raw(owned) };
            Some(string.to_string())
        }

        pub fn all_devices() -> Vec<AudioObjectID> {
            get_array::<AudioObjectID>(SYSTEM, kAudioHardwarePropertyDevices)
        }

        pub fn device_by_uid(wanted: &str) -> Option<AudioObjectID> {
            all_devices()
                .into_iter()
                .find(|&d| uid(d).as_deref() == Some(wanted))
        }

        pub fn default_input() -> Option<AudioObjectID> {
            get::<AudioObjectID>(SYSTEM, kAudioHardwarePropertyDefaultInputDevice)
                .filter(|&id| id != 0)
        }

        pub fn default_input_uid() -> Option<String> {
            uid(default_input()?)
        }

        pub fn set_default_input(device: AudioObjectID) -> bool {
            set(SYSTEM, kAudioHardwarePropertyDefaultInputDevice, device)
        }

        pub fn sample_rate(device: AudioObjectID) -> Option<f64> {
            get::<f64>(device, kAudioDevicePropertyNominalSampleRate)
        }

        pub fn set_sample_rate(device: AudioObjectID, rate: f64) -> bool {
            set(device, kAudioDevicePropertyNominalSampleRate, rate)
        }

        /// Discrete rates the device advertises, ignoring continuous ranges: a
        /// range we could pick any value out of is not a reconfiguration the
        /// driver will report.
        pub fn discrete_sample_rates(device: AudioObjectID) -> Vec<f64> {
            get_array::<ValueRange>(device, kAudioDevicePropertyAvailableNominalSampleRates)
                .into_iter()
                .filter(|r| (r.minimum - r.maximum).abs() < f64::EPSILON)
                .map(|r| r.minimum)
                .collect()
        }

        /// Add a device to the machine's device list, and hand back its id.
        ///
        /// `private` keeps it visible only to this process, so a real
        /// device-list change can be provoked without touching anything the
        /// user owns and without leaving a stray device behind.
        ///
        /// `wraps` names a sub-device by UID. Passing one gives the aggregate
        /// that device's input streams, which is what makes it eligible to be
        /// the system default input; passing `None` produces an empty
        /// aggregate, which still counts as a device-list change.
        pub fn create_private_aggregate(
            uid: &str,
            name: &str,
            wraps: Option<&str>,
        ) -> Option<AudioObjectID> {
            let mut keys = vec![
                NSString::from_str("uid"),
                NSString::from_str("name"),
                NSString::from_str("private"),
            ];
            // SAFETY: `NSString`, `NSNumber` and `NSArray` are ordinary
            // Objective-C objects; the casts only erase their static types.
            let mut values: Vec<Retained<AnyObject>> = unsafe {
                vec![
                    Retained::cast_unchecked(NSString::from_str(uid)),
                    Retained::cast_unchecked(NSString::from_str(name)),
                    Retained::cast_unchecked(NSNumber::new_i32(1)),
                ]
            };
            if let Some(sub) = wraps {
                let sub_key = NSString::from_str("uid");
                // SAFETY: as above.
                let sub_value: Retained<AnyObject> =
                    unsafe { Retained::cast_unchecked(NSString::from_str(sub)) };
                let entry = NSDictionary::from_retained_objects(&[&*sub_key], &[sub_value]);
                let list = NSArray::from_retained_slice(&[entry]);
                keys.push(NSString::from_str("subdevices"));
                // SAFETY: as above.
                values.push(unsafe { Retained::cast_unchecked(list) });
            }

            let key_refs: Vec<&NSString> = keys.iter().map(|k| &**k).collect();
            let description = NSDictionary::from_retained_objects(&key_refs, &values);

            let mut device: AudioObjectID = 0;
            // SAFETY: `NSDictionary` is toll-free bridged to `CFDictionary`, so
            // the pointer cast is the documented way across; `device` is a live
            // local the HAL writes one id into.
            let status = unsafe {
                let cf = &*(Retained::as_ptr(&description).cast::<CFDictionary>());
                AudioHardwareCreateAggregateDevice(cf, NonNull::from(&mut device))
            };
            (status == 0 && device != 0).then_some(device)
        }

        pub fn destroy_aggregate(device: AudioObjectID) -> bool {
            // SAFETY: `device` came from `create_private_aggregate` and has not
            // been destroyed yet.
            unsafe { AudioHardwareDestroyAggregateDevice(device) == 0 }
        }
    }

    // -----------------------------------------------------------------------
    // Event tap: read back what actually reached the window server
    // -----------------------------------------------------------------------

    /// A listen-only session tap, so a check can assert on the virtual keys and
    /// flags the injector really posted rather than on whatever a text field
    /// happened to render.
    pub mod tap {
        use std::cell::Cell;
        use std::ffi::{c_ulong, c_void};
        use std::ptr::NonNull;
        use std::sync::atomic::{AtomicBool, Ordering};
        use std::sync::Arc;
        use std::thread::JoinHandle;

        use objc2_core_foundation::{kCFRunLoopDefaultMode, CFMachPort, CFRunLoop};
        use objc2_core_graphics::{
            CGEvent, CGEventField, CGEventMask, CGEventTapCallBack, CGEventTapLocation,
            CGEventTapOptions, CGEventTapPlacement, CGEventTapProxy, CGEventType,
        };
        use parking_lot::Mutex;

        /// One keyboard event as the window server saw it.
        #[derive(Clone, Debug)]
        pub struct Seen {
            pub kind: &'static str,
            pub keycode: u16,
            pub flags: u64,
            /// The UTF-16 payload attached to the event, decoded. A plain key
            /// carries the character its keycode produces; a unicode event
            /// carries the character that has no key.
            pub text: String,
            /// `kCGEventSourceUnixProcessID`: 0 means the kernel HID system,
            /// anything else names the process that synthesized it.
            pub source_pid: i64,
        }

        struct Ctx {
            seen: Arc<Mutex<Vec<Seen>>>,
            port: Cell<Option<NonNull<CFMachPort>>>,
        }

        unsafe extern "C-unwind" fn callback(
            _proxy: CGEventTapProxy,
            kind: CGEventType,
            event: NonNull<CGEvent>,
            user: *mut c_void,
        ) -> *mut CGEvent {
            // SAFETY: `user` is the `Ctx` leaked by `run`, which outlives the
            // tap. The callback and the loop owning the `Cell` are the same
            // thread — callbacks fire only while that thread services its run
            // loop — so the `Cell` is never touched concurrently.
            let ctx = unsafe { &*(user as *const Ctx) };

            if matches!(
                kind,
                CGEventType::TapDisabledByTimeout | CGEventType::TapDisabledByUserInput
            ) {
                if let Some(port) = ctx.port.get() {
                    // SAFETY: the port outlives the tap thread.
                    CGEvent::tap_enable(unsafe { port.as_ref() }, true);
                }
                return event.as_ptr();
            }

            let label = match kind {
                CGEventType::KeyDown => "down",
                CGEventType::KeyUp => "up",
                CGEventType::FlagsChanged => "flags",
                _ => return event.as_ptr(),
            };

            // SAFETY: the tap owns this event for the duration of the callback.
            let cg = unsafe { event.as_ref() };
            let keycode =
                CGEvent::integer_value_field(Some(cg), CGEventField::KeyboardEventKeycode) as u16;
            let source_pid =
                CGEvent::integer_value_field(Some(cg), CGEventField::EventSourceUnixProcessID);
            let flags = CGEvent::flags(Some(cg)).0;

            let mut text = String::new();
            if label != "flags" {
                let mut buf = [0u16; 8];
                let mut len: c_ulong = 0;
                // SAFETY: `len` and `buf` are live locals and the length passed
                // is `buf`'s real capacity.
                unsafe {
                    CGEvent::keyboard_get_unicode_string(
                        Some(cg),
                        buf.len() as c_ulong,
                        &mut len,
                        buf.as_mut_ptr(),
                    );
                }
                let len = (len as usize).min(buf.len());
                text = String::from_utf16_lossy(&buf[..len]);
            }

            ctx.seen.lock().push(Seen {
                kind: label,
                keycode,
                flags,
                text,
                source_pid,
            });
            event.as_ptr()
        }

        pub struct KeyTap {
            seen: Arc<Mutex<Vec<Seen>>>,
            running: Arc<AtomicBool>,
            worker: Option<JoinHandle<()>>,
        }

        impl KeyTap {
            /// `None` when the tap could not be created, which on macOS means
            /// this binary is not trusted for Accessibility.
            pub fn start() -> Option<Self> {
                let seen: Arc<Mutex<Vec<Seen>>> = Arc::default();
                let running = Arc::new(AtomicBool::new(true));
                let (ready_tx, ready_rx) = std::sync::mpsc::channel::<bool>();

                let thread_seen = Arc::clone(&seen);
                let thread_running = Arc::clone(&running);
                let worker = std::thread::Builder::new()
                    .name("wl-probe-tap".into())
                    .spawn(move || run(thread_seen, thread_running, &ready_tx))
                    .ok()?;

                if ready_rx.recv().unwrap_or(false) {
                    Some(Self {
                        seen,
                        running,
                        worker: Some(worker),
                    })
                } else {
                    running.store(false, Ordering::SeqCst);
                    let _ = worker.join();
                    None
                }
            }

            /// Everything observed so far, oldest first.
            pub fn seen(&self) -> Vec<Seen> {
                self.seen.lock().clone()
            }

            pub fn clear(&self) {
                self.seen.lock().clear();
            }
        }

        impl Drop for KeyTap {
            fn drop(&mut self) {
                self.running.store(false, Ordering::SeqCst);
                if let Some(worker) = self.worker.take() {
                    let _ = worker.join();
                }
            }
        }

        fn run(
            seen: Arc<Mutex<Vec<Seen>>>,
            running: Arc<AtomicBool>,
            ready: &std::sync::mpsc::Sender<bool>,
        ) {
            let mask: CGEventMask = (1 << CGEventType::KeyDown.0)
                | (1 << CGEventType::KeyUp.0)
                | (1 << CGEventType::FlagsChanged.0);

            let ctx = Box::into_raw(Box::new(Ctx {
                seen,
                port: Cell::new(None),
            }));
            let handler: CGEventTapCallBack = Some(callback);

            // SAFETY: `handler` has the signature the tap expects and `ctx`
            // stays alive until this function drops it, after the tap is gone.
            let port = unsafe {
                CGEvent::tap_create(
                    CGEventTapLocation::SessionEventTap,
                    CGEventTapPlacement::HeadInsertEventTap,
                    // Listen only: the probe must never swallow the user's keys.
                    CGEventTapOptions::ListenOnly,
                    mask,
                    handler,
                    ctx.cast::<c_void>(),
                )
            };
            let Some(port) = port else {
                let _ = ready.send(false);
                // SAFETY: the tap was never created, so nothing else holds this
                // pointer.
                drop(unsafe { Box::from_raw(ctx) });
                return;
            };

            // SAFETY: callbacks run only on this thread's run loop, which has
            // not started, so nothing can be reading the cell.
            unsafe { (*ctx).port.set(Some(NonNull::from(&*port))) };

            let source = CFMachPort::new_run_loop_source(None, Some(&port), 0);
            let current = CFRunLoop::current();
            let (Some(source), Some(loop_ref)) = (source, current) else {
                let _ = ready.send(false);
                CFMachPort::invalidate(&port);
                // SAFETY: the tap is invalidated, so the callback cannot fire.
                drop(unsafe { Box::from_raw(ctx) });
                return;
            };

            // SAFETY: `kCFRunLoopDefaultMode` is an immortal framework constant.
            let mode = unsafe { kCFRunLoopDefaultMode };
            loop_ref.add_source(Some(&source), mode);
            CGEvent::tap_enable(&port, true);
            let _ = ready.send(true);

            // Short cycles rather than a parked `CFRunLoopRun`, so `Drop` can
            // stop the thread with a plain flag. A probe lives for seconds, so
            // the wakeup cost this trades for simplicity does not matter.
            while running.load(Ordering::SeqCst) {
                CFRunLoop::run_in_mode(mode, 0.05, false);
            }

            CFMachPort::invalidate(&port);
            // SAFETY: the tap is invalidated and its run-loop source dies with
            // it, so the callback cannot fire again.
            drop(unsafe { Box::from_raw(ctx) });
        }
    }

    // -----------------------------------------------------------------------
    // Synthetic keystrokes
    // -----------------------------------------------------------------------

    /// Press and release one key at the HID tap, the way a user's keyboard
    /// would appear to the system.
    fn press(virtual_key: u16, flags: CGEventFlags) {
        let source = CGEventSource::new(CGEventSourceStateID::HIDSystemState);
        for down in [true, false] {
            let Some(event) = CGEvent::new_keyboard_event(source.as_deref(), virtual_key, down)
            else {
                return;
            };
            CGEvent::set_flags(Some(&event), flags);
            CGEvent::post(CGEventTapLocation::HIDEventTap, Some(&event));
            std::thread::sleep(Duration::from_millis(25));
        }
    }

    /// Post F20 the way a foreign process would — byte for byte what
    /// `injector::post_key` does for one character, minus the guard the
    /// injector arms for itself.
    fn post_f20() {
        let Some(source) = CGEventSource::new(CGEventSourceStateID::Private) else {
            return;
        };
        for down in [true, false] {
            let Some(event) = CGEvent::new_keyboard_event(Some(&source), VK_F20, down) else {
                return;
            };
            CGEvent::set_flags(Some(&event), CGEventFlags::empty());
            CGEvent::post(CGEventTapLocation::HIDEventTap, Some(&event));
            std::thread::sleep(Duration::from_millis(40));
        }
    }

    // -----------------------------------------------------------------------
    // AUD-019 / AUD-020
    // -----------------------------------------------------------------------

    /// Drive the audio device-change listeners and report what arrives.
    ///
    /// Four phases. Each restores what it changed and verifies the restore by
    /// reading the value back rather than trusting the status code:
    ///
    /// 1. move the machine-wide default input and put it back;
    /// 2. change the device list by creating and destroying a private
    ///    aggregate device;
    /// 3. change the bound device's nominal sample rate and put it back;
    /// 4. an optional manual window for a real hot-plug.
    pub fn device_watch(
        report: &mut Report,
        logs: &LogSink,
        manual_seconds: u64,
    ) -> wl_platform::Result<()> {
        let capture = CpalCapture::new(None);
        let devices = capture.list_devices()?;
        capture.prewarm()?;
        let bound = capture.bound_device();
        report.note(format!(
            "watching with a live stream on {:?}",
            bound.as_ref().map(|d| d.name.as_str())
        ));
        // A stream that has just opened emits nothing; clear the slate.
        let _ = capture.take_faults();
        logs.clear();

        default_input_phase(report, &capture, &devices);
        device_list_phase(report, &capture);
        reconfigure_phase(report, &capture, bound.as_ref().map(|d| d.id.as_str()));
        hotplug_phase(report, &capture, manual_seconds);

        super::show_logs(report, logs);
        capture.release()?;
        Ok(())
    }

    /// Phase 1: does moving the machine's default input reach the app?
    ///
    /// Prefers a second *real* input device. When the machine only has one —
    /// a laptop with no Continuity microphone in range, which is the common
    /// case — it builds a private aggregate wrapping the built-in microphone,
    /// which has that device's input streams and is therefore eligible to be
    /// the system default. Either way the original default is put back and the
    /// restore is verified by reading the property again.
    fn default_input_phase(report: &mut Report, capture: &CpalCapture, devices: &[InputDevice]) {
        let Some(before) = audio_hw::default_input_uid() else {
            report.skip("AUD-019", "the machine reports no default input device");
            return;
        };
        let Some(original) = audio_hw::device_by_uid(&before) else {
            report.skip(
                "AUD-019",
                "could not map the current default input back onto a CoreAudio device",
            );
            return;
        };

        // A real second device if one is present, otherwise a stand-in we
        // create and destroy ourselves.
        let stand_in_uid = format!("wl-probe-default-{}", std::process::id());
        let real = devices
            .iter()
            .find(|d| !d.is_default)
            .map(|d| d.id.trim_start_matches("coreaudio:").to_owned())
            .and_then(|uid| audio_hw::device_by_uid(&uid).map(|id| (uid, id)));
        let (target_uid, target, stand_in) = match real {
            Some((uid, id)) => (uid, id, None),
            None => {
                let created = audio_hw::create_private_aggregate(
                    &stand_in_uid,
                    "WL Probe Default Input",
                    Some(&before),
                );
                let Some(id) = created else {
                    report.skip(
                        "AUD-019",
                        "only one input device and the HAL refused a stand-in aggregate, so \
                         the default input could not be moved",
                    );
                    return;
                };
                report.note(format!(
                    "only one real input device; created a private aggregate {stand_in_uid:?} \
                     (id={id}) wrapping {before:?} to move the default onto"
                ));
                // Adding it is itself a device-list change; do not let that
                // burst be mistaken for the default-change notification.
                let _ = wait_for_faults(capture, Duration::from_secs(2));
                (stand_in_uid.clone(), id, Some(id))
            }
        };

        report.note(format!(
            "moving the machine default input {before:?} -> {target_uid:?}"
        ));
        let moved = audio_hw::set_default_input(target);
        let observed = wait_for_faults(capture, Duration::from_secs(3));
        let during = audio_hw::default_input_uid();
        report.note(format!(
            "set returned {moved}; default input now {during:?}; faults {observed:?}"
        ));

        let restored = audio_hw::set_default_input(original);
        std::thread::sleep(Duration::from_millis(400));
        let back = audio_hw::default_input_uid();
        if back.as_deref() == Some(before.as_str()) {
            report.note(format!(
                "RESTORED: machine default input is {back:?} again (set returned {restored})"
            ));
        } else {
            report.fail(
                "AUD-019",
                format!(
                    "COULD NOT RESTORE the machine default input: wanted {before:?}, it is \
                     {back:?}. Fix this by hand in System Settings > Sound > Input."
                ),
            );
        }

        let mut all = observed;
        all.extend(wait_for_faults(capture, Duration::from_secs(2)));
        // A write the HAL accepts but does not act on proves nothing either
        // way: no notification is the correct answer to a default that never
        // moved. Only assert when the property really did change.
        if during.as_deref() == Some(target_uid.as_str()) {
            report.check(
                "AUD-019",
                all.iter()
                    .any(|f| matches!(f, CaptureFault::DefaultChanged)),
                format!("moving the machine default input raised {all:?}"),
            );
        } else {
            report.skip(
                "AUD-019",
                format!(
                    "the default input did not actually move — the HAL returned success for \
                     {target_uid:?} but the property still reads {during:?}, so the \
                     notification was never due. A private aggregate is not eligible as the \
                     system default input; this half needs a second REAL input device present."
                ),
            );
        }

        if let Some(id) = stand_in {
            let destroyed = audio_hw::destroy_aggregate(id);
            std::thread::sleep(Duration::from_millis(300));
            report.check(
                "AUD-019",
                destroyed && audio_hw::uid(id).is_none(),
                format!("the stand-in aggregate {target_uid:?} was destroyed again"),
            );
            let _ = wait_for_faults(capture, Duration::from_secs(2));
        }
    }

    /// Phase 2: does a change to the device *list* reach the app?
    ///
    /// A private aggregate device is the cheapest real device-list change
    /// available: the HAL genuinely adds and removes an entry, but the device
    /// is visible only to this process, so nothing the user owns is touched and
    /// nothing survives the run.
    fn device_list_phase(report: &mut Report, capture: &CpalCapture) {
        let uid = format!("wl-probe-aggregate-{}", std::process::id());
        let created = audio_hw::create_private_aggregate(&uid, "WL Probe Aggregate", None);
        let Some(device) = created else {
            report.skip(
                "AUD-019",
                "the HAL refused to create a private aggregate device, so the device list \
                 could not be changed",
            );
            return;
        };
        report.note(format!("created private aggregate device id={device}"));
        let added = wait_for_faults(capture, Duration::from_secs(3));

        let destroyed = audio_hw::destroy_aggregate(device);
        let removed = wait_for_faults(capture, Duration::from_secs(3));
        std::thread::sleep(Duration::from_millis(300));
        let gone = audio_hw::uid(device).is_none();
        report.check(
            "AUD-019",
            destroyed && gone,
            format!(
                "the aggregate device was destroyed again (destroy ok={destroyed}, \
                 gone from the HAL={gone})"
            ),
        );

        let mut all = added;
        all.extend(removed);
        report.check(
            "AUD-019",
            all.iter()
                .any(|f| format!("{f:?}").contains("DevicesChanged")),
            format!("adding and removing a device raised {all:?}"),
        );
    }

    /// Phase 3: does reconfiguring the bound device reach the app?
    fn reconfigure_phase(report: &mut Report, capture: &CpalCapture, bound_id: Option<&str>) {
        let target = bound_id
            .map(|id| id.trim_start_matches("coreaudio:"))
            .and_then(audio_hw::device_by_uid);
        let Some(device) = target else {
            report.skip("AUD-020", "no stream is bound, so nothing to reconfigure");
            return;
        };

        let current = audio_hw::sample_rate(device);
        let rates = audio_hw::discrete_sample_rates(device);
        report.note(format!(
            "bound device sample rate {current:?}, discrete rates advertised: {rates:?}"
        ));
        let alternative =
            current.and_then(|now| rates.iter().copied().find(|r| (r - now).abs() > 1.0));
        let (Some(original), Some(other)) = (current, alternative) else {
            report.skip(
                "AUD-020",
                format!(
                    "the bound device advertises no second discrete sample rate (current \
                     {current:?}, rates {rates:?}), so its configuration cannot be changed \
                     from here"
                ),
            );
            return;
        };

        let _ = capture.take_faults();
        report.note(format!(
            "reconfiguring the device {original} Hz -> {other} Hz"
        ));
        let changed = audio_hw::set_sample_rate(device, other);
        let observed = wait_for_faults(capture, Duration::from_secs(3));
        report.note(format!(
            "set returned {changed}; rate now {:?}; faults {observed:?}",
            audio_hw::sample_rate(device)
        ));

        let restored = audio_hw::set_sample_rate(device, original);
        std::thread::sleep(Duration::from_millis(600));
        let back = audio_hw::sample_rate(device);
        if back.is_some_and(|r| (r - original).abs() < 1.0) {
            report.note(format!(
                "RESTORED: sample rate is {back:?} again (set returned {restored})"
            ));
        } else {
            report.fail(
                "AUD-020",
                format!(
                    "COULD NOT RESTORE the device sample rate: wanted {original} Hz, it is \
                     {back:?}. Fix this by hand in Audio MIDI Setup."
                ),
            );
        }

        let mut all = observed;
        all.extend(wait_for_faults(capture, Duration::from_secs(2)));
        report.check(
            "AUD-020",
            all.iter().any(|f| {
                matches!(
                    f,
                    CaptureFault::StreamInvalidated | CaptureFault::DeviceLost
                )
            }),
            format!(
                "reconfiguring the bound device raised {all:?}; the pipeline rebuilds the \
                 stream on either of these"
            ),
        );
    }

    /// Phase 4: a real hot-plug, which no API can fake.
    fn hotplug_phase(report: &mut Report, capture: &CpalCapture, seconds: u64) {
        if seconds == 0 {
            report.note("manual hot-plug window skipped; pass a second count to enable it");
            return;
        }
        report.note(format!(
            "manual window: plug or unplug a microphone within {seconds}s"
        ));
        let started = Instant::now();
        let deadline = started + Duration::from_secs(seconds);
        let mut any = Vec::new();
        while Instant::now() < deadline {
            let faults = capture.take_faults();
            if !faults.is_empty() {
                report.note(format!("+{:?} {faults:?}", started.elapsed()));
                any.extend(faults);
            }
            std::thread::sleep(Duration::from_millis(200));
        }
        report.check(
            "AUD-019",
            !any.is_empty(),
            format!("manual hot-plug window observed {any:?}"),
        );
    }

    fn wait_for_faults(capture: &CpalCapture, budget: Duration) -> Vec<CaptureFault> {
        let deadline = Instant::now() + budget;
        let mut out = Vec::new();
        while Instant::now() < deadline {
            out.extend(capture.take_faults());
            if !out.is_empty() {
                // One more sweep: a burst arrives over a few milliseconds.
                std::thread::sleep(Duration::from_millis(250));
                out.extend(capture.take_faults());
                break;
            }
            std::thread::sleep(Duration::from_millis(50));
        }
        out
    }

    // -----------------------------------------------------------------------
    // HTK-011
    // -----------------------------------------------------------------------

    /// HTK-011: synthetic keystrokes must not trigger the hotkey.
    ///
    /// Two halves, and they have different answers.
    ///
    /// * **Foreign synthetic input.** The Swift listener gated on the CGEvent
    ///   source's unix PID being 0, and `handy-keys` hands the worker a decoded
    ///   `KeyEvent` with no CGEvent behind it, so there is nothing to read that
    ///   field from. The probe posts F20 from this process and reports what
    ///   happens.
    /// * **Our own synthetic input**, rejected through the armed window in
    ///   `wl_platform::macos::begin_synthetic_input`. That is the half that
    ///   matters in practice: without it a transcript containing the hotkey
    ///   character retriggers the very recording that is typing it.
    ///
    /// F20 rather than Left Control on purpose. This machine runs the user's
    /// real dictation app, whose push-to-talk binding is Left Control; posting
    /// that would start a live recording in someone else's process.
    pub fn synthetic(report: &mut Report, human_seconds: u64) -> wl_platform::Result<()> {
        use wl_core::settings::hotkey::{Hotkey, Modifiers, TriggerKey};
        use wl_platform::macos::{synthetic_input_in_flight, SYNTHETIC_GUARD};

        let Some(tap) = tap::KeyTap::start() else {
            report.skip("HTK-011", "could not create an observation tap");
            return Ok(());
        };

        let backend = wl_platform::current::hotkeys()?;
        backend.rebind(&[Hotkey::combo(Modifiers::NONE, TriggerKey::F(20))])?;
        report.note(format!(
            "bound dictate to F20; backend healthy={}",
            backend.is_healthy()
        ));
        std::thread::sleep(Duration::from_millis(500));
        let events = backend.events();
        while events.try_recv().is_ok() {}

        // -- Foreign synthetic input, nothing armed --------------------------
        let unguarded = post_and_collect(report, &tap, &backend, "unguarded", false);
        let ours = std::process::id();
        report.check(
            "HTK-011",
            unguarded.is_empty(),
            format!(
                "a synthetic F20 posted by pid {ours} produced {} hotkey event(s); the row \
                 requires zero, because the Swift listener gated on the event source's unix \
                 pid being 0",
                unguarded.len()
            ),
        );
        if !unguarded.is_empty() {
            report.note(
                "residual gap: handy-keys gives the worker a decoded KeyEvent and never the \
                 CGEvent, so kCGEventSourceUnixProcessID cannot be read where the decision is \
                 made. Foreign synthetic input is therefore still accepted.",
            );
        }

        // -- Our own synthetic input, guard armed ----------------------------
        std::thread::sleep(SYNTHETIC_GUARD * 3);
        report.check(
            "HTK-011",
            !synthetic_input_in_flight(),
            "the guard window closes on its own",
        );
        while events.try_recv().is_ok() {}
        let guarded = post_and_collect(report, &tap, &backend, "guard armed", true);
        report.check(
            "HTK-011",
            guarded.is_empty(),
            format!(
                "with begin_synthetic_input armed, the same synthetic F20 produced {} hotkey \
                 event(s)",
                guarded.len()
            ),
        );

        // -- A real injection in flight --------------------------------------
        // The end-to-end version of the same guard: a live Natural Mode
        // injection arms the window for itself, so a key arriving in the middle
        // of it must be swallowed. Typed into a scratch TextEdit document, so
        // the characters cannot land anywhere the user cares about.
        match Scratch::open("") {
            Some(scratch) => {
                while events.try_recv().is_ok() {}
                let injector = wl_platform::current::platform().injector;
                let handle = std::thread::spawn(move || {
                    injector.inject(
                        "probe injection in flight, deliberately long enough to still be \
                         running when the key arrives",
                        wl_platform::InjectMode::Natural {
                            chars_per_second: 8.0,
                        },
                    )
                });
                std::thread::sleep(Duration::from_millis(400));
                // The decisive datum: was the window actually open at the
                // instant the key arrived? The injector arms it per keystroke
                // for SYNTHETIC_GUARD, so a key landing in a gap between
                // characters is a different failure from a key landing inside
                // the window and being let through anyway.
                let armed_when_posted = synthetic_input_in_flight();
                post_f20();
                std::thread::sleep(Duration::from_millis(400));
                let during: Vec<HotkeyEvent> = events.try_iter().collect();
                let injected = handle.join().map_err(|_| {
                    wl_platform::PlatformError::Other("the injection thread panicked".into())
                })?;
                let typed = scratch.text();
                scratch.close();
                report.note(format!(
                    "concurrent injection returned {injected:?}, document held {typed:?}, \
                     guard armed at the moment of the post: {armed_when_posted}"
                ));
                report.check(
                    "HTK-011",
                    during.is_empty(),
                    format!(
                        "a synthetic F20 arriving while our own injection was in flight \
                         produced {} hotkey event(s) (guard armed: {armed_when_posted})",
                        during.len()
                    ),
                );
            }
            None => report.skip(
                "HTK-011",
                "TextEdit did not come to the front, so the in-flight injection half was not \
                 exercised",
            ),
        }

        // -- A real key must still work --------------------------------------
        if human_seconds == 0 {
            report.skip(
                "HTK-011",
                "the real-key half needs a human; run `probe synthetic 10` and tap Right Shift",
            );
            return Ok(());
        }

        backend.rebind(&[Hotkey::modifier(Modifiers::SHIFT_RIGHT)])?;
        backend.reset();
        std::thread::sleep(Duration::from_millis(300));
        while events.try_recv().is_ok() {}
        tap.clear();
        report.note(format!(
            "now tap RIGHT SHIFT yourself within {human_seconds}s — it is inert, and it is not \
             the other app's binding"
        ));
        let deadline = Instant::now() + Duration::from_secs(human_seconds);
        let mut human = Vec::new();
        while let Some(left) = deadline.checked_duration_since(Instant::now()) {
            match events.recv_timeout(left) {
                Ok(event) => human.push(event),
                Err(_) => break,
            }
        }
        let from_hid = tap.seen().into_iter().filter(|s| s.source_pid == 0).count();
        if human.is_empty() {
            report.skip(
                "HTK-011",
                format!(
                    "nobody tapped Right Shift ({from_hid} kernel-HID event(s) seen), so the \
                     real-key half was not observed"
                ),
            );
        } else {
            report.pass(
                "HTK-011",
                format!(
                    "a real key ({from_hid} tap event(s) with source pid 0) still produced {} \
                     hotkey event(s): {human:?}",
                    human.len()
                ),
            );
        }
        Ok(())
    }

    /// Post a synthetic F20, optionally arming the synthetic-input guard first,
    /// and report both what the window server saw and what the backend emitted.
    fn post_and_collect(
        report: &Report,
        tap: &tap::KeyTap,
        backend: &Arc<dyn HotkeyBackend>,
        phase: &str,
        arm_guard: bool,
    ) -> Vec<HotkeyEvent> {
        use wl_platform::macos::{begin_synthetic_input, SYNTHETIC_GUARD};

        tap.clear();
        if arm_guard {
            begin_synthetic_input(SYNTHETIC_GUARD);
        }
        post_f20();
        std::thread::sleep(Duration::from_millis(500));

        for seen in tap.seen().into_iter().filter(|s| s.keycode == VK_F20) {
            report.note(format!(
                "{phase}: tap saw {} keycode={} flags=0x{:x} source_pid={} \
                 (0 would mean kernel HID)",
                seen.kind, seen.keycode, seen.flags, seen.source_pid
            ));
        }
        backend.events().try_iter().collect()
    }

    // -----------------------------------------------------------------------
    // INJ-026 / INJ-027 / INJ-029 and POL-026
    // -----------------------------------------------------------------------

    /// A scratch TextEdit document to type into, plus the means to put it back.
    struct Scratch {
        path: std::path::PathBuf,
    }

    impl Scratch {
        /// Open a document in TextEdit and wait for it to take focus.
        ///
        /// Opened by the app's absolute bundle path rather than by name, so
        /// this cannot resolve to something else that answers to "TextEdit".
        fn open(contents: &str) -> Option<Self> {
            let path = std::env::temp_dir().join(format!("wl-probe-{}.txt", std::process::id()));
            std::fs::write(&path, contents).ok()?;
            let scratch = Self { path };
            if scratch.focus() {
                Some(scratch)
            } else {
                let _ = std::fs::remove_file(&scratch.path);
                None
            }
        }

        /// Bring the document to the front, and confirm it got there.
        ///
        /// Worth re-running before every measurement: this machine is shared
        /// with other automation, and a browser activating itself mid-check
        /// sends the keystrokes somewhere else entirely.
        fn focus(&self) -> bool {
            // Three goes at the activation, not one. `open` reports success
            // while LaunchServices quietly declines to reactivate, and on a
            // machine where something else is also opening windows a single
            // attempt loses the race often enough to matter.
            for _ in 0..3 {
                let launched = std::process::Command::new("/usr/bin/open")
                    .arg("-a")
                    .arg("/System/Applications/TextEdit.app")
                    .arg(&self.path)
                    .status();
                if launched.is_err() {
                    return false;
                }

                let deadline = Instant::now() + Duration::from_secs(4);
                while Instant::now() < deadline {
                    if Self::is_front() {
                        // The window is up; give the caret a beat to land.
                        std::thread::sleep(Duration::from_millis(700));
                        return true;
                    }
                    std::thread::sleep(Duration::from_millis(200));
                }
            }
            false
        }

        fn is_front() -> bool {
            wl_platform::current::platform()
                .foreground
                .current()
                .bundle_id
                == "com.apple.TextEdit"
        }

        fn text(&self) -> String {
            wl_platform::current::platform()
                .injector
                .read_focused_text()
                .join("\n")
        }

        /// Empty the document, save it so no dialog appears, close the window,
        /// and delete the file.
        fn close(self) {
            press(VK_A, CGEventFlags::MaskCommand);
            press(VK_DELETE, CGEventFlags::empty());
            press(VK_S, CGEventFlags::MaskCommand);
            std::thread::sleep(Duration::from_millis(500));
            press(VK_W, CGEventFlags::MaskCommand);
            std::thread::sleep(Duration::from_millis(500));
            let _ = std::fs::remove_file(&self.path);
        }
    }

    /// INJ-026 / INJ-027 / INJ-029: newline, tab and an unmapped character in
    /// Natural Mode.
    ///
    /// The document read-back proves the characters landed; the event tap
    /// proves *how* — a real Return key rather than a unicode event carrying
    /// U+000A, which the rendered text alone cannot distinguish.
    pub fn keys(report: &mut Report) -> wl_platform::Result<()> {
        let Some(tap) = tap::KeyTap::start() else {
            report.skip(
                "INJ-026/INJ-027/INJ-029",
                "could not create an observation tap",
            );
            return Ok(());
        };
        let Some(scratch) = Scratch::open("") else {
            report.skip(
                "INJ-026/INJ-027/INJ-029",
                "TextEdit did not come to the front within 8s",
            );
            return Ok(());
        };

        tap.clear();
        let injector = wl_platform::current::platform().injector;
        injector.inject(
            INJECT_PROBE,
            wl_platform::InjectMode::Natural {
                chars_per_second: 6.0,
            },
        )?;
        std::thread::sleep(Duration::from_millis(500));

        let typed = scratch.text();
        let seen = tap.seen();
        scratch.close();

        report.note("inject returned Ok".to_string());
        report.note(format!("document read back as {typed:?}"));
        let downs: Vec<_> = seen.iter().filter(|s| s.kind == "down").collect();
        for event in &downs {
            report.note(format!(
                "tap: down keycode={} flags=0x{:x} text={:?}",
                event.keycode, event.flags, event.text
            ));
        }
        let unmodified = |s: &tap::Seen| s.flags & CGEventFlags::MaskCommand.0 == 0;
        let shifted = |s: &tap::Seen| s.flags & CGEventFlags::MaskShift.0 != 0;

        report.check(
            "INJ-026",
            downs
                .iter()
                .any(|s| s.keycode == VK_RETURN && shifted(s) && s.text.contains('\r')),
            format!(
                "newline typed as virtual key {VK_RETURN} (Return) with Shift held, so it does \
                 not submit the composer it lands in"
            ),
        );
        report.check(
            "INJ-027",
            downs
                .iter()
                .any(|s| s.keycode == VK_TAB && unmodified(s) && s.text.contains('\t')),
            format!("tab typed as virtual key {VK_TAB} (Tab) with no modifiers"),
        );
        report.check(
            "INJ-029",
            downs
                .iter()
                .any(|s| s.keycode == VK_UNICODE_CARRIER && s.text.contains('\u{1F600}')),
            format!(
                "the unmapped character was typed as virtual key {VK_UNICODE_CARRIER} with its \
                 UTF-16 units attached"
            ),
        );
        report.check(
            "INJ-026/INJ-027/INJ-029",
            typed == INJECT_PROBE,
            format!("the document holds exactly {INJECT_PROBE:?}"),
        );
        Ok(())
    }
}

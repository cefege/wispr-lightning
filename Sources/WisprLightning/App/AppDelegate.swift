import AppKit
import ApplicationServices

private let logFilePath: String = FileManager.default.homeDirectoryForCurrentUser
    .appendingPathComponent("Library/Logs/WisprLightning.log").path
private let logRotatedPath: String = FileManager.default.homeDirectoryForCurrentUser
    .appendingPathComponent("Library/Logs/WisprLightning.log.1").path
/// Cap the live log at 5 MB. When exceeded, rotate (mv to .log.1, drop the
/// previous .log.1) so disk usage stays bounded but recent history survives.
private let logMaxBytes: UInt64 = 5 * 1024 * 1024

private var logFile: FileHandle? = {
    FileManager.default.createFile(atPath: logFilePath, contents: nil)
    return FileHandle(forWritingAtPath: logFilePath)
}()

private let logQueue = DispatchQueue(label: "com.wisprlightning.log")
private let logDateFormatter: ISO8601DateFormatter = {
    let f = ISO8601DateFormatter()
    return f
}()
private var logBytesWritten: UInt64 = {
    let attrs = try? FileManager.default.attributesOfItem(atPath: logFilePath)
    return (attrs?[.size] as? UInt64) ?? 0
}()

private func rotateLogIfNeeded(addedBytes: Int) {
    logBytesWritten &+= UInt64(addedBytes)
    guard logBytesWritten > logMaxBytes else { return }
    // Use the throwing `close()` (macOS 10.15+) instead of `closeFile()` —
    // the legacy variant raises NSExceptions that can't be caught from Swift
    // and crash the process when the descriptor is already gone.
    try? logFile?.close()
    // Drop previous rotated file, move current → .1, start fresh.
    try? FileManager.default.removeItem(atPath: logRotatedPath)
    try? FileManager.default.moveItem(atPath: logFilePath, toPath: logRotatedPath)
    FileManager.default.createFile(atPath: logFilePath, contents: nil)
    logFile = FileHandle(forWritingAtPath: logFilePath)
    logBytesWritten = 0
}

func wLog(_ message: String) {
    logQueue.async {
        let ts = logDateFormatter.string(from: Date())
        let line = "[\(ts)] \(message)\n"
        let data = line.data(using: .utf8) ?? Data()
        // Use the throwing APIs (macOS 10.15+). The legacy `seekToEndOfFile`
        // and `write(_:)` raise NSExceptions on a bad descriptor (closed file,
        // I/O error) which Swift can't catch — they abort the process. With
        // `seekToEnd()` / `write(contentsOf:)` we get a regular `Error`, can
        // null out the handle, and fall back to NSLog so subsequent log calls
        // are silent on disk but still visible in Console.app.
        guard let handle = logFile else { return }
        do {
            try handle.seekToEnd()
            try handle.write(contentsOf: data)
            rotateLogIfNeeded(addedBytes: data.count)
        } catch {
            logFile = nil
            NSLog("Wispr Lightning: log write failed (%@); further log lines will go to NSLog only", error.localizedDescription)
        }
    }
    NSLog("Wispr Lightning: %@", message)
}

var isVerboseLoggingEnabled: Bool = false

func wLogVerbose(_ message: String) {
    guard isVerboseLoggingEnabled else { return }
    wLog("[VERBOSE] \(message)")
}

class AppDelegate: NSObject, NSApplicationDelegate {
    var statusBarController: StatusBarController!
    private var session: Session!
    private var settings: AppSettings!
    private var dbManager: DatabaseManager!
    private var audioRecorder: AudioRecorder!
    private var dictationProvider: DictationProvider!
    private var textInjector: TextInjector!
    private var hotkeyListener: HotkeyListener!
    private var historyStore: HistoryStore!
    private var dictionaryStore: DictionaryStore!
    private var polishService: PolishService!
    private var polishStore: PolishStore!
    private var notesStore: NotesStore!
    private var soundManager: SoundManager!
    private var musicController: MusicController!
    private enum RecordingState { case idle, listening, recording }
    private var recordingState: RecordingState = .idle
    private var lastPressTime: Date?
    private static let lockDebounceInterval: TimeInterval = 0.5
    private static let trailingBufferInterval: TimeInterval = 0.5
    /// Short tail capture after a tap-to-stop press in toggle / locked modes.
    /// Without it, the final word often clips because the user releases the
    /// thought a frame before the syllable finishes. 0.25s matches typical
    /// utterance-end inertia without delaying transcription perceptibly.
    private static let toggleStopTrailingBuffer: TimeInterval = 0.25
    private var isRecording: Bool { recordingState != .idle }
    private var recordingOverlay: RecordingOverlay!
    private var toastNotification: ToastNotification!
    private var recordingTimer: Timer?
    private var recordingStartTime: Date?
    private var recordingMaxSec = 0
    private var recordingWarnSec = 0
    private var recordingFinalSec = 0
    private var cachedOCRContext: [String] = []
    private var cachedAXContext: [String] = []
    /// OCR captured during the previous recording, fed to Claude Voice's
    /// keyterms hint for the *next* session (URL-fixed keyterms can't be
    /// added after WS open).
    private var lastSessionOcrLines: [String] = []
    private var tapDelayTimer: Timer?
    private var processingTimeoutTimer: Timer?
    private var rearmTimer: Timer?
    private var settingsObserver: NSObjectProtocol?
    private var audioDevicesObserver: NSObjectProtocol?
    private var cmdCommaMonitor: Any?
    private var onboardingController: OnboardingWindowController?
    private var pendingPackets: [Data]?
    /// Owns the .pcm file for the in-flight / pending dictation. Replaces
    /// the previous trio of `activeRecordingFileHandle`,
    /// `activeRecordingFileURL`, `pendingAudioFileURL`. Lives from open() in
    /// startRecordingSession through delete() in the inject success path
    /// (or other terminal sites).
    private var pendingAudio: RecordingArtifact?
    private var pendingAppInfo: [String: String]?
    private var pendingOcrContext: [String]?
    private var pendingAxContext: [String]?
    private var currentRetryAttempt = 0
    private var isTranscribing = false
    private static let maxAutoRetries = 2
    /// Recent-attempt telemetry surfaced in the status-bar "Recent
    /// dictations" submenu. Lets the user (and us) see whether the fallback
    /// chain and watchdog are doing anything in practice.
    private let telemetryStore = TelemetryStore()
    /// Set when the current attempt starts (after audio capture stops, before
    /// dictationProvider.stop). Used to compute elapsed at terminal outcome.
    private var attemptStartedAt: Date?
    /// True if any per-provider watchdog fired during the current attempt.
    /// Reset per fresh dictation, propagated through chain hops.
    private var attemptWatchdogFired = false
    /// Per-provider hard ceiling. Scaled by recording duration in
    /// `providerWatchdogTimeout(for:)` because some backends (OpenRouter +
    /// Gemini doing audio-in-and-text-out, Wispr Flow on a long upload) take
    /// proportional time to a 10-minute recording — a flat 45s would
    /// pre-empt a legitimately slow result and falsely advance the chain.
    private static let perProviderWatchdogBase: TimeInterval = 45
    private static let perProviderWatchdogPerSecond: TimeInterval = 0.4
    private static let perProviderWatchdogCap: TimeInterval = 300
    /// Index into the fallback chain. 0 = primary vendor (settings.activeVendor);
    /// 1..N = settings.fallbackChain[index - 1]. Reset to 0 between dictations.
    private var currentChainIndex: Int = 0
    private static let pendingAudioDir: URL = {
        let base = FileManager.default.urls(for: .applicationSupportDirectory, in: .userDomainMask).first
            ?? FileManager.default.homeDirectoryForCurrentUser
                .appendingPathComponent("Library/Application Support")
        let dir = base.appendingPathComponent("WisprLightning/PendingAudio")
        try? FileManager.default.createDirectory(at: dir, withIntermediateDirectories: true)
        return dir
    }()
    private var wisprFlowSessionWatcher: DispatchSourceFileSystemObject?
    private let ocrQueue = DispatchQueue(label: "com.wisprlightning.ocr", qos: .userInitiated)
    private let axQueue = DispatchQueue(label: "com.wisprlightning.ax", qos: .userInitiated)

    func applicationDidFinishLaunching(_ notification: Notification) {
        settings = AppSettings.load()
        session = Session()
        dbManager = DatabaseManager()
        historyStore = HistoryStore(dbManager: dbManager)
        dictionaryStore = DictionaryStore(dbManager: dbManager)
        polishStore = PolishStore(dbManager: dbManager)
        notesStore = NotesStore(dbManager: dbManager)
        audioRecorder = AudioRecorder(settings: settings)
        activeVendor = DictationVendor(rawValue: settings.activeVendor) ?? .wisprFlow
        dictationProvider = Self.makeProvider(vendor: activeVendor, session: session, settings: settings)
        dictationProvider.dictionaryStore = dictionaryStore
        polishService = PolishService(session: session, settings: settings)
        textInjector = TextInjector(settings: settings)
        soundManager = SoundManager(settings: settings)
        musicController = MusicController(settings: settings)

        statusBarController = StatusBarController(
            session: session,
            settings: settings,
            historyStore: historyStore,
            dictionaryStore: dictionaryStore,
            notesStore: notesStore,
            textInjector: textInjector,
            telemetryStore: telemetryStore
        )

        recordingOverlay = RecordingOverlay()
        recordingOverlay.prewarm()
        recordingOverlay.onCancelAction = { [weak self] in
            self?.cancelActiveRecording()
        }
        toastNotification = ToastNotification()

        isVerboseLoggingEnabled = settings.verboseLogging

        settingsObserver = NotificationCenter.default.addObserver(forName: .settingsChanged, object: nil, queue: .main) { [weak self] notification in
            if let updated = notification.object as? AppSettings {
                isVerboseLoggingEnabled = updated.verboseLogging
            }
            guard let self = self else { return }
            self.refreshProviderIfChanged()
            self.rearmMicrophone()
        }

        audioDevicesObserver = NotificationCenter.default.addObserver(forName: .audioDevicesChanged, object: nil, queue: .main) { [weak self] _ in
            guard let self = self else { return }
            self.statusBarController.updateMenu()

            if self.isRecording {
                if let targetUID = self.settings.micDeviceUID {
                    let devices = AudioRecorder.listInputDevices()
                    if !devices.contains(where: { $0.uid == targetUID }) {
                        // Mid-recording mic disconnect (AirPods walk out of
                        // range, USB mic unplugged). Previously we just logged
                        // and let the engine keep capturing against whatever
                        // device CoreAudio fell back to — silently producing
                        // wrong-source audio. Now: stop the session so the
                        // packets we DID capture get transcribed and the user
                        // sees an immediate result instead of a corrupted one.
                        wLog("Target mic '\(self.settings.micDeviceName ?? targetUID)' disconnected during recording — stopping session")
                        self.stopRecordingSession()
                    }
                }
            } else {
                self.rearmMicrophone()
            }
        }

        let hasSession = session.load()
        if !hasSession {
            NSLog("Wispr Lightning: No session found — sign in via Settings")
        } else {
            NSLog("Wispr Lightning: Session loaded for %@", session.userEmail ?? "unknown")
        }

        startWisprFlowSessionWatcher()

        statusBarController.updateMenu()

        // Auto-open settings on first launch if not signed in
        if !hasSession {
            statusBarController.openSettings()
        }

        hotkeyListener = HotkeyListener(
            settings: settings,
            session: session,
            currentVendor: { [weak self] in self?.activeVendor ?? .wisprFlow },
            onPress: { [weak self] in self?.onHotkeyPress() },
            onRelease: { [weak self] in self?.onHotkeyRelease() }
        )
        hotkeyListener.onPolishPress = { [weak self] in self?.onPolishHotkeyPress() }
        hotkeyListener.start()

        statusBarController.onTogglePause = { [weak self] in
            guard let self = self else { return }
            self.hotkeyListener.setPaused(!self.hotkeyListener.isPaused)
        }
        statusBarController.onShowOnboarding = { [weak self] in
            self?.showOnboarding()
        }

        // Pre-warm microphone if enabled (eliminates iPhone Continuity Camera startup delay)
        if settings.keepMicrophoneActive {
            audioRecorder.prewarm()
        }

        // Seed dictionary defaults and pre-warm cache off main thread
        DispatchQueue.global(qos: .utility).async { [weak self] in
            guard let self = self else { return }
            self.dictionaryStore.seedDefaults(userName: self.session.userFirstName)
            // Pre-warm dictionary cache so first transcription is fast
            _ = self.dictionaryStore.getVocabularyPhrases()
            _ = self.dictionaryStore.getReplacements()
            _ = self.dictionaryStore.getSnippets()
            // Prune old / excess history rows so the SQLite file doesn't
            // accumulate forever on long-running installs.
            self.historyStore.prune()
        }

        // Abort recording if Mac goes to sleep
        NSWorkspace.shared.notificationCenter.addObserver(
            self,
            selector: #selector(onSystemSleep),
            name: NSWorkspace.willSleepNotification,
            object: nil
        )

        if settings.showInDock {
            NSApp.setActivationPolicy(.regular)
        }

        // Build main menu bar (visible when showInDock is true)
        let mainMenu = NSMenu()

        let appMenuItem = NSMenuItem()
        let appMenu = NSMenu()
        appMenu.addItem(withTitle: "About Wispr Lightning", action: #selector(NSApplication.orderFrontStandardAboutPanel(_:)), keyEquivalent: "")
        appMenu.addItem(NSMenuItem.separator())
        let settingsMenuItem = NSMenuItem(title: "Settings...", action: #selector(openSettingsFromMenu), keyEquivalent: ",")
        settingsMenuItem.target = self
        appMenu.addItem(settingsMenuItem)
        appMenu.addItem(NSMenuItem.separator())
        appMenu.addItem(withTitle: "Quit Wispr Lightning", action: #selector(NSApplication.terminate(_:)), keyEquivalent: "q")
        appMenuItem.submenu = appMenu
        mainMenu.addItem(appMenuItem)

        NSApp.mainMenu = mainMenu

        // Local key event monitor for Cmd+, when in accessory/menu-bar-only mode
        cmdCommaMonitor = NSEvent.addLocalMonitorForEvents(matching: .keyDown) { [weak self] event in
            if event.modifierFlags.contains(.command) && event.charactersIgnoringModifiers == "," {
                self?.statusBarController.openSettings()
                return nil
            }
            return event
        }

        // Onboarding wizard: auto-show whenever a required permission is
        // missing, or on first launch (didCompleteOnboarding == false).
        let requiredOk = PermissionsManager.allRequiredGranted()
        if !requiredOk || !settings.didCompleteOnboarding {
            showOnboarding()
        }
        wLog("Permissions on launch — mic=\(PermissionsManager.status(.microphone)) input=\(PermissionsManager.status(.inputMonitoring)) ax=\(PermissionsManager.status(.accessibility)) screen=\(PermissionsManager.status(.screenRecording))")

        wLog("Ready — press \(settings.hotkeyLabels.first ?? "Left Control") to start dictating")

        // Check for unsent recordings from a previous crash/failure
        recoverPendingAudio()

        // Register for URL scheme callbacks
        NSAppleEventManager.shared().setEventHandler(
            self,
            andSelector: #selector(handleURLEvent(_:withReplyEvent:)),
            forEventClass: AEEventClass(kInternetEventClass),
            andEventID: AEEventID(kAEGetURL)
        )
    }

    @objc private func openSettingsFromMenu() {
        statusBarController.openSettings()
    }

    @objc func handleURLEvent(_ event: NSAppleEventDescriptor, withReplyEvent reply: NSAppleEventDescriptor) {
        guard let urlString = event.paramDescriptor(forKeyword: AEKeyword(keyDirectObject))?.stringValue,
              let url = URL(string: urlString) else { return }
        NSLog("Wispr Lightning: Received URL callback: %@", urlString)
        // Only handle auth callbacks; ignore other wispr-flow:// deep links
        guard urlString.contains("auth/") else { return }
        AuthService.handleCallback(url: url, session: session) { success in
            DispatchQueue.main.async {
                if success {
                    NSLog("Wispr Lightning: Sign in successful")
                    NotificationCenter.default.post(name: .sessionChanged, object: nil)
                } else {
                    NSLog("Wispr Lightning: Sign in failed")
                }
            }
        }
    }

    @objc private func onSystemSleep() {
        guard isRecording else { return }
        wLog("System going to sleep — aborting recording")
        abortRecording(reason: "system sleep")
    }

    /// User clicked the hover-revealed ✕ on the pill. Mirrors the sleep path:
    /// discard packets, cancel the provider, hide the pill. No-op if no
    /// recording is active so a stray hover-click race can't crash anything.
    private func cancelActiveRecording() {
        guard isRecording else { return }
        wLog("User cancelled recording via pill ✕")
        // Only record if an attempt was actually in flight. Cancelling
        // during Listening (no Processing yet) wouldn't have a meaningful
        // duration to surface.
        if attemptStartedAt != nil {
            recordAttempt(outcome: .cancelled, vendor: nil, preview: nil)
        }
        abortRecording(reason: "user cancel")
    }

    /// Shared teardown for non-graceful recording exits (sleep, user cancel).
    /// Drops in-memory provider state but preserves any on-disk PCM snapshot
    /// from a prior `attemptTranscription()` so the next launch's recovery
    /// path can offer the user to retry rather than silently losing audio.
    private func abortRecording(reason: String) {
        recordingState = .idle
        lastPressTime = nil

        recordingTimer?.invalidate()
        recordingTimer = nil
        rearmTimer?.invalidate()
        rearmTimer = nil
        recordingStartTime = nil
        hotkeyListener.resetState()

        audioRecorder.onLevelUpdate = nil
        audioRecorder.onPacket = nil
        _ = audioRecorder.stop() // discard packets
        // Close any in-flight incremental file but KEEP it on disk. Recovery
        // on the next launch can offer the user to retry; the 24h sweep
        // handles ones they explicitly dismiss or ignore.
        pendingAudio?.finishWriting()
        dictationProvider.cancel()
        clearPendingTranscription()

        statusBarController.setRecording(false)
        recordingOverlay.hide()
        resumeMusicInBackground()
    }

    private func onHotkeyPress() {
        switch recordingState {
        case .idle:
            // First press: start recording in "Listening" (push-to-talk) state
            recordingState = .listening
            lastPressTime = Date()
            startRecordingSession()

        case .listening:
            // Second press: cancel any pending tap-delay stop
            tapDelayTimer?.invalidate()
            tapDelayTimer = nil
            // If quick succession → lock into hands-free "Recording" mode
            let elapsed = lastPressTime.map { Date().timeIntervalSince($0) } ?? 1.0
            if elapsed < AppDelegate.lockDebounceInterval {
                recordingState = .recording
                lastPressTime = Date()
                wLog("Recording locked — hands-free mode")
                recordingOverlay.showLocked()
            } else {
                // Slow second press: treat as stop with trailing tail capture.
                stopRecordingSessionWithTrailingBuffer()
            }

        case .recording:
            // Third press: stop hands-free recording with trailing tail
            // capture so the final syllable doesn't clip.
            stopRecordingSessionWithTrailingBuffer()
        }
    }

    /// Schedule a `stopRecordingSession()` after `toggleStopTrailingBuffer`
    /// has elapsed. Audio capture continues during the buffer; the recorder
    /// stop call happens on the timer's tick. If the user re-presses inside
    /// the window we cancel the pending stop and treat it as the normal
    /// state machine input.
    private func stopRecordingSessionWithTrailingBuffer() {
        tapDelayTimer?.invalidate()
        tapDelayTimer = Timer.scheduledTimer(withTimeInterval: AppDelegate.toggleStopTrailingBuffer, repeats: false) { [weak self] _ in
            guard let self = self else { return }
            // Only fire if the user hasn't already moved us out of an active
            // recording state (e.g. clicked the pill ✕ during the 0.25s
            // window — abortRecording already torn things down).
            guard self.recordingState == .recording || self.recordingState == .listening else { return }
            self.stopRecordingSession()
        }
    }

    private func rearmMicrophone() {
        rearmTimer?.invalidate()
        rearmTimer = Timer.scheduledTimer(withTimeInterval: 0.15, repeats: false) { [weak self] _ in
            guard let self = self else { return }
            self.audioRecorder.deactivate()
            if self.settings.keepMicrophoneActive {
                self.audioRecorder.prewarm()
            }
        }
    }

    private func startRecordingSession() {
        pendingAppInfo = AppInfoDetector.getFrontmostAppInfo()
        recordingMaxSec   = Constants.maxRecordingSeconds
        recordingWarnSec  = Constants.warningSeconds
        recordingFinalSec = Constants.finalWarningSeconds
        soundManager.playStart()

        // Open the incremental disk file BEFORE wiring callbacks so the very
        // first packet has somewhere to land. If the file can't be created
        // (disk full, perms broken) we still proceed — the in-memory packets
        // path remains the source of truth for transcription.
        let filename = "recording-\(logDateFormatter.string(from: Date())).pcm"
        let url = Self.pendingAudioDir.appendingPathComponent(filename)
        pendingAudio = RecordingArtifact(creatingAt: url)
        if pendingAudio == nil {
            wLog("Failed to create incremental audio file at \(url.lastPathComponent) — proceeding without disk snapshot")
        }

        // Wire the audio capture callbacks BEFORE audioRecorder.start() so no
        // packets/level updates emitted in the engine's startup tick are lost
        // (they fire on the capture thread; a nil callback at that instant
        // silently drops the packet, costing us the first few ms of audio).
        if let cv = dictationProvider as? ClaudeVoiceProvider {
            cv.setPendingOcrLines(lastSessionOcrLines)
        }
        dictationProvider.start()
        audioRecorder.onPacket = { [weak self] packet in
            guard let self = self else { return }
            self.dictationProvider.feed(packet: packet)
            // RecordingArtifact owns the I/O queue + handle. Append is a
            // single dispatch_async; on disk error it tears down its own
            // handle so subsequent writes silently no-op.
            self.pendingAudio?.append(packet)
        }
        audioRecorder.onLevelUpdate = { [weak self] level in
            DispatchQueue.main.async {
                self?.recordingOverlay.updateAudioLevel(level)
            }
        }

        let startResult = audioRecorder.start()
        switch startResult {
        case .started:
            break
        case .startedWithFallback:
            wLog("Recording started with fallback mic (requested device unavailable)")
        case .failed(let reason):
            wLog("Failed to start recording: \(reason)")
            audioRecorder.onPacket = nil
            audioRecorder.onLevelUpdate = nil
            dictationProvider.cancel()
            recordingState = .idle
            recordingOverlay.showError(message: "Mic unavailable")
            musicController.resumeMusic()
            return
        }
        recordingStartTime = Date()

        // Pause music in background — AppleScript calls are slow
        DispatchQueue.global(qos: .userInitiated).async { [weak self] in
            self?.musicController.pauseMusic()
        }

        // Pre-warm WebSocket connection (TCP+TLS handshake) during recording
        dictationProvider.prewarmConnection()

        // Capture accessibility context in background — AX API can be slow on some apps
        if settings.useAccessibilityContext {
            axQueue.async { [weak self] in
                let context = TextInjector.readFocusedElementText()
                self?.cachedAXContext = context
                wLog("AX context: \(context.isEmpty ? "none" : "\(context[0].prefix(80))...")")
            }
        } else {
            cachedAXContext = []
        }

        // Start OCR capture early — runs in parallel with recording
        if settings.useScreenContext {
            ocrQueue.async { [weak self] in
                let context = ScreenCaptureContext.captureOCRContext()
                self?.cachedOCRContext = context
                wLog("OCR context (early): \(context.count) lines captured")
            }
        } else {
            cachedOCRContext = []
        }

        statusBarController.setRecording(true)
        recordingOverlay.show()

        // Start 1-second repeating timer for duration tracking
        recordingTimer = Timer.scheduledTimer(withTimeInterval: 1.0, repeats: true) { [weak self] _ in
            self?.onRecordingTimerTick()
        }
        wLog("Recording started")
    }

    private func onRecordingTimerTick() {
        guard let startTime = recordingStartTime else { return }
        let elapsed = Int(Date().timeIntervalSince(startTime))
        recordingOverlay.updateElapsed(elapsed)
        if elapsed >= recordingMaxSec {
            wLog("Max recording duration reached (\(recordingMaxSec)s), auto-stopping")
            stopRecordingSession()
        } else if elapsed >= recordingFinalSec {
            recordingOverlay.showFinalWarning()
        } else if elapsed >= recordingWarnSec {
            recordingOverlay.showWarning()
        }
    }

    private func onHotkeyRelease() {
        // In locked (hands-free) mode, key release does nothing — next press stops recording
        guard recordingState == .listening else { return }

        let heldDuration = lastPressTime.map { Date().timeIntervalSince($0) } ?? 1.0
        let behavior = settings.hotkeyPressBehavior

        if heldDuration >= AppDelegate.lockDebounceInterval {
            // Long hold (PTT): stop after a short trailing buffer to capture
            // tail-end of speech. Same across all behaviors — holding always
            // behaves as push-to-talk.
            tapDelayTimer?.invalidate()
            tapDelayTimer = Timer.scheduledTimer(withTimeInterval: AppDelegate.trailingBufferInterval, repeats: false) { [weak self] _ in
                guard let self = self, self.recordingState == .listening else { return }
                self.stopRecordingSession()
            }
            return
        }

        switch behavior {
        case "hold":
            // Quick tap in hold-only mode: end the recording immediately. No
            // debounce wait, no locking — releasing the key always stops.
            tapDelayTimer?.invalidate()
            tapDelayTimer = nil
            stopRecordingSession()

        case "toggle":
            // Quick tap: lock immediately into hands-free recording. Next
            // press stops it.
            tapDelayTimer?.invalidate()
            tapDelayTimer = nil
            recordingState = .recording
            lastPressTime = Date()
            wLog("Recording locked — tap-to-toggle mode")
            recordingOverlay.showLocked()

        default:
            // "legacy" — wait for a potential 2nd press to lock. Fire at
            // exactly lockDebounceInterval from the first press so a quick
            // 2nd press wins.
            let remaining = AppDelegate.lockDebounceInterval - heldDuration
            tapDelayTimer?.invalidate()
            tapDelayTimer = Timer.scheduledTimer(withTimeInterval: remaining, repeats: false) { [weak self] _ in
                guard let self = self, self.recordingState == .listening else { return }
                self.stopRecordingSession()
            }
        }
    }

    private func stopRecordingSession() {
        guard isRecording else { return }
        recordingState = .idle
        lastPressTime = nil

        tapDelayTimer?.invalidate()
        tapDelayTimer = nil

        // Stop recording timer
        recordingTimer?.invalidate()
        recordingTimer = nil
        let elapsedRecordingTime = recordingStartTime.map { Date().timeIntervalSince($0) } ?? 0
        recordingStartTime = nil

        audioRecorder.onLevelUpdate = nil
        audioRecorder.onPacket = nil
        let packets = audioRecorder.stop()
        // Drain any in-flight incremental writes, then close the handle. The
        // serial sync inside finishWriting() guarantees every queued packet
        // has landed on disk before we hand the artifact to the transcription
        // pipeline.
        pendingAudio?.finishWriting()
        soundManager.playStop()
        statusBarController.setRecording(false)

        guard packets.count >= 5 else {
            dictationProvider.cancel()
            // Discard the tiny file — no value in keeping <200ms of audio.
            pendingAudio?.delete()
            pendingAudio = nil
            if packets.count == 0 && elapsedRecordingTime > 1.0 {
                wLog("Recording captured 0 packets over \(String(format: "%.1f", elapsedRecordingTime))s — likely mic disconnected")
                recordingOverlay.showError(message: "Mic not responding")
            } else {
                wLog("Too short (\(packets.count) packets), ignoring")
                recordingOverlay.hide()
            }
            musicController.resumeMusic()
            return
        }

        recordingOverlay.showProcessing()

        pendingPackets = packets
        currentRetryAttempt = 0
        // Reset telemetry accumulators for this fresh attempt. Re-entry from
        // auto-retry / fallback-chain advance keeps them sticky so the final
        // record reflects the whole attempt, not just the last hop.
        attemptStartedAt = Date()
        attemptWatchdogFired = false
        scheduleProcessingTimeout()
        if pendingAudio == nil {
            wLog("Recording finished without an on-disk snapshot — no crash recovery for this attempt")
        }

        DispatchQueue.global(qos: .userInitiated).async { [weak self, count = packets.count] in
            guard let self = self else { return }
            // Drain OCR/AX queues here — avoids blocking main thread
            self.pendingOcrContext = self.ocrQueue.sync {
                let ctx = self.cachedOCRContext
                self.cachedOCRContext = []
                return ctx
            }
            // Stash for next-session keyterms hint (Claude Voice only uses this).
            if let ocr = self.pendingOcrContext, !ocr.isEmpty {
                self.lastSessionOcrLines = ocr
            }
            self.pendingAxContext = self.axQueue.sync {
                let ctx = self.cachedAXContext
                self.cachedAXContext = []
                return ctx
            }
            wLog("Recording stopped — \(count) packets (\(String(format: "%.1f", Double(count) * 0.04))s), transcribing with \(self.pendingOcrContext?.count ?? 0) OCR lines...")
            self.attemptTranscription()
        }
    }

    private func attemptTranscription() {
        guard !isTranscribing else {
            wLog("Transcription already in flight, skipping duplicate attempt")
            return
        }
        guard let packets = pendingPackets,
              let appInfo = pendingAppInfo else { return }

        isTranscribing = true

        let ocrContext = pendingOcrContext ?? []
        let axContext = pendingAxContext ?? []
        let context = DictationContext(appInfo: appInfo, ocrContext: ocrContext, axContext: axContext)

        // Re-prime the provider's internal buffer whenever we're talking to a
        // provider that wasn't fed live during recording — that's the case
        // for every retry (manual or auto), every fallback chain step beyond
        // the primary, and after dismissRetry+retryTranscription. The initial
        // attempt is skipped because audioRecorder.onPacket already fed it.
        if currentRetryAttempt > 0 || currentChainIndex > 0 {
            dictationProvider.cancel()
            dictationProvider.start()
            for packet in packets {
                dictationProvider.feed(packet: packet)
            }
        }

        // Per-provider watchdog + idempotent completion. The watchdog fires
        // after the scaled timeout and synthesizes a `.timeout` if the
        // provider never calls back — covers crashed/hung WS handshakes,
        // misbehaving SDK threads, anything that would otherwise park the
        // pill in Processing forever. Both paths funnel through the same
        // SafeCompletion gate so chain advancement only runs once.
        let gate = SafeCompletion<Result<TranscriptResult, TranscriptionError>> { [weak self] result in
            guard let self = self else { return }
            self.handleTranscriptionResult(result, appInfo: appInfo)
        }

        let recordingSeconds = Double(packets.count) * Double(Constants.chunkDurationMs) / 1000.0
        let watchdogSeconds = min(
            Self.perProviderWatchdogCap,
            Self.perProviderWatchdogBase + recordingSeconds * Self.perProviderWatchdogPerSecond
        )
        let watchdog = DispatchWorkItem { [weak self] in
            wLog("Provider watchdog fired after \(Int(watchdogSeconds))s — forcing fallback")
            self?.attemptWatchdogFired = true
            self?.dictationProvider.cancel()
            gate.fire(.failure(.timeout))
        }
        DispatchQueue.global().asyncAfter(deadline: .now() + watchdogSeconds, execute: watchdog)

        dictationProvider.stop(context: context) { result in
            watchdog.cancel()
            gate.fire(result)
        }
    }

    /// Body of the transcription completion handler, extracted so both the
    /// provider's natural completion and the per-provider watchdog can share
    /// the same code path via `safeComplete`. `appInfo` is the snapshot taken
    /// at recording start; everything else reads from `self`.
    private func handleTranscriptionResult(
        _ result: Result<TranscriptResult, TranscriptionError>,
        appInfo: [String: String]
    ) {
        switch result {
            case .success(let transcriptResult):
                self.isTranscribing = false
                // Record telemetry BEFORE clearPendingTranscription nukes
                // currentChainIndex / attemptStartedAt.
                let preview = (transcriptResult.formattedText ?? transcriptResult.asrText)
                    .map { String($0.prefix(60)) }
                let finalVendor = self.activeVendorForChainStep().displayName
                self.recordAttempt(
                    outcome: preview?.isEmpty == false ? .success : .failure,
                    vendor: finalVendor,
                    preview: preview
                )
                // Capture the artifact BEFORE clearing state so we can
                // delete the file only after the text actually lands in the
                // focused app. Previously we cleared (and deleted the file)
                // before inject ran — if inject failed, crashed, or
                // returned thin results, the user lost both the transcript
                // and the source audio with no way to retry.
                let artifactToRetire = self.pendingAudio
                self.clearPendingTranscription()
                self.resumeMusicInBackground()

                DispatchQueue.main.async {
                    var displayText = transcriptResult.formattedText ?? transcriptResult.asrText ?? ""
                    if !displayText.isEmpty {
                        // Email signature
                        if self.settings.emailAutoSignature && appInfo["type"] == "email" {
                            let suffix = self.settings.emailSignatureOption == "spoken_with_lightning"
                                ? "\n\n— Spoken with Wispr Lightning"
                                : "\n\n— Written with Wispr Lightning"
                            displayText += suffix
                        }

                        wLog("Injecting: \(String(displayText.prefix(80)))")

                        let activeInstructions = self.settings.activePolishInstructions
                        if self.session.canUsePolish(activeVendor: self.activeVendor) && self.settings.autoPolish && self.settings.polishEnabled
                            && !activeInstructions.isEmpty {
                            // Auto-polish runs async and itself can hang or fail.
                            // Keep the source audio on disk for ~60s as a safety
                            // net — if polish silently never injects, the user
                            // still has the .pcm to recover on next launch.
                            // The opportunistic sweep cleans it up after 24h.
                            artifactToRetire?.deleteAfter(60)
                        } else {
                            self.recordingOverlay.showInserting()
                            self.textInjector.inject(text: displayText) { pasteSucceeded in
                                if pasteSucceeded {
                                    // Transcript landed in the focused app —
                                    // user has it. Safe to drop the audio.
                                    artifactToRetire?.delete()
                                } else {
                                    // Paste failed (focused field gone, clipboard
                                    // blocked, accessibility revoked). The
                                    // transcript wasn't delivered — keep the
                                    // audio file so recovery / Save can offer
                                    // the user another shot. Sweep handles
                                    // eventual cleanup after 24h.
                                    wLog("Inject reported failure — keeping audio file for recovery: \(artifactToRetire?.url.lastPathComponent ?? "(nil)")")
                                }
                                DispatchQueue.main.async { self.recordingOverlay.hide() }
                            }
                        }

                        self.statusBarController.setLastTranscription(displayText)

                        // Move DB writes off main thread
                        DispatchQueue.global(qos: .utility).async {
                            self.historyStore.addEntry(result: transcriptResult, appInfo: appInfo, language: self.settings.languages.joined(separator: ","))

                            if self.settings.autoLearnWords,
                               let asrText = transcriptResult.asrText,
                               let formattedText = transcriptResult.formattedText {
                                self.autoLearnWords(asrText: asrText, formattedText: formattedText)
                            }
                        }

                        // Auto-polish after dictation (Wispr Flow only)
                        if self.session.canUsePolish(activeVendor: self.activeVendor) && self.settings.autoPolish && self.settings.polishEnabled {
                            self.autoPolishText(displayText)
                        }
                    } else {
                        wLog("Empty transcription result")
                        // Empty result almost always means the mic captured
                        // silence / room tone / unintelligible speech — the
                        // recovery flow can't usefully retry it (the same
                        // providers will return the same empty result), and
                        // emptyResult.shouldFallback is false so no chain
                        // advance happens. Drop the file now; the 24h sweep
                        // would only delay the same outcome.
                        artifactToRetire?.delete()
                        self.recordingOverlay.showError(message: TranscriptionError.emptyResult.userMessage)
                    }
                }

            case .failure(let error):
                self.isTranscribing = false

                // Fallback chain: if the user configured one and this failure
                // should fall through (auth / network / server / timeout),
                // jump straight to the next chain step with the same audio
                // packets. Single-shot per step — chain length IS the retry
                // budget. emptyResult never falls back (mic didn't catch it).
                if error.shouldFallback && self.hasNextChainStep() {
                    let nextVendor = self.advanceChainStep()
                    wLog("Fallback: step \(self.currentChainIndex) → \(nextVendor.displayName) (after \(error.userMessage))")
                    DispatchQueue.main.async {
                        self.recordingOverlay.showRetrying(
                            attempt: self.currentChainIndex + 1,
                            maxAttempts: self.settings.fallbackChain.count + 1
                        )
                    }
                    DispatchQueue.global(qos: .userInitiated).asyncAfter(deadline: .now() + 0.3) { [weak self] in
                        self?.attemptTranscription()
                    }
                    return
                }

                if error.isRetryable && self.currentRetryAttempt < Self.maxAutoRetries {
                    self.currentRetryAttempt += 1
                    let attempt = self.currentRetryAttempt
                    let maxAttempts = Self.maxAutoRetries + 1
                    wLog("Transcription failed (retryable): \(error.userMessage) — auto-retry \(attempt)/\(Self.maxAutoRetries)")

                    DispatchQueue.main.async {
                        self.recordingOverlay.showRetrying(attempt: attempt + 1, maxAttempts: maxAttempts)
                    }

                    // Pre-warm connection during retry delay so TCP+TLS handshake overlaps with wait
                    self.dictationProvider.prewarmConnection()

                    DispatchQueue.global(qos: .userInitiated).asyncAfter(deadline: .now() + 1.5) { [weak self] in
                        self?.attemptTranscription()
                    }
                } else if error.isRetryable {
                    // Auto-retries exhausted — show persistent retry UI
                    wLog("Transcription failed after \(Self.maxAutoRetries) retries: \(error.userMessage)")
                    self.recordAttempt(
                        outcome: .failure,
                        vendor: self.activeVendorForChainStep().displayName,
                        preview: error.userMessage
                    )
                    self.resumeMusicInBackground()

                    DispatchQueue.main.async {
                        self.recordingOverlay.showRetryableError(
                            message: error.userMessage,
                            onRetry: { [weak self] in self?.retryTranscription() },
                            onSave: { [weak self] in self?.saveAudioToDownloads() },
                            onDismiss: { [weak self] in self?.dismissRetry() }
                        )
                    }
                } else {
                    // Non-retryable error — still show persistent retry UI so audio is never lost
                    wLog("Transcription failed (non-retryable): \(error.userMessage)")
                    self.recordAttempt(
                        outcome: .failure,
                        vendor: self.activeVendorForChainStep().displayName,
                        preview: error.userMessage
                    )
                    self.resumeMusicInBackground()

                    DispatchQueue.main.async {
                        self.recordingOverlay.showRetryableError(
                            message: error.userMessage,
                            onRetry: { [weak self] in self?.retryTranscription() },
                            onSave: { [weak self] in self?.saveAudioToDownloads() },
                            onDismiss: { [weak self] in self?.dismissRetry() }
                        )
                    }
                }
        }
    }

    private func retryTranscription() {
        // Manual retry restarts the chain from the top: rebuild the primary
        // provider and reset both counters so attemptTranscription re-primes
        // its buffer and the user gets a fresh 2x auto-retry budget.
        currentChainIndex = 0
        currentRetryAttempt = 1
        dictationProvider.cancel()
        dictationProvider = Self.makeProvider(vendor: activeVendor, session: session, settings: settings)
        dictationProvider.dictionaryStore = dictionaryStore
        recordingOverlay.showProcessing()
        // Pre-warm connection so TCP+TLS handshake starts immediately
        dictationProvider.prewarmConnection()
        scheduleProcessingTimeout()
        DispatchQueue.global(qos: .userInitiated).async { [weak self] in
            self?.attemptTranscription()
        }
    }

    private func scheduleProcessingTimeout() {
        let packetCount = pendingPackets?.count ?? 0
        let recordingDuration = Double(packetCount) * Double(Constants.chunkDurationMs) / 1000.0
        let timeout = max(30.0, 30.0 + recordingDuration * 0.5)
        processingTimeoutTimer?.invalidate()
        processingTimeoutTimer = Timer.scheduledTimer(withTimeInterval: timeout, repeats: false) { [weak self] _ in
            guard let self = self else { return }
            wLog("Processing timeout — showing retry UI (packets preserved)")
            self.processingTimeoutTimer = nil
            self.isTranscribing = false
            self.resumeMusicInBackground()
            self.recordingOverlay.showRetryableError(
                message: "Timed out",
                onRetry: { [weak self] in self?.retryTranscription() },
                onSave: { [weak self] in self?.saveAudioToDownloads() },
                onDismiss: { [weak self] in self?.dismissRetry() }
            )
        }
    }

    private func dismissRetry() {
        // User explicitly chose Dismiss after seeing the Save button — the
        // audio is no longer wanted. Drop the .pcm now so it doesn't linger
        // until the 24h sweep.
        pendingAudio?.delete()
        clearPendingTranscription()
        recordingOverlay.hide()
    }

    private func resumeMusicInBackground() {
        DispatchQueue.global(qos: .utility).async { [weak self] in
            self?.musicController.resumeMusic()
        }
    }

    /// Opportunistic cleanup of stale PendingAudio files. Called after every
    /// dictation completes so a long-running install (no relaunch in weeks)
    /// doesn't accumulate 24h+ of failed-recovery .pcm files.
    /// `activePath` snapshots the currently-in-use file path on the main
    /// thread so the background sweep doesn't read `pendingAudio` concurrently
    /// with the next dictation writing to it.
    private func sweepStalePendingAudio(activePath: String?) {
        let dir = Self.pendingAudioDir
        guard let files = try? FileManager.default.contentsOfDirectory(
            at: dir, includingPropertiesForKeys: [.creationDateKey]
        ) else { return }
        let now = Date()
        for file in files where file.pathExtension == "pcm" {
            // Don't sweep the file we're currently using.
            if let active = activePath, file.path == active { continue }
            guard let created = (try? file.resourceValues(forKeys: [.creationDateKey]).creationDate),
                  now.timeIntervalSince(created) > 86400 else { continue }
            try? FileManager.default.removeItem(at: file)
        }
    }

    /// Single source of truth for "which vendor sits at chain step N." Step 0
    /// is the primary vendor (`settings.activeVendor`); step >0 reads from
    /// `settings.fallbackChain[index - 1]`. Used by telemetry recording,
    /// chain advancement, and live provider rebuild.
    private func vendorAtChainStep(_ index: Int) -> DictationVendor {
        if index == 0 {
            return DictationVendor(rawValue: settings.activeVendor) ?? .wisprFlow
        }
        return DictationVendor(rawValue: settings.fallbackChain[index - 1].vendor) ?? .wisprFlow
    }

    /// Convenience: the vendor that owns the *current* chain step. Wraps
    /// `vendorAtChainStep(currentChainIndex)` so callers don't have to
    /// thread `currentChainIndex` explicitly.
    private func activeVendorForChainStep() -> DictationVendor {
        return vendorAtChainStep(currentChainIndex)
    }

    /// Append an attempt record to the telemetry ring buffer. Snapshots
    /// `currentChainIndex` / `attemptWatchdogFired` / `attemptStartedAt`
    /// before the caller clears them via `clearPendingTranscription()`.
    private func recordAttempt(outcome: AttemptRecord.Outcome, vendor: String?, preview: String?) {
        let started = attemptStartedAt ?? Date()
        let elapsed = Date().timeIntervalSince(started)
        let record = AttemptRecord(
            id: UUID(),
            timestamp: Date(),
            finalVendor: outcome == .success ? vendor : nil,
            fallbackHops: currentChainIndex,
            watchdogFired: attemptWatchdogFired,
            elapsedSeconds: elapsed,
            outcome: outcome,
            preview: preview
        )
        telemetryStore.record(record)
        attemptStartedAt = nil
        attemptWatchdogFired = false
    }

    /// Reset transcription state. Does NOT delete the on-disk audio file —
    /// each caller decides explicitly by calling `pendingAudio?.delete()` /
    /// `.deleteAfter(_:)` (or capturing the artifact reference for deferred
    /// deletion). Splitting the file-lifecycle decision from the state-reset
    /// removes a class of "comment says keep, code deletes" bugs where a
    /// default-true bool quietly threw away audio the caller meant to keep.
    private func clearPendingTranscription() {
        processingTimeoutTimer?.invalidate()
        processingTimeoutTimer = nil
        pendingPackets = nil
        pendingAudio = nil
        pendingAppInfo = nil
        pendingOcrContext = nil
        pendingAxContext = nil
        currentRetryAttempt = 0
        currentChainIndex = 0
        isTranscribing = false
        dictationProvider.clearEncodingCache()
        // After a successful or dismissed dictation, reset the live provider
        // back to the user's primary vendor so the next dictation starts fresh.
        if currentVendor() != activeVendor {
            // Already handled by refreshProviderIfChanged in the settings observer.
        } else {
            dictationProvider.cancel()
            dictationProvider = Self.makeProvider(vendor: activeVendor, session: session, settings: settings)
            dictationProvider.dictionaryStore = dictionaryStore
        }
        // Opportunistic sweep — snapshot the active path on main, then run
        // the directory scan on a background queue so the I/O doesn't block
        // the UI.
        let activePath = pendingAudio?.url.path
        DispatchQueue.global(qos: .background).async { [weak self] in
            self?.sweepStalePendingAudio(activePath: activePath)
        }
    }

    /// Save audio as a playable WAV file to ~/Downloads.
    private func saveAudioToDownloads() {
        guard let packets = pendingPackets, !packets.isEmpty else { return }
        let downloadsDir = FileManager.default.urls(for: .downloadsDirectory, in: .userDomainMask).first
            ?? FileManager.default.homeDirectoryForCurrentUser.appendingPathComponent("Downloads")
        let timestamp = logDateFormatter.string(from: Date())
            .replacingOccurrences(of: ":", with: "-")
        let url = downloadsDir.appendingPathComponent("wispr-recording-\(timestamp).wav")

        let wav = AudioEncoding.wavData(from: packets)

        do {
            try wav.write(to: url)
            wLog("Saved WAV to Downloads: \(url.lastPathComponent) (\(wav.count / 1024)KB)")
        } catch {
            wLog("Failed to save WAV to Downloads: \(error.localizedDescription)")
        }
    }

    /// Save audio packets to disk so they survive app crashes and failed retries.
    /// Format: simple concatenation of fixed-size packets (each 1280 bytes = 640 samples × 2).
    private func saveAudioToDisk(_ packets: [Data]) -> URL? {
        let filename = "recording-\(logDateFormatter.string(from: Date())).pcm"
        let url = Self.pendingAudioDir.appendingPathComponent(filename)
        var combined = Data(capacity: packets.count * Constants.chunkSamples * 2)
        for packet in packets {
            combined.append(packet)
        }
        do {
            try combined.write(to: url)
            wLog("Saved \(packets.count) packets (\(combined.count / 1024)KB) to \(filename)")
            return url
        } catch {
            wLog("Failed to save audio: \(error.localizedDescription)")
            return nil
        }
    }

    /// Load audio packets back from a saved file.
    private func loadAudioFromDisk(_ url: URL) -> [Data]? {
        guard let data = try? Data(contentsOf: url) else { return nil }
        let packetSize = Constants.chunkSamples * 2 // 1280 bytes
        guard data.count >= packetSize else { return nil }
        var packets: [Data] = []
        packets.reserveCapacity(data.count / packetSize)
        var offset = 0
        while offset + packetSize <= data.count {
            packets.append(data.subdata(in: offset..<(offset + packetSize)))
            offset += packetSize
        }
        wLog("Loaded \(packets.count) packets from \(url.lastPathComponent)")
        return packets
    }

    /// Check for leftover audio from a previous crash/failure and offer retry.
    private func recoverPendingAudio() {
        let dir = Self.pendingAudioDir
        guard let files = try? FileManager.default.contentsOfDirectory(at: dir, includingPropertiesForKeys: [.creationDateKey]) else { return }
        let pcmFiles = files.filter { $0.pathExtension == "pcm" }
        guard let mostRecent = pcmFiles.sorted(by: {
            let d1 = (try? $0.resourceValues(forKeys: [.creationDateKey]).creationDate) ?? .distantPast
            let d2 = (try? $1.resourceValues(forKeys: [.creationDateKey]).creationDate) ?? .distantPast
            return d1 > d2
        }).first else { return }

        let recoveredFileCreated = (try? mostRecent.resourceValues(forKeys: [.creationDateKey]).creationDate) ?? .distantPast
        let fileAge = Date().timeIntervalSince(recoveredFileCreated)
        // Only recover files from the last 24 hours
        if fileAge > 86400 {
            // Too old — clean up all pending files
            for file in pcmFiles { try? FileManager.default.removeItem(at: file) }
            return
        }

        guard let packets = loadAudioFromDisk(mostRecent) else {
            try? FileManager.default.removeItem(at: mostRecent)
            return
        }

        wLog("Recovered \(packets.count) packets from previous session: \(mostRecent.lastPathComponent)")
        pendingPackets = packets
        pendingAudio = RecordingArtifact(capturedAt: mostRecent)
        pendingAppInfo = ["name": "Unknown", "bundle_id": "", "type": "other", "url": ""]
        currentRetryAttempt = 0
        // Recovery always starts from the primary vendor. Without this, if the
        // previous session crashed mid-fallback (chainIndex=2), the recovered
        // audio would be replayed into the same broken fallback step — not
        // what the user wants if they've since fixed their primary auth.
        currentChainIndex = 0

        // Auto-retry fresh files (< 90s old) silently. If the user just
        // crashed mid-dictation 30 seconds ago, parking a retry pill in
        // front of them is friction — try the transcription first; only
        // surface the retry UI if the auto-retry itself fails. Older files
        // (probably from a session they've forgotten about) keep the
        // existing explicit-prompt behavior so they don't get a surprise
        // transcript dumped into whatever app they're now using.
        if fileAge < 90 {
            wLog("Recovered file is \(Int(fileAge))s old — auto-retrying transcription silently")
            DispatchQueue.main.async {
                self.retryTranscription()
            }
        } else {
            DispatchQueue.main.async {
                self.recordingOverlay.showRetryableError(
                    message: "Recovered unsent recording",
                    onRetry: { [weak self] in self?.retryTranscription() },
                    onSave: { [weak self] in self?.saveAudioToDownloads() },
                    onDismiss: { [weak self] in self?.dismissRetry() }
                )
            }
        }

        // Clean up any other old files
        for file in pcmFiles where file != mostRecent {
            try? FileManager.default.removeItem(at: file)
        }
    }

    // MARK: - Polish

    private func onPolishHotkeyPress() {
        guard session.canUsePolish(activeVendor: activeVendor) else {
            wLog("Polish skipped — Wispr Flow account / vendor required")
            return
        }
        guard settings.polishEnabled else { return }

        let activeInstructions = settings.activePolishInstructions
        guard !activeInstructions.isEmpty else {
            wLog("Polish: no instructions enabled")
            return
        }

        let appInfo = AppInfoDetector.getFrontmostAppInfo()

        // Show the pill and play start sound immediately
        soundManager.playStart()
        recordingOverlay.show()
        recordingOverlay.showProcessing()

        DispatchQueue.global(qos: .userInitiated).async { [weak self] in
            guard let self = self else { return }

            // Save original clipboard before touching it, so we can restore after polish
            let originalClipboard = TextInjector.saveClipboard()

            // Simulate Cmd+C to copy whatever is selected in the focused app.
            // More reliable than AX kAXSelectedTextAttribute, works across all apps.
            let source = CGEventSource(stateID: .hidSystemState)
            if let keyDown = CGEvent(keyboardEventSource: source, virtualKey: 8, keyDown: true),
               let keyUp = CGEvent(keyboardEventSource: source, virtualKey: 8, keyDown: false) {
                keyDown.flags = .maskCommand
                keyUp.flags = .maskCommand
                keyDown.post(tap: .cghidEventTap)
                keyUp.post(tap: .cghidEventTap)
            }

            // Give the target app time to process the copy
            Thread.sleep(forTimeInterval: 0.15)

            var selectedText: String?
            DispatchQueue.main.sync {
                selectedText = NSPasteboard.general.string(forType: .string)
            }

            guard let text = selectedText, !text.isEmpty else {
                wLog("Polish: no text selected")
                DispatchQueue.main.async {
                    TextInjector.restoreClipboard(originalClipboard)
                    self.recordingOverlay.showError(message: "Select text to polish")
                }
                return
            }

            wLog("Polish: processing \(text.count) chars with \(activeInstructions.count) instructions")

            self.polishService.polish(text: text, instructions: activeInstructions) { [weak self] result in
                guard let self = self else { return }

                DispatchQueue.main.async {
                    switch result {
                    case .success(let polishResult):
                        wLog("Polish complete: \(polishResult.polishedText.count) chars in \(String(format: "%.1f", polishResult.processingTime))s")

                        self.recordingOverlay.showInserting()
                        self.textInjector.inject(text: polishResult.polishedText) { _ in
                            // Restore the original clipboard (before our Cmd+C), after TextInjector's own restore
                            DispatchQueue.main.asyncAfter(deadline: .now() + 0.3) {
                                TextInjector.restoreClipboard(originalClipboard)
                                wLog("Polish: clipboard restored")
                                self.soundManager.playStop()
                                self.recordingOverlay.hide()
                            }
                        }

                        self.polishStore.saveResult(polishResult, app: appInfo["name"] ?? "")

                    case .failure(let error):
                        wLog("Polish failed: \(error.userMessage)")
                        TextInjector.restoreClipboard(originalClipboard)
                        self.recordingOverlay.showError(message: error.userMessage)
                    }
                }
            }
        }
    }

    // MARK: - Auto-Learn

    private func autoLearnWords(asrText: String, formattedText: String) {
        let asrWords = Set(asrText.lowercased().split(separator: " ").map(String.init))
        let formattedWords = formattedText.split(separator: " ").map(String.init)

        var wordsToLearn: [String] = []
        for word in formattedWords {
            let lowered = word.lowercased()
            // Skip if word exists in ASR output (not a correction)
            guard !asrWords.contains(lowered) else { continue }
            // Only learn capitalized words (likely proper nouns) that are > 2 chars
            let cleaned = word.trimmingCharacters(in: .punctuationCharacters)
            guard cleaned.count > 2,
                  cleaned.first?.isUppercase == true else { continue }
            wordsToLearn.append(cleaned)
        }

        if !wordsToLearn.isEmpty {
            dictionaryStore.addAutoLearnedWords(phrases: wordsToLearn)
            wLog("Auto-learned \(wordsToLearn.count) words")
        }
    }

    // MARK: - Auto-Polish

    /// Hard ceiling on auto-polish. If the polish call hangs (network drop,
    /// model rate-limited, etc.) we fall back to injecting the original
    /// transcript rather than parking the pill in Processing forever.
    private static let autoPolishWatchdogSeconds: TimeInterval = 30

    private func autoPolishText(_ text: String) {
        let activeInstructions = settings.activePolishInstructions
        guard !activeInstructions.isEmpty else { return }

        // SafeCompletion gate: exactly one terminal action (inject polished
        // OR inject original) regardless of whether polish completed normally
        // or the watchdog fired.
        let gate = SafeCompletion<(text: String, isPolished: Bool)> { [weak self] outcome in
            guard let self = self else { return }
            DispatchQueue.main.async {
                self.recordingOverlay.showInserting()
                self.textInjector.inject(text: outcome.text) { _ in
                    DispatchQueue.main.async { self.recordingOverlay.hide() }
                }
                if outcome.isPolished {
                    wLog("Auto-polish complete: \(outcome.text.count) chars")
                }
            }
        }

        let watchdog = DispatchWorkItem {
            wLog("Auto-polish watchdog fired — injecting original text")
            gate.fire((text: text, isPolished: false))
        }
        DispatchQueue.global().asyncAfter(deadline: .now() + Self.autoPolishWatchdogSeconds, execute: watchdog)

        DispatchQueue.global(qos: .userInitiated).async { [weak self] in
            self?.polishService.polish(text: text, instructions: activeInstructions) { [weak self] result in
                watchdog.cancel()
                guard let self = self else { return }
                switch result {
                case .success(let polishResult):
                    gate.fire((text: polishResult.polishedText, isPolished: true))
                    self.polishStore.saveResult(polishResult)
                case .failure(let error):
                    wLog("Auto-polish failed: \(error.userMessage) — injecting original text")
                    gate.fire((text: text, isPolished: false))
                }
            }
        }
    }

    // MARK: - Wispr Flow session watcher

    /// Watch Wispr Flow's session directory so Lightning picks up sign-ins that land in Wispr Flow.
    /// When the user clicks "Sign In with Google", both apps share the wispr-flow:// scheme —
    /// whichever is foregrounded handles the callback. If Wispr Flow wins, it writes its session file
    /// and this watcher immediately migrates it into Lightning's own session.
    private func startWisprFlowSessionWatcher() {
        let dirURL = Session.wisprFlowSessionURL.deletingLastPathComponent()
        try? FileManager.default.createDirectory(at: dirURL, withIntermediateDirectories: true)

        let fd = open(dirURL.path, O_EVTONLY)
        guard fd >= 0 else {
            NSLog("Wispr Lightning: Could not watch Wispr Flow session directory")
            return
        }

        let source = DispatchSource.makeFileSystemObjectSource(
            fileDescriptor: fd,
            eventMask: [.write, .rename],
            queue: DispatchQueue.main
        )
        source.setEventHandler { [weak self] in
            guard let self = self else { return }
            // Only migrate if we don't have a valid session of our own.
            guard !self.session.isValid else { return }
            // The .write event fires mid-write — the file can be empty or
            // contain partial JSON. Retry a few times with a short delay so
            // we don't waste the migration on the half-written state.
            self.attemptWisprFlowSessionMigration(attempt: 1, maxAttempts: 5)
        }
        source.setCancelHandler { close(fd) }
        source.resume()
        wisprFlowSessionWatcher = source
    }

    private func attemptWisprFlowSessionMigration(attempt: Int, maxAttempts: Int) {
        if session.load() {
            NSLog("Wispr Lightning: Picked up session from Wispr Flow (%@)", session.userEmail ?? "unknown")
            NotificationCenter.default.post(name: .sessionChanged, object: nil)
            statusBarController.updateMenu()
            return
        }
        guard attempt < maxAttempts else {
            wLog("Wispr Flow session.json still unreadable after \(maxAttempts) attempts")
            return
        }
        // Exponential-ish backoff: 50, 150, 400, 900ms.
        let delaySec = Double(attempt * attempt) * 0.05 + 0.05
        DispatchQueue.main.asyncAfter(deadline: .now() + delaySec) { [weak self] in
            guard let self else { return }
            guard !self.session.isValid else { return }
            self.attemptWisprFlowSessionMigration(attempt: attempt + 1, maxAttempts: maxAttempts)
        }
    }

    // MARK: - Onboarding

    func showOnboarding() {
        if onboardingController == nil {
            onboardingController = OnboardingWindowController(settings: settings, onCompleted: { [weak self] in
                guard let self else { return }
                wLog("Onboarding completed")
                self.onboardingController = nil
            })
        }
        onboardingController?.show()
    }

    // MARK: - Provider selection

    private var activeVendor: DictationVendor = .wisprFlow

    private func currentVendor() -> DictationVendor {
        return DictationVendor(rawValue: settings.activeVendor) ?? .wisprFlow
    }

    private static func makeProvider(vendor: DictationVendor,
                                     session: Session,
                                     settings: AppSettings,
                                     openRouterModelOverride: String? = nil) -> DictationProvider {
        switch vendor {
        case .wisprFlow:
            return WisprFlowProvider(session: session, settings: settings)
        case .openRouter:
            return OpenRouterProvider(settings: settings, modelOverride: openRouterModelOverride)
        case .claudeVoice:
            return ClaudeVoiceProvider(settings: settings)
        case .deepgram:
            return DeepgramProvider(settings: settings)
        }
    }

    private func refreshProviderIfChanged() {
        let desired = currentVendor()
        guard desired != activeVendor else { return }
        wLog("Switching transcription vendor: \(activeVendor.displayName) → \(desired.displayName)")
        dictationProvider.cancel()
        dictationProvider = Self.makeProvider(vendor: desired, session: session, settings: settings)
        dictationProvider.dictionaryStore = dictionaryStore
        activeVendor = desired
    }

    /// Build the provider for `currentChainIndex`. Step 0 is the primary
    /// vendor; later steps come from `settings.fallbackChain` with their
    /// per-step `openRouterModel` override.
    private func providerForCurrentChainStep() -> DictationProvider {
        let vendor = vendorAtChainStep(currentChainIndex)
        if currentChainIndex == 0 {
            return Self.makeProvider(vendor: vendor, session: session, settings: settings)
        }
        return Self.makeProvider(
            vendor: vendor,
            session: session,
            settings: settings,
            openRouterModelOverride: settings.fallbackChain[currentChainIndex - 1].openRouterModel
        )
    }

    /// True when there's at least one more fallback step to try.
    private func hasNextChainStep() -> Bool {
        return currentChainIndex < settings.fallbackChain.count
    }

    /// Advance the chain index and rebuild the live provider. Returns the
    /// vendor we moved to, for logging.
    private func advanceChainStep() -> DictationVendor {
        currentChainIndex += 1
        let vendor = vendorAtChainStep(currentChainIndex)
        dictationProvider.cancel()
        dictationProvider = providerForCurrentChainStep()
        dictationProvider.dictionaryStore = dictionaryStore
        return vendor
    }

    func applicationWillTerminate(_ notification: Notification) {
        if let observer = settingsObserver { NotificationCenter.default.removeObserver(observer) }
        if let observer = audioDevicesObserver { NotificationCenter.default.removeObserver(observer) }
        NSWorkspace.shared.notificationCenter.removeObserver(self)
        if let monitor = cmdCommaMonitor { NSEvent.removeMonitor(monitor) }
        // Invalidate any active timers — recording, retry, rearm, processing.
        // Most paths invalidate them already, but app termination during a
        // rare state (mid-retry, mid-trailing-buffer) would otherwise leak.
        recordingTimer?.invalidate(); recordingTimer = nil
        tapDelayTimer?.invalidate(); tapDelayTimer = nil
        processingTimeoutTimer?.invalidate(); processingTimeoutTimer = nil
        rearmTimer?.invalidate(); rearmTimer = nil
        NSAppleEventManager.shared().removeEventHandler(
            forEventClass: AEEventClass(kInternetEventClass),
            andEventID: AEEventID(kAEGetURL)
        )
        hotkeyListener.stop()
        wisprFlowSessionWatcher?.cancel()
        wisprFlowSessionWatcher = nil
        // If the user quits mid-recording, drain the incremental-write queue
        // and close the file handle so the partial .pcm on disk is valid and
        // recoverable on next launch. Don't delete it — recovery scans for
        // exactly this case.
        pendingAudio?.finishWriting()
        // Drain any pending log writes on the serial log queue BEFORE closing
        // the file. Otherwise a wLog call in-flight from a background thread
        // (audio, WS receive, settings observer) lands on a closed FileHandle
        // and abort()s the process during shutdown — which was the root cause
        // of the crash users saw on Cmd+Q after switching providers.
        logQueue.sync {
            try? logFile?.close()
            logFile = nil
        }
        audioRecorder.cleanup()
        historyStore.close()
        dbManager.close()
    }
}

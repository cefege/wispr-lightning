import AppKit
import ApplicationServices

private let logFile: FileHandle? = {
    let path = FileManager.default.homeDirectoryForCurrentUser
        .appendingPathComponent("Library/Logs/WisprLightning.log").path
    FileManager.default.createFile(atPath: path, contents: nil)
    return FileHandle(forWritingAtPath: path)
}()

private let logQueue = DispatchQueue(label: "com.wisprlightning.log")
private let logDateFormatter: ISO8601DateFormatter = {
    let f = ISO8601DateFormatter()
    return f
}()

func wLog(_ message: String) {
    logQueue.async {
        let ts = logDateFormatter.string(from: Date())
        let line = "[\(ts)] \(message)\n"
        logFile?.seekToEndOfFile()
        logFile?.write(line.data(using: .utf8) ?? Data())
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
    private var transcriptionClient: TranscriptionClient!
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
    private var tapDelayTimer: Timer?
    private var processingTimeoutTimer: Timer?
    private var rearmTimer: Timer?
    private var settingsObserver: NSObjectProtocol?
    private var audioDevicesObserver: NSObjectProtocol?
    private var cmdCommaMonitor: Any?
    private var pendingPackets: [Data]?
    private var pendingAudioFileURL: URL?
    private var pendingAppInfo: [String: String]?
    private var pendingOcrContext: [String]?
    private var pendingAxContext: [String]?
    private var currentRetryAttempt = 0
    private var isTranscribing = false
    private static let maxAutoRetries = 2
    private static let pendingAudioDir: URL = {
        let dir = FileManager.default.urls(for: .applicationSupportDirectory, in: .userDomainMask).first!
            .appendingPathComponent("WisprLightning/PendingAudio")
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
        transcriptionClient = TranscriptionClient(session: session, settings: settings)
        transcriptionClient.dictionaryStore = dictionaryStore
        polishService = PolishService(session: session, settings: settings)
        textInjector = TextInjector()
        soundManager = SoundManager(settings: settings)
        musicController = MusicController(settings: settings)

        statusBarController = StatusBarController(
            session: session,
            settings: settings,
            historyStore: historyStore,
            dictionaryStore: dictionaryStore,
            notesStore: notesStore
        )

        recordingOverlay = RecordingOverlay()
        recordingOverlay.prewarm()
        toastNotification = ToastNotification()

        isVerboseLoggingEnabled = settings.verboseLogging

        settingsObserver = NotificationCenter.default.addObserver(forName: .settingsChanged, object: nil, queue: .main) { [weak self] notification in
            if let updated = notification.object as? AppSettings {
                isVerboseLoggingEnabled = updated.verboseLogging
            }
            guard let self = self else { return }
            self.rearmMicrophone()
        }

        audioDevicesObserver = NotificationCenter.default.addObserver(forName: .audioDevicesChanged, object: nil, queue: .main) { [weak self] _ in
            guard let self = self else { return }
            self.statusBarController.updateMenu()

            if self.isRecording {
                if let targetUID = self.settings.micDeviceUID {
                    let devices = AudioRecorder.listInputDevices()
                    if !devices.contains(where: { $0.uid == targetUID }) {
                        wLog("Target mic '\(self.settings.micDeviceName ?? targetUID)' disconnected during recording")
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
            onPress: { [weak self] in self?.onHotkeyPress() },
            onRelease: { [weak self] in self?.onHotkeyRelease() }
        )
        hotkeyListener.onPolishPress = { [weak self] in self?.onPolishHotkeyPress() }
        hotkeyListener.start()

        statusBarController.onTogglePause = { [weak self] in
            guard let self = self else { return }
            self.hotkeyListener.setPaused(!self.hotkeyListener.isPaused)
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

        // Prompt for Accessibility if not yet granted (required for text injection)
        let trusted = AXIsProcessTrustedWithOptions(
            [kAXTrustedCheckOptionPrompt.takeUnretainedValue(): true] as CFDictionary
        )
        if !trusted {
            wLog("Accessibility not granted — text injection will not work until enabled in System Settings > Privacy & Security > Accessibility")
        } else {
            wLog("Accessibility: trusted")
        }

        wLog("Ready — press \(settings.hotkeyLabel) to start dictating")

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
        recordingState = .idle
        lastPressTime = nil

        recordingTimer?.invalidate()
        recordingTimer = nil
        rearmTimer?.invalidate()
        rearmTimer = nil
        recordingStartTime = nil
        hotkeyListener.resetState()

        _ = audioRecorder.stop() // discard packets
        transcriptionClient.cancelPrewarmedConnection()
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
                // Slow second press: treat as stop
                stopRecordingSession()
            }

        case .recording:
            // Third press: stop hands-free recording
            stopRecordingSession()
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
        let startResult = audioRecorder.start()
        switch startResult {
        case .started:
            break
        case .startedWithFallback:
            wLog("Recording started with fallback mic (requested device unavailable)")
        case .failed(let reason):
            wLog("Failed to start recording: \(reason)")
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
        transcriptionClient.prewarmConnection()

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
        // In locked (hands-free) mode, key release does nothing — third press stops recording
        guard recordingState == .listening else { return }

        let heldDuration = lastPressTime.map { Date().timeIntervalSince($0) } ?? 1.0
        if heldDuration >= AppDelegate.lockDebounceInterval {
            // Long hold (PTT): stop after a short trailing buffer to capture tail-end of speech
            tapDelayTimer?.invalidate()
            tapDelayTimer = Timer.scheduledTimer(withTimeInterval: AppDelegate.trailingBufferInterval, repeats: false) { [weak self] _ in
                guard let self = self, self.recordingState == .listening else { return }
                self.stopRecordingSession()
            }
        } else {
            // Quick tap: wait for potential second press before stopping.
            // Fire at exactly lockDebounceInterval from the first press.
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

        let packets = audioRecorder.stop()
        soundManager.playStop()
        statusBarController.setRecording(false)

        guard packets.count >= 5 else {
            transcriptionClient.cancelPrewarmedConnection()
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
        scheduleProcessingTimeout()

        // Save audio to disk in background — survives crashes and failed retries
        let packetsToSave = packets
        DispatchQueue.global(qos: .utility).async { [weak self] in
            self?.pendingAudioFileURL = self?.saveAudioToDisk(packetsToSave)
        }

        DispatchQueue.global(qos: .userInitiated).async { [weak self, count = packets.count] in
            guard let self = self else { return }
            // Drain OCR/AX queues here — avoids blocking main thread
            self.pendingOcrContext = self.ocrQueue.sync {
                let ctx = self.cachedOCRContext
                self.cachedOCRContext = []
                return ctx
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

        transcriptionClient.transcribe(packets: packets, appInfo: appInfo, ocrContext: ocrContext, axContext: axContext) { [weak self] result in
            guard let self = self else { return }

            switch result {
            case .success(let transcriptResult):
                self.isTranscribing = false
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
                        if self.settings.autoPolish && self.settings.polishEnabled
                            && !activeInstructions.isEmpty {
                            // Auto-polish will inject the final text — skip raw injection
                            // Keep overlay in Processing state while polish runs
                        } else {
                            self.textInjector.inject(text: displayText) { _ in
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

                        // Auto-polish after dictation
                        if self.settings.autoPolish && self.settings.polishEnabled {
                            self.autoPolishText(displayText)
                        }
                    } else {
                        wLog("Empty transcription result")
                        self.recordingOverlay.showError(message: TranscriptionError.emptyResult.userMessage)
                    }
                }

            case .failure(let error):
                self.isTranscribing = false

                if error.isRetryable && self.currentRetryAttempt < Self.maxAutoRetries {
                    self.currentRetryAttempt += 1
                    let attempt = self.currentRetryAttempt
                    let maxAttempts = Self.maxAutoRetries + 1
                    wLog("Transcription failed (retryable): \(error.userMessage) — auto-retry \(attempt)/\(Self.maxAutoRetries)")

                    DispatchQueue.main.async {
                        self.recordingOverlay.showRetrying(attempt: attempt + 1, maxAttempts: maxAttempts)
                    }

                    // Pre-warm connection during retry delay so TCP+TLS handshake overlaps with wait
                    self.transcriptionClient.prewarmConnection()

                    DispatchQueue.global(qos: .userInitiated).asyncAfter(deadline: .now() + 1.5) { [weak self] in
                        self?.attemptTranscription()
                    }
                } else if error.isRetryable {
                    // Auto-retries exhausted — show persistent retry UI
                    wLog("Transcription failed after \(Self.maxAutoRetries) retries: \(error.userMessage)")
                    self.resumeMusicInBackground()

                    DispatchQueue.main.async {
                        self.recordingOverlay.showRetryableError(
                            message: error.userMessage,
                            onRetry: { [weak self] in self?.retryTranscription() },
                            onDismiss: { [weak self] in self?.dismissRetry() }
                        )
                    }
                } else {
                    // Non-retryable error — still show persistent retry UI so audio is never lost
                    wLog("Transcription failed (non-retryable): \(error.userMessage)")
                    self.resumeMusicInBackground()

                    DispatchQueue.main.async {
                        self.recordingOverlay.showRetryableError(
                            message: error.userMessage,
                            onRetry: { [weak self] in self?.retryTranscription() },
                            onDismiss: { [weak self] in self?.dismissRetry() }
                        )
                    }
                }
            }
        }
    }

    private func retryTranscription() {
        currentRetryAttempt = 0
        recordingOverlay.showProcessing()
        // Pre-warm connection so TCP+TLS handshake starts immediately
        transcriptionClient.prewarmConnection()
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
                onDismiss: { [weak self] in self?.dismissRetry() }
            )
        }
    }

    private func dismissRetry() {
        clearPendingTranscription()
        recordingOverlay.hide()
    }

    private func resumeMusicInBackground() {
        DispatchQueue.global(qos: .utility).async { [weak self] in
            self?.musicController.resumeMusic()
        }
    }

    private func clearPendingTranscription() {
        processingTimeoutTimer?.invalidate()
        processingTimeoutTimer = nil
        // Delete saved audio file on successful transcription
        if let url = pendingAudioFileURL {
            try? FileManager.default.removeItem(at: url)
            wLog("Deleted saved audio: \(url.lastPathComponent)")
        }
        pendingPackets = nil
        pendingAudioFileURL = nil
        pendingAppInfo = nil
        pendingOcrContext = nil
        pendingAxContext = nil
        currentRetryAttempt = 0
        isTranscribing = false
        transcriptionClient.clearEncodingCache()
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

        // Only recover files from the last 24 hours
        if let created = (try? mostRecent.resourceValues(forKeys: [.creationDateKey]).creationDate),
           Date().timeIntervalSince(created) > 86400 {
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
        pendingAudioFileURL = mostRecent
        pendingAppInfo = ["name": "Unknown", "bundle_id": "", "type": "other", "url": ""]
        currentRetryAttempt = 0

        DispatchQueue.main.async {
            self.recordingOverlay.showRetryableError(
                message: "Recovered unsent recording",
                onRetry: { [weak self] in self?.retryTranscription() },
                onDismiss: { [weak self] in self?.dismissRetry() }
            )
        }

        // Clean up any other old files
        for file in pcmFiles where file != mostRecent {
            try? FileManager.default.removeItem(at: file)
        }
    }

    // MARK: - Polish

    private func onPolishHotkeyPress() {
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

    private func autoPolishText(_ text: String) {
        let activeInstructions = settings.activePolishInstructions
        guard !activeInstructions.isEmpty else { return }

        DispatchQueue.global(qos: .userInitiated).async { [weak self] in
            self?.polishService.polish(text: text, instructions: activeInstructions) { [weak self] result in
                guard let self = self else { return }
                switch result {
                case .success(let polishResult):
                    DispatchQueue.main.async {
                        self.textInjector.inject(text: polishResult.polishedText) { _ in
                            DispatchQueue.main.async { self.recordingOverlay.hide() }
                        }
                        wLog("Auto-polish complete: \(polishResult.polishedText.count) chars")
                    }
                    self.polishStore.saveResult(polishResult)
                case .failure(let error):
                    wLog("Auto-polish failed: \(error.userMessage) — injecting original text")
                    DispatchQueue.main.async {
                        self.textInjector.inject(text: text) { _ in
                            DispatchQueue.main.async { self.recordingOverlay.hide() }
                        }
                    }
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
            // Only migrate if we don't have a valid session of our own
            guard !self.session.isValid else { return }
            if self.session.load() {
                NSLog("Wispr Lightning: Picked up session from Wispr Flow (%@)", self.session.userEmail ?? "unknown")
                NotificationCenter.default.post(name: .sessionChanged, object: nil)
                self.statusBarController.updateMenu()
            }
        }
        source.setCancelHandler { close(fd) }
        source.resume()
        wisprFlowSessionWatcher = source
    }

    func applicationWillTerminate(_ notification: Notification) {
        if let observer = settingsObserver { NotificationCenter.default.removeObserver(observer) }
        if let observer = audioDevicesObserver { NotificationCenter.default.removeObserver(observer) }
        NSWorkspace.shared.notificationCenter.removeObserver(self)
        if let monitor = cmdCommaMonitor { NSEvent.removeMonitor(monitor) }
        hotkeyListener.stop()
        wisprFlowSessionWatcher?.cancel()
        wisprFlowSessionWatcher = nil
        logFile?.closeFile()
        audioRecorder.cleanup()
        historyStore.close()
        dbManager.close()
    }
}

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
    private var isRecording = false
    private var recordingOverlay: RecordingOverlay!
    private var toastNotification: ToastNotification!
    private var recordingTimer: Timer?
    private var recordingStartTime: Date?
    private var cachedOCRContext: [String] = []
    private var cachedAXContext: [String] = []
    private var pendingPackets: [Data]?
    private var pendingAppInfo: [String: String]?
    private var pendingOcrContext: [String]?
    private var pendingAxContext: [String]?
    private var currentRetryAttempt = 0
    private var isTranscribing = false
    private static let maxAutoRetries = 2
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
        toastNotification = ToastNotification()

        let hasSession = session.load()
        if !hasSession {
            NSLog("Wispr Lightning: No session found. Sign in via Settings or log in to Wispr Flow first.")
        } else {
            NSLog("Wispr Lightning: Session loaded for %@", session.userEmail ?? "unknown")
        }

        statusBarController.updateMenu()

        // Auto-open main window on first launch if not signed in
        if !hasSession {
            statusBarController.openMainWindow()
        }

        hotkeyListener = HotkeyListener(
            settings: settings,
            onPress: { [weak self] in self?.onHotkeyPress() },
            onRelease: { [weak self] in self?.onHotkeyRelease() }
        )
        hotkeyListener.onPolishPress = { [weak self] in self?.onPolishHotkeyPress() }
        hotkeyListener.start()

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
        NSEvent.addLocalMonitorForEvents(matching: .keyDown) { [weak self] event in
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
        isRecording = false

        recordingTimer?.invalidate()
        recordingTimer = nil
        recordingStartTime = nil

        _ = audioRecorder.stop() // discard packets
        transcriptionClient.cancelPrewarmedConnection()
        clearPendingTranscription()

        DispatchQueue.main.async {
            self.statusBarController.setRecording(false)
            self.recordingOverlay.hide()
        }

        resumeMusicInBackground()
    }

    private func onHotkeyPress() {
        guard !isRecording else { return }
        isRecording = true
        soundManager.playStart()
        audioRecorder.start()
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

        DispatchQueue.main.async {
            self.statusBarController.setRecording(true)
            self.recordingOverlay.show()

            // Start 1-second repeating timer for duration tracking
            self.recordingTimer = Timer.scheduledTimer(withTimeInterval: 1.0, repeats: true) { [weak self] _ in
                self?.onRecordingTimerTick()
            }
        }
        wLog("Recording started")
    }

    private func onRecordingTimerTick() {
        guard let startTime = recordingStartTime else { return }
        let elapsed = Int(Date().timeIntervalSince(startTime))

        recordingOverlay.updateElapsed(elapsed)

        let maxSec = settings.creatorMode ? Constants.creatorMaxRecordingSeconds : Constants.maxRecordingSeconds
        let warnSec = settings.creatorMode ? Constants.creatorWarningSeconds : Constants.warningSeconds
        let finalSec = settings.creatorMode ? Constants.creatorFinalWarningSeconds : Constants.finalWarningSeconds

        if elapsed >= maxSec {
            wLog("Max recording duration reached (\(maxSec)s), auto-stopping")
            onHotkeyRelease()
        } else if elapsed >= finalSec {
            recordingOverlay.showFinalWarning()
        } else if elapsed >= warnSec {
            recordingOverlay.showWarning()
        }
    }

    private func onHotkeyRelease() {
        guard isRecording else { return }
        isRecording = false

        // Stop recording timer
        recordingTimer?.invalidate()
        recordingTimer = nil
        recordingStartTime = nil

        let packets = audioRecorder.stop()
        soundManager.playStop()
        DispatchQueue.main.async {
            self.statusBarController.setRecording(false)
        }

        guard packets.count >= 5 else {
            wLog("Too short (\(packets.count) packets), ignoring")
            transcriptionClient.cancelPrewarmedConnection()
            DispatchQueue.main.async { self.recordingOverlay.hide() }
            musicController.resumeMusic()
            return
        }

        // Show processing indicator while transcribing
        DispatchQueue.main.async { self.recordingOverlay.showProcessing() }

        let appInfo = AppInfoDetector.getFrontmostAppInfo()
        let ocrContext = ocrQueue.sync {
            let ctx = cachedOCRContext
            cachedOCRContext = []
            return ctx
        }
        let axContext = axQueue.sync {
            let ctx = cachedAXContext
            cachedAXContext = []
            return ctx
        }
        wLog("Recording stopped — \(packets.count) packets (\(String(format: "%.1f", Double(packets.count) * 0.04))s), transcribing with \(ocrContext.count) OCR lines...")

        // Store pending data for retry support
        pendingPackets = packets
        pendingAppInfo = appInfo
        pendingOcrContext = ocrContext
        pendingAxContext = axContext
        currentRetryAttempt = 0

        DispatchQueue.global(qos: .userInitiated).async { [weak self] in
            self?.attemptTranscription()
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

                        self.textInjector.inject(text: displayText) { _ in
                            DispatchQueue.main.async {
                                self.recordingOverlay.hide()
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
                    // Non-retryable error — clear state, show auto-dismiss error
                    self.clearPendingTranscription()
                    wLog("Transcription failed (non-retryable): \(error.userMessage)")
                    self.resumeMusicInBackground()

                    DispatchQueue.main.async {
                        self.recordingOverlay.showError(message: error.userMessage)
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
        DispatchQueue.global(qos: .userInitiated).async { [weak self] in
            self?.attemptTranscription()
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
        pendingPackets = nil
        pendingAppInfo = nil
        pendingOcrContext = nil
        pendingAxContext = nil
        currentRetryAttempt = 0
        isTranscribing = false
        transcriptionClient.clearEncodingCache()
    }

    // MARK: - Polish

    private func onPolishHotkeyPress() {
        guard settings.polishEnabled else { return }

        guard let selectedText = TextInjector.readSelectedText() else {
            wLog("Polish: no text selected")
            DispatchQueue.main.async {
                self.recordingOverlay.showError(message: "Select text to polish")
            }
            return
        }

        let activeInstructions = settings.activePolishInstructions
        guard !activeInstructions.isEmpty else {
            wLog("Polish: no instructions enabled")
            return
        }

        wLog("Polish: processing \(selectedText.count) chars with \(activeInstructions.count) instructions")
        DispatchQueue.main.async { self.recordingOverlay.showProcessing() }

        let appInfo = AppInfoDetector.getFrontmostAppInfo()

        DispatchQueue.global(qos: .userInitiated).async { [weak self] in
            self?.polishService.polish(text: selectedText, instructions: activeInstructions) { [weak self] result in
                guard let self = self else { return }

                DispatchQueue.main.async {
                    switch result {
                    case .success(let polishResult):
                        wLog("Polish complete: \(polishResult.polishedText.count) chars in \(String(format: "%.1f", polishResult.processingTime))s")

                        self.textInjector.inject(text: polishResult.polishedText) { _ in
                            DispatchQueue.main.async {
                                self.recordingOverlay.hide()
                            }
                        }

                        self.polishStore.saveResult(polishResult, app: appInfo["name"] ?? "")

                    case .failure(let error):
                        wLog("Polish failed: \(error.userMessage)")
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
                if case .success(let polishResult) = result {
                    DispatchQueue.main.async {
                        self.textInjector.inject(text: polishResult.polishedText) { _ in }
                        wLog("Auto-polish complete: \(polishResult.polishedText.count) chars")
                    }
                    self.polishStore.saveResult(polishResult)
                }
            }
        }
    }

    func applicationWillTerminate(_ notification: Notification) {
        audioRecorder.cleanup()
        historyStore.close()
        dbManager.close()
    }
}

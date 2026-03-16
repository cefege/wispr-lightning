import AppKit
import ApplicationServices

private let logFile: FileHandle? = {
    let path = FileManager.default.homeDirectoryForCurrentUser
        .appendingPathComponent("Library/Logs/WisprLite.log").path
    FileManager.default.createFile(atPath: path, contents: nil)
    return FileHandle(forWritingAtPath: path)
}()

func wLog(_ message: String) {
    let ts = ISO8601DateFormatter().string(from: Date())
    let line = "[\(ts)] \(message)\n"
    logFile?.seekToEndOfFile()
    logFile?.write(line.data(using: .utf8) ?? Data())
    NSLog("Wispr Lite: %@", message)
}

class AppDelegate: NSObject, NSApplicationDelegate {
    var statusBarController: StatusBarController!
    private var session: Session!
    private var settings: AppSettings!
    private var audioRecorder: AudioRecorder!
    private var transcriptionClient: TranscriptionClient!
    private var textInjector: TextInjector!
    private var hotkeyListener: HotkeyListener!
    private var historyStore: HistoryStore!
    private var soundManager: SoundManager!
    private var musicController: MusicController!
    private var isRecording = false
    private var recordingOverlay: RecordingOverlay!
    private var toastNotification: ToastNotification!
    private var recordingTimer: Timer?
    private var recordingStartTime: Date?

    func applicationDidFinishLaunching(_ notification: Notification) {
        settings = AppSettings.load()
        session = Session()
        historyStore = HistoryStore()
        audioRecorder = AudioRecorder(settings: settings)
        transcriptionClient = TranscriptionClient(session: session)
        textInjector = TextInjector()
        soundManager = SoundManager(settings: settings)
        musicController = MusicController(settings: settings)

        statusBarController = StatusBarController(
            session: session,
            settings: settings,
            historyStore: historyStore
        )

        recordingOverlay = RecordingOverlay()
        toastNotification = ToastNotification()

        if !session.load() {
            NSLog("Wispr Lite: No session found. Sign in via Settings or log in to Wispr Flow first.")
        } else {
            NSLog("Wispr Lite: Session loaded for %@", session.userEmail ?? "unknown")
        }

        statusBarController.updateMenu()

        hotkeyListener = HotkeyListener(
            settings: settings,
            onPress: { [weak self] in self?.onHotkeyPress() },
            onRelease: { [weak self] in self?.onHotkeyRelease() }
        )
        hotkeyListener.start()

        if settings.showInDock {
            NSApp.setActivationPolicy(.regular)
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

    @objc func handleURLEvent(_ event: NSAppleEventDescriptor, withReplyEvent reply: NSAppleEventDescriptor) {
        guard let urlString = event.paramDescriptor(forKeyword: AEKeyword(keyDirectObject))?.stringValue,
              let url = URL(string: urlString) else { return }
        NSLog("Wispr Lite: Received URL callback: %@", urlString)
        AuthService.handleCallback(url: url, session: session) { success in
            DispatchQueue.main.async {
                if success {
                    NSLog("Wispr Lite: Sign in successful")
                    NotificationCenter.default.post(name: .sessionChanged, object: nil)
                } else {
                    NSLog("Wispr Lite: Sign in failed")
                }
            }
        }
    }

    private func onHotkeyPress() {
        guard !isRecording else { return }
        isRecording = true
        soundManager.playStart()
        musicController.pauseMusic()
        audioRecorder.start()
        recordingStartTime = Date()

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

        if elapsed >= Constants.maxRecordingSeconds {
            wLog("Max recording duration reached (\(Constants.maxRecordingSeconds)s), auto-stopping")
            onHotkeyRelease()
        } else if elapsed >= Constants.finalWarningSeconds {
            recordingOverlay.showFinalWarning()
        } else if elapsed >= Constants.warningSeconds {
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
            DispatchQueue.main.async { self.recordingOverlay.hide() }
            musicController.resumeMusic()
            return
        }

        // Show processing indicator while transcribing
        DispatchQueue.main.async { self.recordingOverlay.showProcessing() }

        let appInfo = AppInfoDetector.getFrontmostAppInfo()
        wLog("Recording stopped — \(packets.count) packets (\(String(format: "%.1f", Double(packets.count) * 0.04))s), transcribing...")

        // Capture OCR context off main thread to avoid blocking UI
        DispatchQueue.global(qos: .userInitiated).async { [weak self] in
            let ocrContext = ScreenCaptureContext.captureOCRContext()
            wLog("OCR context: \(ocrContext.count) lines captured")

            self?.transcriptionClient.transcribe(packets: packets, appInfo: appInfo, ocrContext: ocrContext) { [weak self] result in
            guard let self = self else { return }

            DispatchQueue.main.async {
                self.musicController.resumeMusic()

                if let result = result {
                    let displayText = result.formattedText ?? result.asrText ?? ""
                    if !displayText.isEmpty {
                        wLog("Injecting: \(String(displayText.prefix(80)))")
                        let wordCount = displayText.split(separator: " ").count

                        self.textInjector.inject(text: displayText) { _ in
                            DispatchQueue.main.async {
                                self.recordingOverlay.hide()
                                self.toastNotification.show(wordCount: wordCount)
                            }
                        }

                        self.historyStore.addEntry(result: result, appInfo: appInfo)
                        self.statusBarController.setLastTranscription(displayText)
                    } else {
                        wLog("Empty transcription result")
                        self.recordingOverlay.showError(message: "No transcription")
                    }
                } else {
                    wLog("No transcription result (nil)")
                    self.recordingOverlay.showError(message: "No transcription")
                }
            }
            }
        }
    }

    func applicationWillTerminate(_ notification: Notification) {
        audioRecorder.cleanup()
        historyStore.close()
    }
}

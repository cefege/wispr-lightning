import AppKit

class RecordingOverlay {
    private var panel: NSPanel?
    private var dotView: NSView?
    private var timeLabel: NSTextField?
    private var effectView: NSVisualEffectView?
    private var mainLabel: NSTextField?
    private var spinner: NSProgressIndicator?
    private var warningState = 0
    private var errorDismissTimer: Timer?
    private var retryButton: NSButton?
    private var saveButton: NSButton?
    private var dismissButton: NSButton?
    private var onRetryAction: (() -> Void)?
    private var onSaveAction: (() -> Void)?
    private var onDismissAction: (() -> Void)?
    /// Wired once by AppDelegate. Invoked when the user clicks the hover-
    /// revealed ✕ during Listening or Recording — aborts capture, discards
    /// audio, dismisses the pill.
    var onCancelAction: (() -> Void)?
    private var cancelButton: NSButton?
    /// True while the pill is in a state where the user-cancel ✕ should
    /// appear on hover (Listening or Recording). False otherwise so hover
    /// during Processing/Error/Retrying does nothing.
    private var isRecordingMode = false
    private var currentPanelWidth: CGFloat = 120
    /// Container view holding the VU bars. Sits in the stack between the dot
    /// and the label. Hidden in non-recording states (Processing, Error,
    /// Retrying) where the dot is also hidden.
    private var levelBarsView: NSView?
    private var levelBars: [CALayer] = []
    /// Rolling buffer of recent audio levels — newest on the right. Index N
    /// drives bar N's height after smoothing.
    private var levelBuffer: [Float] = Array(repeating: 0, count: 5)
    /// Per-bar smoothed level so each bar eases toward its buffer value
    /// instead of snapping at update rate (~25 Hz). Same length as `levelBars`.
    private var displayedBarLevels: [Float] = Array(repeating: 0, count: 5)
    private var levelLastUpdate: Date?

    private static let vuBarCount = 18
    /// Pixel dimensions of the VU strip. Tuned so the band reads at a glance
    /// without ballooning the pill back to its old debug-tool proportions.
    private static let vuBarWidth: CGFloat = 3
    private static let vuBarSpacing: CGFloat = 2
    private static let vuBarMinHeight: CGFloat = 3
    private static let vuBarMaxHeight: CGFloat = 20
    private static let vuStripHeight: CGFloat = 22
    private static let pillHeight: CGFloat = 36
    /// Single recording-state pill width. The ✕ cancel button is an overlay
    /// (not in the stack) so the band stays centered whether the cancel is
    /// visible or not — no layout jitter on hover.
    private static let recordingPillWidth: CGFloat = 130
    private static let cancelButtonSize: CGFloat = 20
    private static let cancelButtonRightMargin: CGFloat = 8

    /// Call at app launch to build the panel before the first keypress.
    func prewarm() {
        guard panel == nil else { return }
        buildPanel()
    }

    func show() {
        if panel != nil {
            // Reset state for new recording
            warningState = 0
            timeLabel?.isHidden = true
            retryButton?.isHidden = true
            saveButton?.isHidden = true
            saveButton?.title = "Save"
            saveButton?.isEnabled = true
            dismissButton?.isHidden = true
            onRetryAction = nil
            onSaveAction = nil
            onDismissAction = nil
            effectView?.layer?.backgroundColor = nil
            // Listening / Recording states show ONLY the big VU band — no
            // dot, no "Listening" text. The band is the indicator: bars
            // moving = mic alive, bars jumping = voice detected.
            dotView?.isHidden = true
            mainLabel?.isHidden = true
            levelBarsView?.isHidden = false
            cancelButton?.alphaValue = 0   // hover-reveal
            spinner?.stopAnimation(nil)
            spinner?.isHidden = true
            isRecordingMode = true
            // Reset to the resting (red) bar color in case the prior session
            // ended in showLocked() with green bars.
            for bar in levelBars {
                bar.backgroundColor = Theme.Colors.error.cgColor
            }
            currentPanelWidth = 0  // force resize to reposition after any state
            resetLevelBars()
            resizePanel(width: Self.recordingPillWidth)
            panel?.orderFront(nil)
            return
        }
        buildPanel()
        repositionPanel()
        panel?.orderFront(nil)
        startPulsing()
    }

    private func buildPanel() {
        let panel = NSPanel(
            contentRect: NSRect(x: 0, y: 0, width: 120, height: Self.pillHeight),
            styleMask: [.nonactivatingPanel, .fullSizeContentView],
            backing: .buffered,
            defer: false
        )
        panel.level = .floating
        panel.isOpaque = false
        panel.backgroundColor = .clear
        panel.hasShadow = true
        panel.isMovableByWindowBackground = false
        panel.collectionBehavior = [.canJoinAllSpaces, .stationary]
        panel.animationBehavior = .utilityWindow

        let effectView = HoverEffectView()
        effectView.material = .popover
        effectView.state = .active
        effectView.wantsLayer = true
        effectView.layer?.cornerRadius = Self.pillHeight / 2
        effectView.layer?.masksToBounds = true
        effectView.onMouseEntered = { [weak self] in self?.handleHoverChanged(hovering: true) }
        effectView.onMouseExited = { [weak self] in self?.handleHoverChanged(hovering: false) }

        panel.contentView = effectView
        self.effectView = effectView

        let stack = NSStackView()
        stack.orientation = .horizontal
        stack.spacing = Theme.Spacing.medium
        stack.edgeInsets = NSEdgeInsets(top: 0, left: Theme.Spacing.large, bottom: 0, right: Theme.Spacing.large)
        // Force single-item centering even when only the strip is visible —
        // the default gravity behavior was leaving the band left-aligned with
        // empty space on the right.
        stack.distribution = .equalCentering
        stack.alignment = .centerY

        let dot = NSView()
        dot.wantsLayer = true
        dot.layer?.backgroundColor = Theme.Colors.error.cgColor
        dot.layer?.cornerRadius = 5
        dot.setSize(width: 10, height: 10)
        self.dotView = dot

        // VU strip — 5 thin bars whose heights map to a rolling buffer of
        // recent RMS levels (oldest on the left, newest on the right). Always
        // shows at least the min-height baseline so the user gets visual
        // confirmation the audio path is alive even before they speak.
        let stripWidth = CGFloat(Self.vuBarCount) * Self.vuBarWidth
            + CGFloat(Self.vuBarCount - 1) * Self.vuBarSpacing
        let strip = NSView()
        strip.wantsLayer = true
        strip.setSize(width: stripWidth, height: Self.vuStripHeight)
        for i in 0..<Self.vuBarCount {
            let bar = CALayer()
            let x = CGFloat(i) * (Self.vuBarWidth + Self.vuBarSpacing)
            // anchor at bottom-center so scaling on Y grows the bar upward
            // from the baseline rather than expanding in both directions.
            bar.anchorPoint = CGPoint(x: 0.5, y: 0)
            bar.frame = CGRect(
                x: x,
                y: 0,
                width: Self.vuBarWidth,
                height: Self.vuBarMinHeight
            )
            bar.cornerRadius = Self.vuBarWidth / 2
            bar.backgroundColor = Theme.Colors.error.cgColor
            strip.layer?.addSublayer(bar)
            levelBars.append(bar)
        }
        self.levelBarsView = strip

        let spin = NSProgressIndicator()
        spin.style = .spinning
        spin.controlSize = .small
        spin.isIndeterminate = true
        spin.isHidden = true
        spin.setSize(width: 16, height: 16)
        self.spinner = spin

        let label = NSTextField(labelWithString: "Listening")
        label.font = Theme.Fonts.body
        label.textColor = .labelColor
        self.mainLabel = label

        let tLabel = NSTextField(labelWithString: "")
        tLabel.font = Theme.Fonts.body
        tLabel.textColor = .secondaryLabelColor
        tLabel.isHidden = true
        self.timeLabel = tLabel

        let retry = NSButton(title: "Retry", target: self, action: #selector(retryButtonClicked))
        retry.bezelStyle = .rounded
        retry.controlSize = .small
        retry.font = Theme.Fonts.body
        retry.isHidden = true
        self.retryButton = retry

        let save = NSButton(title: "Save", target: self, action: #selector(saveButtonClicked))
        save.bezelStyle = .rounded
        save.controlSize = .small
        save.font = Theme.Fonts.body
        save.isHidden = true
        self.saveButton = save

        let dismiss = NSButton(title: "✕", target: self, action: #selector(dismissButtonClicked))
        dismiss.bezelStyle = .inline
        dismiss.isBordered = false
        dismiss.font = Theme.Fonts.body
        dismiss.isHidden = true
        self.dismissButton = dismiss

        // Hover-revealed cancel button for recording states. Separate from
        // dismissButton (which handles retryable-error dismissal) and NOT in
        // the stack — it overlays the trailing edge of the pill so showing/
        // hiding it doesn't shift the centered VU band. Uses the standard
        // macOS dismiss glyph (xmark.circle.fill) so it reads as a button
        // rather than a stray character.
        let cancel = CancelButton()
        cancel.image = Self.makeCancelImage()
        cancel.target = self
        cancel.action = #selector(cancelButtonClicked)
        cancel.isBordered = false
        cancel.bezelStyle = .inline
        cancel.imageScaling = NSImageScaling.scaleProportionallyDown
        cancel.imagePosition = .imageOnly
        cancel.title = ""
        cancel.alphaValue = 0
        cancel.toolTip = "Cancel recording"
        cancel.translatesAutoresizingMaskIntoConstraints = true
        cancel.autoresizingMask = NSView.AutoresizingMask.minXMargin   // stays trailing on width change
        cancel.frame = NSRect(
            x: Self.recordingPillWidth - Self.cancelButtonSize - Self.cancelButtonRightMargin,
            y: (Self.pillHeight - Self.cancelButtonSize) / 2,
            width: Self.cancelButtonSize,
            height: Self.cancelButtonSize
        )
        self.cancelButton = cancel

        stack.addArrangedSubview(dot)
        stack.addArrangedSubview(strip)
        stack.addArrangedSubview(spin)
        stack.addArrangedSubview(label)
        stack.addArrangedSubview(tLabel)
        stack.addArrangedSubview(retry)
        stack.addArrangedSubview(save)
        stack.addArrangedSubview(dismiss)

        effectView.addSubview(stack)
        stack.pinToSuperview()
        // Add cancel after the stack so its z-order puts it above the band.
        effectView.addSubview(cancel)

        self.panel = panel
    }

    func hide() {
        errorDismissTimer?.invalidate()
        errorDismissTimer = nil
        stopPulsing()
        resetLevelBars()
        isRecordingMode = false
        levelBarsView?.isHidden = true
        // Restore the resting (Listening/red) bar color so the next recording
        // doesn't start with leftover green from a locked session.
        for bar in levelBars {
            bar.backgroundColor = Theme.Colors.error.cgColor
        }
        spinner?.stopAnimation(nil)
        spinner?.isHidden = true
        // Restore dot+label visibility for next non-recording state (Processing
        // etc); show() and showLocked() re-hide them before the panel becomes
        // visible again.
        dotView?.isHidden = false
        mainLabel?.isHidden = false
        dotView?.layer?.backgroundColor = Theme.Colors.error.cgColor
        retryButton?.isHidden = true
        saveButton?.isHidden = true
        dismissButton?.isHidden = true
        cancelButton?.alphaValue = 0
        onRetryAction = nil
        onSaveAction = nil
        onDismissAction = nil
        panel?.orderOut(nil)
    }

    func showLocked() {
        warningState = 0
        effectView?.layer?.backgroundColor = nil
        // Hide dot and label — the green bars are now the sole indicator
        // that we're in locked/hands-free Recording.
        dotView?.isHidden = true
        mainLabel?.isHidden = true
        levelBarsView?.isHidden = false
        cancelButton?.alphaValue = 0   // hover-reveal
        isRecordingMode = true
        for bar in levelBars {
            bar.backgroundColor = NSColor.systemGreen.cgColor
        }
        resizePanel(width: Self.recordingPillWidth)
        panel?.orderFront(nil)
    }

    func showProcessing() {
        showSpinner(label: "Processing", width: 145)
    }

    /// Shown while text is being injected into the focused app — fast for the
    /// clipboard path, several seconds in Natural Mode at slow speed. Call
    /// before each `TextInjector.inject` so prior state (Retrying yellow,
    /// error buttons) is cleared.
    func showInserting() {
        showSpinner(label: "Inserting…", width: 145)
    }

    private func showSpinner(label: String, width: CGFloat) {
        stopPulsing()
        resetLevelBars()
        warningState = 0
        effectView?.layer?.backgroundColor = nil
        timeLabel?.isHidden = true
        dotView?.isHidden = true
        levelBarsView?.isHidden = true
        mainLabel?.isHidden = false
        retryButton?.isHidden = true
        saveButton?.isHidden = true
        dismissButton?.isHidden = true
        cancelButton?.alphaValue = 0
        isRecordingMode = false
        spinner?.isHidden = false
        spinner?.startAnimation(nil)
        mainLabel?.stringValue = label
        resizePanel(width: width)
        panel?.orderFront(nil)
    }

    func showError(message: String) {
        configureErrorState(message: message, width: 180)
        errorDismissTimer?.invalidate()
        errorDismissTimer = Timer.scheduledTimer(withTimeInterval: 3.0, repeats: false) { [weak self] _ in
            self?.hide()
        }
    }

    func updateElapsed(_ seconds: Int) {
        guard seconds >= 30 else { return }
        let minutes = seconds / 60
        let secs = seconds % 60
        var timeStr = String(format: "%d:%02d", minutes, secs)
        if warningState > 0 {
            timeStr += " ⚠️"
        }

        if timeLabel?.isHidden == true {
            timeLabel?.isHidden = false
            resizePanel(width: 200)
        }
        timeLabel?.stringValue = timeStr
    }

    func showWarning() {
        guard warningState < 1 else { return }
        warningState = 1
        effectView?.layer?.backgroundColor = NSColor.systemYellow.withAlphaComponent(0.3).cgColor
    }

    func showFinalWarning() {
        guard warningState < 2 else { return }
        warningState = 2
        effectView?.layer?.backgroundColor = NSColor.systemRed.withAlphaComponent(0.3).cgColor
    }

    func showRetryableError(message: String, onRetry: @escaping () -> Void, onSave: (() -> Void)? = nil, onDismiss: @escaping () -> Void) {
        configureErrorState(message: message, width: onSave != nil ? 300 : 260)

        onRetryAction = onRetry
        onSaveAction = onSave
        onDismissAction = onDismiss
        retryButton?.isHidden = false
        saveButton?.isHidden = onSave == nil
        dismissButton?.isHidden = false

        // No auto-dismiss timer — persistent until user acts
        errorDismissTimer?.invalidate()
        errorDismissTimer = nil
    }

    func showRetrying(attempt: Int, maxAttempts: Int) {
        stopPulsing()
        resetLevelBars()
        dotView?.isHidden = true
        levelBarsView?.isHidden = true
        mainLabel?.isHidden = false
        cancelButton?.alphaValue = 0
        isRecordingMode = false
        retryButton?.isHidden = true
        saveButton?.isHidden = true
        dismissButton?.isHidden = true
        timeLabel?.isHidden = true
        spinner?.isHidden = false
        spinner?.startAnimation(nil)
        mainLabel?.stringValue = "Retrying… (\(attempt)/\(maxAttempts))"
        effectView?.layer?.backgroundColor = NSColor.systemYellow.withAlphaComponent(0.2).cgColor
        resizePanel(width: 175)
        panel?.orderFront(nil)
    }

    @objc private func retryButtonClicked() {
        onRetryAction?()
    }

    @objc private func saveButtonClicked() {
        onSaveAction?()
        saveButton?.title = "Saved"
        saveButton?.isEnabled = false
    }

    @objc private func dismissButtonClicked() {
        onDismissAction?()
    }

    @objc private func cancelButtonClicked() {
        // Defensive: only fire when we're actually in a cancellable state.
        // The button is hidden outside isRecordingMode, but a stray click
        // race during state transition would otherwise call into a stale
        // closure. AppDelegate.cancelActiveRecording is idempotent so this
        // is belt-and-suspenders.
        guard isRecordingMode else { return }
        onCancelAction?()
    }

    /// Fade the cancel ✕ in/out when the mouse enters or exits the pill.
    /// Uses alphaValue (not isHidden) so the button never affects layout —
    /// the centered VU band stays put as the X appears and disappears.
    private func handleHoverChanged(hovering: Bool) {
        guard isRecordingMode else {
            cancelButton?.alphaValue = 0
            return
        }
        NSAnimationContext.runAnimationGroup { ctx in
            ctx.duration = 0.12
            cancelButton?.animator().alphaValue = hovering ? 1 : 0
        }
    }

    private func configureErrorState(message: String, width: CGFloat) {
        stopPulsing()
        resetLevelBars()
        spinner?.stopAnimation(nil)
        spinner?.isHidden = true
        dotView?.isHidden = true
        levelBarsView?.isHidden = true
        mainLabel?.isHidden = false
        cancelButton?.alphaValue = 0
        isRecordingMode = false
        timeLabel?.isHidden = true
        mainLabel?.stringValue = message
        effectView?.layer?.backgroundColor = NSColor.systemRed.withAlphaComponent(0.3).cgColor
        resizePanel(width: width)
        panel?.orderFront(nil)
    }

    private func repositionPanel() {
        guard let panel = panel, let screen = NSScreen.main else { return }
        let screenFrame = screen.visibleFrame
        let x = screenFrame.midX - panel.frame.width / 2
        let y = screenFrame.minY + 50
        panel.setFrameOrigin(NSPoint(x: x, y: y))
    }

    private func resizePanel(width: CGFloat) {
        guard currentPanelWidth != width,
              let panel = panel, let screen = NSScreen.main else { return }
        currentPanelWidth = width
        var frame = panel.frame
        frame.size.width = width
        let screenFrame = screen.visibleFrame
        frame.origin.x = screenFrame.midX - width / 2
        frame.origin.y = screenFrame.minY + 50
        panel.setFrame(frame, display: true)
    }

    /// Update the VU bars to reflect a 0.0–1.0 audio level. Shifts the rolling
    /// buffer left, appends the new sample on the right, and redraws all bars
    /// with per-bar smoothing so they ease between samples instead of snapping
    /// at update rate (~25 Hz). No-op when the bars are hidden (Processing/
    /// Error/Retrying).
    func updateAudioLevel(_ level: Float) {
        guard let strip = levelBarsView, !strip.isHidden, !levelBars.isEmpty else { return }

        // First level update after a quiet period: stop the opacity pulse so
        // the dot's blink doesn't fight the bar animation.
        if levelLastUpdate == nil {
            stopPulsing()
        }
        levelLastUpdate = Date()

        // Shift buffer left, append new sample on the right. Apply a mild
        // perceptual curve so quiet speech (RMS ~0.1) still nudges the bars
        // visibly instead of staying near the baseline.
        let clamped = max(0, min(1, level))
        let curved = sqrt(clamped)
        for i in 0..<(levelBuffer.count - 1) {
            levelBuffer[i] = levelBuffer[i + 1]
        }
        levelBuffer[levelBuffer.count - 1] = curved

        CATransaction.begin()
        CATransaction.setAnimationDuration(0.06)
        CATransaction.setDisableActions(false)
        for (i, bar) in levelBars.enumerated() {
            let target = levelBuffer[i]
            let smoothed = displayedBarLevels[i] * 0.5 + target * 0.5
            displayedBarLevels[i] = smoothed
            let h = Self.vuBarMinHeight + CGFloat(smoothed)
                * (Self.vuBarMaxHeight - Self.vuBarMinHeight)
            var frame = bar.frame
            frame.size.height = h
            bar.frame = frame
        }
        CATransaction.commit()
    }

    private func resetLevelBars() {
        levelBuffer = Array(repeating: 0, count: Self.vuBarCount)
        displayedBarLevels = Array(repeating: 0, count: Self.vuBarCount)
        CATransaction.begin()
        CATransaction.setDisableActions(true)
        for bar in levelBars {
            var frame = bar.frame
            frame.size.height = Self.vuBarMinHeight
            bar.frame = frame
            bar.removeAllAnimations()
        }
        CATransaction.commit()
        levelLastUpdate = nil
    }

    private func startPulsing() {
        guard let layer = dotView?.layer else { return }
        layer.removeAnimation(forKey: "pulse")

        let animation = CABasicAnimation(keyPath: "opacity")
        animation.fromValue = 1.0
        animation.toValue = 0.3
        animation.duration = 0.6
        animation.autoreverses = true
        animation.repeatCount = .infinity
        animation.timingFunction = CAMediaTimingFunction(name: .easeInEaseOut)
        layer.add(animation, forKey: "pulse")
    }

    private func stopPulsing() {
        dotView?.layer?.removeAnimation(forKey: "pulse")
        dotView?.layer?.opacity = 1.0
    }

    /// Builds the SF Symbol image for the cancel button. Tinted with the
    /// secondary label color so it reads as a UI control without competing
    /// with the band for visual weight.
    private static func makeCancelImage() -> NSImage? {
        let config = NSImage.SymbolConfiguration(pointSize: cancelButtonSize, weight: .medium)
        let img = NSImage(systemSymbolName: "xmark.circle.fill", accessibilityDescription: "Cancel recording")?
            .withSymbolConfiguration(config)
        img?.isTemplate = true
        return img
    }
}

/// NSButton subclass that brightens its content on mouse-over and supplies a
/// custom cursor. macOS doesn't give us hover styling for borderless buttons
/// out of the box; tracking-area + contentTintColor gets us there with no
/// custom drawing.
private final class CancelButton: NSButton {
    private var trackingArea: NSTrackingArea?

    override func updateTrackingAreas() {
        super.updateTrackingAreas()
        if let existing = trackingArea {
            removeTrackingArea(existing)
        }
        let area = NSTrackingArea(
            rect: bounds,
            options: [.mouseEnteredAndExited, .activeAlways, .inVisibleRect, .cursorUpdate],
            owner: self,
            userInfo: nil
        )
        addTrackingArea(area)
        trackingArea = area
        contentTintColor = .secondaryLabelColor
    }

    override func cursorUpdate(with event: NSEvent) {
        NSCursor.pointingHand.set()
    }

    override func mouseEntered(with event: NSEvent) {
        contentTintColor = .labelColor
    }

    override func mouseExited(with event: NSEvent) {
        contentTintColor = .secondaryLabelColor
    }
}

/// NSVisualEffectView subclass that forwards mouse-enter / mouse-exit events
/// to callback closures. Used by the pill to reveal the cancel ✕ on hover
/// without forcing RecordingOverlay to subclass NSView.
private final class HoverEffectView: NSVisualEffectView {
    var onMouseEntered: (() -> Void)?
    var onMouseExited: (() -> Void)?
    private var trackingArea: NSTrackingArea?

    override func updateTrackingAreas() {
        super.updateTrackingAreas()
        if let existing = trackingArea {
            removeTrackingArea(existing)
        }
        let area = NSTrackingArea(
            rect: bounds,
            options: [.mouseEnteredAndExited, .activeAlways, .inVisibleRect],
            owner: self,
            userInfo: nil
        )
        addTrackingArea(area)
        trackingArea = area
    }

    override func mouseEntered(with event: NSEvent) {
        onMouseEntered?()
    }

    override func mouseExited(with event: NSEvent) {
        onMouseExited?()
    }
}

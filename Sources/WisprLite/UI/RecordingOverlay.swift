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

    func show() {
        if panel != nil {
            // Reset state for new recording
            warningState = 0
            timeLabel?.isHidden = true
            effectView?.layer?.backgroundColor = nil
            if let mainLabel = mainLabel {
                mainLabel.stringValue = "Recording…"
            }
            resizePanel(width: 130)
            repositionPanel()
            panel?.orderFront(nil)
            startPulsing()
            return
        }

        let panel = NSPanel(
            contentRect: NSRect(x: 0, y: 0, width: 130, height: 36),
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

        let effectView = NSVisualEffectView()
        effectView.material = .popover
        effectView.state = .active
        effectView.wantsLayer = true
        effectView.layer?.cornerRadius = 18
        effectView.layer?.masksToBounds = true

        panel.contentView = effectView
        self.effectView = effectView

        let stack = NSStackView()
        stack.orientation = .horizontal
        stack.spacing = Theme.Spacing.medium
        stack.edgeInsets = NSEdgeInsets(top: 0, left: Theme.Spacing.large, bottom: 0, right: Theme.Spacing.large)

        // Red dot
        let dot = NSView()
        dot.wantsLayer = true
        dot.layer?.backgroundColor = Theme.Colors.error.cgColor
        dot.layer?.cornerRadius = 5
        dot.setSize(width: 10, height: 10)
        self.dotView = dot

        // Spinner (hidden by default, shown during processing)
        let spin = NSProgressIndicator()
        spin.style = .spinning
        spin.controlSize = .small
        spin.isIndeterminate = true
        spin.isHidden = true
        spin.setSize(width: 16, height: 16)
        self.spinner = spin

        let label = NSTextField(labelWithString: "Recording…")
        label.font = Theme.Fonts.body
        label.textColor = .labelColor
        self.mainLabel = label

        let tLabel = NSTextField(labelWithString: "")
        tLabel.font = Theme.Fonts.body
        tLabel.textColor = .secondaryLabelColor
        tLabel.isHidden = true
        self.timeLabel = tLabel

        stack.addArrangedSubview(dot)
        stack.addArrangedSubview(spin)
        stack.addArrangedSubview(label)
        stack.addArrangedSubview(tLabel)

        effectView.addSubview(stack)
        stack.pinToSuperview()

        self.panel = panel
        repositionPanel()
        panel.orderFront(nil)
        startPulsing()
    }

    func hide() {
        errorDismissTimer?.invalidate()
        errorDismissTimer = nil
        stopPulsing()
        spinner?.stopAnimation(nil)
        spinner?.isHidden = true
        dotView?.isHidden = false
        panel?.orderOut(nil)
    }

    func showProcessing() {
        stopPulsing()
        warningState = 0
        effectView?.layer?.backgroundColor = nil
        timeLabel?.isHidden = true
        dotView?.isHidden = true
        spinner?.isHidden = false
        spinner?.startAnimation(nil)
        mainLabel?.stringValue = "Processing…"
        resizePanel(width: 145)
        repositionPanel()
        panel?.orderFront(nil)
    }

    func showError(message: String) {
        spinner?.stopAnimation(nil)
        spinner?.isHidden = true
        dotView?.isHidden = true
        mainLabel?.stringValue = message
        effectView?.layer?.backgroundColor = NSColor.systemRed.withAlphaComponent(0.3).cgColor
        resizePanel(width: 180)
        repositionPanel()
        panel?.orderFront(nil)
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
            repositionPanel()
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

    private func repositionPanel() {
        guard let panel = panel, let screen = NSScreen.main else { return }
        let screenFrame = screen.visibleFrame
        let x = screenFrame.midX - panel.frame.width / 2
        let y = screenFrame.minY + 50
        panel.setFrameOrigin(NSPoint(x: x, y: y))
    }

    private func resizePanel(width: CGFloat) {
        guard let panel = panel else { return }
        var frame = panel.frame
        frame.size.width = width
        panel.setFrame(frame, display: true)
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
}

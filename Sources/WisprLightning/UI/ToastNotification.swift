import AppKit

class ToastNotification {
    private var panel: NSPanel?

    func show(wordCount: Int) {
        showToast(
            message: "Done",
            symbolName: "bolt.fill",
            tintColor: .white,
            autoDismissAfter: 1.5
        )
    }

    private func showToast(message: String, symbolName: String, tintColor: NSColor, autoDismissAfter: TimeInterval) {
        // Dismiss any existing toast
        dismiss()

        let panelWidth: CGFloat = message.count > 30 ? 340 : 120
        let panel = NSPanel(
            contentRect: NSRect(x: 0, y: 0, width: panelWidth, height: 40),
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
        effectView.layer?.cornerRadius = 12
        effectView.layer?.masksToBounds = true

        panel.contentView = effectView

        let stack = NSStackView()
        stack.orientation = .horizontal
        stack.alignment = .centerY
        stack.spacing = 6
        stack.edgeInsets = NSEdgeInsets(top: 0, left: Theme.Spacing.large, bottom: 0, right: Theme.Spacing.large)

        // Icon
        if let image = NSImage(systemSymbolName: symbolName, accessibilityDescription: nil) {
            let imageView = NSImageView(image: image)
            imageView.symbolConfiguration = .init(pointSize: 14, weight: .medium)
            imageView.contentTintColor = tintColor
            imageView.setSize(width: 18, height: 18)
            stack.addArrangedSubview(imageView)
        }

        let label = NSTextField(labelWithString: message)
        label.font = Theme.Fonts.body
        label.textColor = .labelColor
        label.alignment = .center
        stack.addArrangedSubview(label)

        effectView.addSubview(stack)
        stack.translatesAutoresizingMaskIntoConstraints = false
        NSLayoutConstraint.activate([
            stack.centerXAnchor.constraint(equalTo: effectView.centerXAnchor),
            stack.centerYAnchor.constraint(equalTo: effectView.centerYAnchor),
        ])

        // Position at bottom-center of screen
        if let screen = NSScreen.main {
            let screenFrame = screen.visibleFrame
            let x = screenFrame.midX - panelWidth / 2
            let y = screenFrame.minY + 50
            panel.setFrameOrigin(NSPoint(x: x, y: y))
        }

        self.panel = panel

        // Slide in
        panel.alphaValue = 0
        panel.orderFront(nil)
        NSAnimationContext.runAnimationGroup { context in
            context.duration = 0.25
            panel.animator().alphaValue = 1.0
        }

        // Auto-dismiss
        DispatchQueue.main.asyncAfter(deadline: .now() + autoDismissAfter) { [weak self] in
            self?.dismiss()
        }
    }

    private func dismiss() {
        guard let panel = panel else { return }
        NSAnimationContext.runAnimationGroup({ context in
            context.duration = 0.3
            panel.animator().alphaValue = 0
        }, completionHandler: { [weak self] in
            panel.orderOut(nil)
            self?.panel = nil
        })
    }
}

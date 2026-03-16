import AppKit

enum AppInfoDetector {
    private static let messagingBundleIds: Set<String> = [
        "com.slack.Slack", "com.tinyspeck.slackmacgap",
        "net.whatsapp.WhatsApp", "com.tdesktop.Telegram",
        "org.whispersystems.signal-desktop", "com.discordapp.Discord"
    ]

    private static let emailBundleIds: Set<String> = [
        "com.apple.mail", "com.microsoft.Outlook", "com.google.Gmail"
    ]

    private static let aiBundleIds: Set<String> = [
        "com.openai.chat", "com.anthropic.claudefordesktop",
        "com.todesktop.230313mzl4w4u92", "com.microsoft.VSCode"
    ]

    static func getFrontmostAppInfo() -> [String: String] {
        guard let app = NSWorkspace.shared.frontmostApplication else {
            return ["name": "", "bundle_id": "", "type": "other", "url": ""]
        }

        let bundleId = app.bundleIdentifier ?? ""
        let name = app.localizedName ?? ""

        let appType: String
        if messagingBundleIds.contains(bundleId) {
            appType = "messaging"
        } else if emailBundleIds.contains(bundleId) {
            appType = "email"
        } else if aiBundleIds.contains(bundleId) {
            appType = "ai"
        } else {
            appType = "other"
        }

        return ["name": name, "bundle_id": bundleId, "type": appType, "url": ""]
    }
}

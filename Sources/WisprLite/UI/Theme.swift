import AppKit
import SwiftUI

enum Theme {
    enum Colors {
        static let background = NSColor.windowBackgroundColor
        static let secondaryText = NSColor.secondaryLabelColor
        static let accent = NSColor.controlAccentColor
        static let error = NSColor.systemRed
        static let hintText = NSColor.tertiaryLabelColor

        // SwiftUI convenience
        static var swiftAccent: Color { Color(nsColor: accent) }
        static var swiftSecondaryText: Color { Color(nsColor: secondaryText) }
        static var swiftError: Color { Color(nsColor: error) }
        static var swiftHintText: Color { Color(nsColor: hintText) }
    }

    enum Fonts {
        static let title = NSFont.preferredFont(forTextStyle: .title3)
        static let heading = NSFont.preferredFont(forTextStyle: .headline)
        static let body = NSFont.preferredFont(forTextStyle: .body)
        static let caption = NSFont.preferredFont(forTextStyle: .subheadline)
    }

    enum Spacing {
        static let small: CGFloat = 4
        static let medium: CGFloat = 8
        static let large: CGFloat = 16
        static let xlarge: CGFloat = 24
    }
}

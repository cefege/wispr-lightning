#!/bin/bash
set -e

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
APP_NAME="Wispr Lite"
APP_BUNDLE="$APP_NAME.app"
INSTALL_DIR="/Applications"

echo "Building Wispr Lite (release)..."
cd "$SCRIPT_DIR"
swift build -c release

echo "Creating app bundle..."
rm -rf "$SCRIPT_DIR/$APP_BUNDLE"
mkdir -p "$SCRIPT_DIR/$APP_BUNDLE/Contents/MacOS"
mkdir -p "$SCRIPT_DIR/$APP_BUNDLE/Contents/Resources"

# Copy binary
cp "$SCRIPT_DIR/.build/release/WisprLite" "$SCRIPT_DIR/$APP_BUNDLE/Contents/MacOS/WisprLite"

# Copy Info.plist
cp "$SCRIPT_DIR/Resources/Info.plist" "$SCRIPT_DIR/$APP_BUNDLE/Contents/Info.plist"

# Write PkgInfo
echo -n "APPL????" > "$SCRIPT_DIR/$APP_BUNDLE/Contents/PkgInfo"

# Generate an app icon from SF Symbols using a small Swift script
echo "Generating app icon..."
cat > /tmp/wispr_icon_gen.swift << 'ICONEOF'
import AppKit

let sizes: [(CGFloat, String)] = [
    (16, "icon_16x16"),
    (32, "icon_16x16@2x"),
    (32, "icon_32x32"),
    (64, "icon_32x32@2x"),
    (128, "icon_128x128"),
    (256, "icon_128x128@2x"),
    (256, "icon_256x256"),
    (512, "icon_256x256@2x"),
    (512, "icon_512x512"),
    (1024, "icon_512x512@2x"),
]

let iconsetPath = CommandLine.arguments[1]
try FileManager.default.createDirectory(atPath: iconsetPath, withIntermediateDirectories: true)

for (size, name) in sizes {
    let image = NSImage(size: NSSize(width: size, height: size))
    image.lockFocus()

    // Background circle
    let bg = NSBezierPath(ovalIn: NSRect(x: size * 0.05, y: size * 0.05, width: size * 0.9, height: size * 0.9))
    NSColor(red: 0x4F/255.0, green: 0xBF/255.0, blue: 0x78/255.0, alpha: 1.0).setFill()
    bg.fill()

    // Mic symbol
    if let symbol = NSImage(systemSymbolName: "mic.fill", accessibilityDescription: nil) {
        let config = NSImage.SymbolConfiguration(pointSize: size * 0.4, weight: .medium)
        let configured = symbol.withSymbolConfiguration(config)!
        let symbolSize = configured.size
        let x = (size - symbolSize.width) / 2
        let y = (size - symbolSize.height) / 2
        NSColor.white.set()
        configured.draw(in: NSRect(x: x, y: y, width: symbolSize.width, height: symbolSize.height),
                       from: .zero, operation: .sourceOver, fraction: 1.0)
    }

    image.unlockFocus()

    guard let tiff = image.tiffRepresentation,
          let bitmap = NSBitmapImageRep(data: tiff),
          let png = bitmap.representation(using: .png, properties: [:]) else { continue }
    try png.write(to: URL(fileURLWithPath: "\(iconsetPath)/\(name).png"))
}
ICONEOF

ICONSET_PATH="$SCRIPT_DIR/$APP_BUNDLE/Contents/Resources/AppIcon.iconset"
if swift /tmp/wispr_icon_gen.swift "$ICONSET_PATH" 2>/dev/null; then
    iconutil -c icns "$ICONSET_PATH" -o "$SCRIPT_DIR/$APP_BUNDLE/Contents/Resources/AppIcon.icns" 2>/dev/null && \
        rm -rf "$ICONSET_PATH"
    # Add icon reference to Info.plist if not present
    if ! grep -q CFBundleIconFile "$SCRIPT_DIR/$APP_BUNDLE/Contents/Info.plist"; then
        sed -i '' 's|</dict>|    <key>CFBundleIconFile</key>\n    <string>AppIcon</string>\n</dict>|' "$SCRIPT_DIR/$APP_BUNDLE/Contents/Info.plist"
    fi
    echo "App icon generated."
else
    echo "Skipping icon generation (non-critical)."
    rm -rf "$ICONSET_PATH" /tmp/wispr_icon_gen.swift
fi
rm -f /tmp/wispr_icon_gen.swift

# Install to /Applications
echo "Installing to $INSTALL_DIR..."
if [ -d "$INSTALL_DIR/$APP_BUNDLE" ]; then
    rm -rf "$INSTALL_DIR/$APP_BUNDLE"
fi
cp -R "$SCRIPT_DIR/$APP_BUNDLE" "$INSTALL_DIR/$APP_BUNDLE"

echo ""
echo "Installed: $INSTALL_DIR/$APP_BUNDLE"
echo ""
echo "Next steps:"
echo "  1. Open the app:  open '/Applications/$APP_NAME.app'"
echo "  2. Grant permissions in System Settings → Privacy & Security:"
echo "     - Accessibility (for text injection)"
echo "     - Input Monitoring (for hotkey capture)"
echo "     - Microphone (will prompt on first use)"
echo ""
echo "Done!"

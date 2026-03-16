#!/bin/bash
# Build Wispr Lite as a macOS .app bundle
set -e

echo "Building Wispr Lite..."
swift build -c release 2>&1

APP_DIR="Wispr Lite.app/Contents"
rm -rf "Wispr Lite.app"
mkdir -p "$APP_DIR/MacOS"
mkdir -p "$APP_DIR/Resources"

# Copy binary
cp .build/release/WisprLite "$APP_DIR/MacOS/WisprLite"

# Copy Info.plist
cp Resources/Info.plist "$APP_DIR/Info.plist"

echo "Built: Wispr Lite.app"
echo ""
echo "To install: cp -r \"Wispr Lite.app\" /Applications/"
echo "To run:     open \"Wispr Lite.app\""

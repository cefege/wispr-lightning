#!/bin/bash
# record_demo.sh — Records a demo video of Wispr Lightning
# Usage: ./record_demo.sh
# You will need to speak one sentence when prompted.

set -e

OUTPUT_RAW="demo_raw.mp4"
OUTPUT_FINAL="demo.mp4"
OUTPUT_GIF="demo_settings.gif"

echo "=== Wispr Lightning Demo Recorder ==="
echo ""

# Clean up old outputs
rm -f "$OUTPUT_RAW" "$OUTPUT_FINAL" "$OUTPUT_GIF"

# Require ffmpeg
if ! command -v ffmpeg &>/dev/null; then
    echo "Error: ffmpeg not found. Run: brew install ffmpeg"
    exit 1
fi

# Ensure app is running
if ! pgrep -x "WisprLightning" > /dev/null; then
    echo "Launching Wispr Lightning..."
    open "/Applications/Wispr Lightning.app"
    sleep 3
fi

echo "Starting in:"
for i in 3 2 1; do echo "  $i..."; sleep 1; done
echo "  Recording!"
echo ""

# ── Start screen + mic recording ───────────────────────────────────────────────
ffmpeg -f avfoundation -i "1:0" \
    -r 30 -vcodec libx264 -preset ultrafast -pix_fmt yuv420p \
    "$OUTPUT_RAW" -y 2>/dev/null &
FFMPEG_PID=$!
sleep 1.5  # let ffmpeg warm up

# ── Show the menu bar icon, click to open menu ─────────────────────────────────
osascript <<'AS'
tell application "System Events"
    tell process "WisprLightning"
        click menu bar item 1 of menu bar 2
    end tell
end tell
AS
sleep 1

# ── Click Settings ─────────────────────────────────────────────────────────────
osascript <<'AS'
tell application "System Events"
    tell process "WisprLightning"
        click menu item "Settings" of menu 1 of menu bar item 1 of menu bar 2
    end tell
end tell
AS
sleep 2.5  # let settings window open and render

# ── Navigate sidebar: General → Dictation → Privacy → System ──────────────────
# After window opens, sidebar should have initial focus on General.
# Down arrow navigates the sidebar list.
osascript <<'AS'
tell application "System Events"
    tell process "WisprLightning"
        -- Move to Dictation (one down from General)
        key code 125
        delay 2.5

        -- Move to Privacy (5 more: Polish → History → Dictionary → Notes → Privacy)
        repeat 5 times
            key code 125
            delay 0.25
        end repeat
        delay 1.5

        -- Move to System
        key code 125
        delay 1.5

        -- Close the settings window
        keystroke "w" using command down
    end tell
end tell
AS
sleep 1

# ── Open TextEdit so there's a text field to dictate into ──────────────────────
osascript -e 'tell application "TextEdit" to activate'
sleep 1
osascript -e 'tell application "System Events" to keystroke "n" using command down'
sleep 1.5

# ── Prompt user to dictate ─────────────────────────────────────────────────────
osascript -e 'display dialog "TextEdit is ready.\n\nClick OK then:\n  1. Hold Left Control\n  2. Say a sentence\n  3. Release — watch your words appear" buttons {"OK, I'\''m ready"} default button 1'

# Give time for the full dictation flow: hold → speak → release → text appears
sleep 14

# ── Stop recording ─────────────────────────────────────────────────────────────
kill $FFMPEG_PID 2>/dev/null
wait $FFMPEG_PID 2>/dev/null || true

echo "Recording stopped. Processing..."
echo ""

# ── Trim and optimize final video ──────────────────────────────────────────────
ffmpeg -i "$OUTPUT_RAW" -ss 1 \
    -vf "scale=1280:-2" -crf 23 \
    "$OUTPUT_FINAL" -y 2>/dev/null

# ── GIF of settings walkthrough (first 22s after trim) ─────────────────────────
ffmpeg -i "$OUTPUT_FINAL" -t 22 \
    -vf "fps=12,scale=800:-2:flags=lanczos,split[s0][s1];[s0]palettegen[p];[s1][p]paletteuse" \
    "$OUTPUT_GIF" -y 2>/dev/null

echo "Done!"
echo ""
echo "  Video → $OUTPUT_FINAL"
echo "  GIF   → $OUTPUT_GIF"
echo ""
echo "Next steps:"
echo "  1. Review the video — open $OUTPUT_FINAL"
echo "  2. Upload demo.mp4 to Loom or GitHub Releases"
echo "  3. Add demo_settings.gif to the README"
echo ""

open "$OUTPUT_FINAL"

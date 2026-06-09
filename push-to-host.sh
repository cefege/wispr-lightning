#!/bin/bash
# push-to-host.sh — copy this Mac's installed Wispr Lightning.app to a remote
# macOS host, ad-hoc-sign with a stable identifier, and relaunch. Defaults to
# host `m5` (matching the user's ~/.ssh/config entry); pass a different host
# as the first argument to retarget.
#
# Prerequisites:
#   - ./install.sh has been run locally so /Applications/Wispr Lightning.app
#     reflects the latest build.
#   - ssh <host> is configured (key auth, no password prompt).
#   - mike's account on the remote has admin rights to /Applications.
#
# What it does:
#   1. Quits any running Lightning on the remote.
#   2. scp's the bundle to /tmp/ on the remote (atomic staging — never
#      corrupts the existing /Applications bundle on partial transfer).
#   3. Swaps the staged bundle into /Applications.
#   4. Strips quarantine + ad-hoc signs so Gatekeeper allows the launch.
#   5. Relaunches and reports the running PID + signature identifier.
#
# Heads up: ad-hoc signature differs from any previous code identity on the
# remote, so macOS TCC will treat this as a "new app" — Accessibility / Input
# Monitoring / Mic / Screen Recording need to be re-granted in System
# Settings on the remote after the first push. To avoid that on subsequent
# pushes, run setup-codesign.sh on the remote once and edit this script to
# pass --sign "Wispr Lightning Dev" instead of --sign -.

set -e
HOST="${1:-m5}"
APP_NAME="Wispr Lightning.app"
LOCAL_PATH="/Applications/$APP_NAME"
REMOTE_PATH="/Applications/$APP_NAME"
STAGE_PATH="/tmp/wispr-lightning.app-new"

if [ ! -d "$LOCAL_PATH" ]; then
    echo "Local bundle not found at $LOCAL_PATH" >&2
    echo "Run ./install.sh first to build + install locally." >&2
    exit 1
fi

echo "[1/5] Quitting Wispr Lightning on $HOST..."
# Graceful first, then a force-kill for any stragglers. Both are best-effort.
ssh "$HOST" 'osascript -e "quit app \"Wispr Lightning\"" >/dev/null 2>&1 || true
             sleep 1
             pkill -9 -x WisprLightning >/dev/null 2>&1 || true
             true'

echo "[2/5] Copying bundle to $HOST:$STAGE_PATH..."
# STAGE_PATH has no spaces — modern scp uses SFTP which rejects extra
# quoting in remote paths, so we pass the raw path. The bundle's internal
# "Wispr Lightning.app/Contents/..." names are handled fine inside the
# transfer; only the top-level destination needs to be shell-safe.
ssh "$HOST" "rm -rf $STAGE_PATH"
scp -rq "$LOCAL_PATH" "$HOST:$STAGE_PATH"

echo "[3/5] Swapping into /Applications and ad-hoc signing..."
ssh "$HOST" "set -e
    rm -rf '$REMOTE_PATH'
    mv '$STAGE_PATH' '$REMOTE_PATH'
    xattr -cr '$REMOTE_PATH' >/dev/null 2>&1 || true
    codesign --force --deep --sign - --identifier com.wisprlightning.app '$REMOTE_PATH' >/dev/null 2>&1"

echo "[4/5] Launching on $HOST..."
ssh "$HOST" "open -a '$REMOTE_PATH'"

echo "[5/5] Verifying..."
sleep 2
ssh "$HOST" "pgrep -ax WisprLightning | head -1 || echo '(not running yet)'
             codesign -dvv '$REMOTE_PATH' 2>&1 | grep -E '^Identifier|^Authority' | head -2"

echo ""
echo "Done. Local commit:"
git -C "$(dirname "$0")" log -1 --format='  %h %s' 2>/dev/null || true

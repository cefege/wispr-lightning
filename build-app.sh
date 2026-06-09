#!/bin/bash
# Build + install Wispr Lightning. Thin wrapper around install.sh — the
# legacy "Wispr Lite" version of this script referenced a non-existent
# binary path and is gone.
set -e
SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
exec "$SCRIPT_DIR/install.sh" "$@"

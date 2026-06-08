#!/bin/bash
# One-time setup: creates a self-signed Code Signing identity named
# "Wispr Lightning Dev" in the login keychain so install.sh can sign every
# build with the same identity. Without this, every rebuild produces a new
# cdhash and macOS treats it as a brand-new app for TCC — meaning you have
# to re-grant Accessibility, Input Monitoring, Microphone, and Screen
# Recording on every install.

set -e
SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
IDENTITY_NAME="Wispr Lightning Dev"
IDENTITY_FILE="$SCRIPT_DIR/.codesign-identity"

if security find-identity -p codesigning -v 2>/dev/null | grep -q "$IDENTITY_NAME"; then
    echo "Identity '$IDENTITY_NAME' already in your login keychain — nothing to do."
    echo "$IDENTITY_NAME" > "$IDENTITY_FILE"
    echo "Wrote: $IDENTITY_FILE"
    echo "Next:  ./install.sh"
    exit 0
fi

echo "Creating self-signed Code Signing identity '$IDENTITY_NAME'..."

TMPDIR="$(mktemp -d)"
trap "rm -rf '$TMPDIR'" EXIT

# Why: macOS CSSM evaluation requires BOTH keyUsage:digitalSignature AND
# extendedKeyUsage:codeSigning. Without the basic Key Usage extension,
# `security find-identity -p codesigning` reports "Invalid Key Usage for
# policy" and codesign refuses the identity.
openssl req -x509 -newkey rsa:2048 \
    -keyout "$TMPDIR/key.pem" -out "$TMPDIR/cert.pem" \
    -days 3650 -nodes \
    -subj "/CN=$IDENTITY_NAME" \
    -addext "keyUsage=critical,digitalSignature" \
    -addext "extendedKeyUsage=codeSigning" \
    -addext "basicConstraints=critical,CA:false" \
    >/dev/null 2>&1

# Why: openssl 3.x uses AES-256 + PBKDF2 by default for PKCS#12; macOS
# `security import` doesn't accept that yet. -legacy forces RC2/3DES which
# the macOS Security framework reads without complaint.
openssl pkcs12 -export -legacy \
    -inkey "$TMPDIR/key.pem" -in "$TMPDIR/cert.pem" \
    -name "$IDENTITY_NAME" -passout pass:wisprlightning \
    -out "$TMPDIR/identity.p12" \
    >/dev/null 2>&1

# Import with -T /usr/bin/codesign so codesign can use the key without prompting.
security import "$TMPDIR/identity.p12" \
    -P wisprlightning \
    -T /usr/bin/codesign \
    -k "$HOME/Library/Keychains/login.keychain-db" \
    >/dev/null

# User-level trust for code signing. No sudo needed for -d-less invocations.
# Suppress non-zero exit if trust was already set or macOS prompts; non-fatal.
security add-trusted-cert \
    -p codeSign \
    -r trustRoot \
    -k "$HOME/Library/Keychains/login.keychain-db" \
    "$TMPDIR/cert.pem" >/dev/null 2>&1 || true

echo "$IDENTITY_NAME" > "$IDENTITY_FILE"

echo ""
echo "Done."
echo "  Identity:  $IDENTITY_NAME (in login keychain)"
echo "  Marker:    $IDENTITY_FILE"
echo ""
echo "Next: ./install.sh"
echo "On the first install after this, macOS will treat Lightning as a new app"
echo "ONE more time — re-grant Accessibility / Input Monitoring / Microphone /"
echo "Screen Recording. After that, future rebuilds keep all four grants."

#!/bin/bash

# Script to create CLAP bundle after building
set -e

PROFILE=${1:-release}
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_ROOT="$(cd "$SCRIPT_DIR/../.." && pwd)"

# Bundle configuration
BUNDLE_NAME="TwinPeaks.clap"
BUNDLE_IDENTIFIER="com.oscen.twin-peaks.clap"
BUNDLE_VERSION="0.1.0"
DYLIB_NAME="libclap_twin_peaks.dylib"

# Paths
TARGET_DIR="$PROJECT_ROOT/target/$PROFILE"
BUNDLE_DIR="$TARGET_DIR/$BUNDLE_NAME"
CONTENTS_DIR="$BUNDLE_DIR/Contents"
MACOS_DIR="$CONTENTS_DIR/MacOS"

# Create bundle structure
mkdir -p "$MACOS_DIR"

# Create Info.plist
cat > "$CONTENTS_DIR/Info.plist" << EOF
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
    <key>CFBundleExecutable</key>
    <string>TwinPeaks</string>
    <key>CFBundleIdentifier</key>
    <string>$BUNDLE_IDENTIFIER</string>
    <key>CFBundleInfoDictionaryVersion</key>
    <string>6.0</string>
    <key>CFBundleName</key>
    <string>Twin Peaks Synth</string>
    <key>CFBundlePackageType</key>
    <string>BNDL</string>
    <key>CFBundleShortVersionString</key>
    <string>$BUNDLE_VERSION</string>
    <key>CFBundleVersion</key>
    <string>$BUNDLE_VERSION</string>
    <key>LSMinimumSystemVersion</key>
    <string>10.13</string>
</dict>
</plist>
EOF

# Create PkgInfo
echo "BNDL????" > "$CONTENTS_DIR/PkgInfo"

# Copy dylib
SRC_DYLIB="$TARGET_DIR/$DYLIB_NAME"
DST_DYLIB="$MACOS_DIR/TwinPeaks"

if [ -f "$SRC_DYLIB" ]; then
    cp "$SRC_DYLIB" "$DST_DYLIB"
    echo "✅ Bundle created successfully at: $BUNDLE_DIR"
    echo "✅ Copied dylib from $SRC_DYLIB to $DST_DYLIB"
else
    echo "❌ Error: Dylib not found at $SRC_DYLIB"
    echo "Please run 'cargo build --release' first"
    exit 1
fi
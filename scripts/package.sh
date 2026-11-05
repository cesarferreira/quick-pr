#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT"

PROFILE="release"
BUILD_ARGS=(--release)
if [[ "${1:-}" == "--debug" && "$#" == "1" ]]; then
    PROFILE="debug"
    BUILD_ARGS=()
elif [[ "$#" != "0" ]]; then
    printf 'Usage: %s [--debug]\n' "$0" >&2
    exit 2
fi

if [[ "$(uname -s)" != "Darwin" ]]; then
    printf 'Quick PR is a macOS application. Package it on macOS.\n' >&2
    exit 1
fi

cargo build --locked "${BUILD_ARGS[@]}" --package quick-pr
METADATA="$(cargo metadata --no-deps --format-version 1)"
TARGET_DIR="$(/usr/bin/python3 -c 'import json,sys; print(json.load(sys.stdin)["target_directory"])' <<< "$METADATA")"
VERSION="$(/usr/bin/python3 -c 'import json,sys; print(next(p["version"] for p in json.load(sys.stdin)["packages"] if p["name"] == "quick-pr"))' <<< "$METADATA")"
APP="$ROOT/dist/Quick PR.app"

mkdir -p "$APP/Contents/MacOS" "$APP/Contents/Resources"
BINARY_DIR="$TARGET_DIR/${CARGO_BUILD_TARGET:+$CARGO_BUILD_TARGET/}$PROFILE"
cp "$BINARY_DIR/quick-pr" "$APP/Contents/MacOS/quick-pr"
chmod +x "$APP/Contents/MacOS/quick-pr"
xcrun swift "$ROOT/scripts/generate-icon.swift" "$APP/Contents/Resources"

cat > "$APP/Contents/Info.plist" <<PLIST
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
    <key>CFBundleDevelopmentRegion</key><string>en</string>
    <key>CFBundleDisplayName</key><string>Quick PR</string>
    <key>CFBundleExecutable</key><string>quick-pr</string>
    <key>CFBundleIdentifier</key><string>com.cesarferreira.quick-pr</string>
    <key>CFBundleIconFile</key><string>QuickPR</string>
    <key>CFBundleInfoDictionaryVersion</key><string>6.0</string>
    <key>CFBundleName</key><string>Quick PR</string>
    <key>CFBundlePackageType</key><string>APPL</string>
    <key>CFBundleShortVersionString</key><string>$VERSION</string>
    <key>CFBundleVersion</key><string>$VERSION</string>
    <key>LSMinimumSystemVersion</key><string>12.0</string>
    <key>LSUIElement</key><true/>
    <key>NSHighResolutionCapable</key><true/>
</dict>
</plist>
PLIST

/usr/bin/plutil -lint "$APP/Contents/Info.plist"
/usr/bin/codesign --force --deep --sign - "$APP"
/usr/bin/codesign --verify --deep --strict "$APP"
printf '\nPackaged: %s\nLaunch with: open "%s"\n' "$APP" "$APP"

#!/usr/bin/env bash
# Assemble and sign TEMPRIX.app:
#
#   TEMPRIX.app/Contents/
#     MacOS/TemprixApp                  host: turns temprix:// URLs into `temprix reveal`
#     MacOS/temprix                     the Rust CLI and daemon
#     PlugIns/TemprixFinder.appex       Finder right-click menu item
#
# Usage: macos/build-app.sh <temprix-binary> <version> <out-dir>
#
# SIGN_IDENTITY: "-" (ad-hoc, the default) runs on this Mac only.
#   A "Developer ID Application: …" identity produces a build that can be
#   notarized and run on other Macs.
# ARCHS: CPU architectures for the Swift parts, e.g. "arm64 x86_64" for a
#   release that runs on Apple Silicon and Intel. Default: this Mac's.
#   The temprix binary passed in must already cover the same ones.
set -euo pipefail

CLI=$1
VERSION=$2
OUT=$3
SIGN_IDENTITY=${SIGN_IDENTITY:--}
ARCHS=${ARCHS:-$(uname -m)}
MIN_MACOS=12.0
HERE=$(cd "$(dirname "$0")" && pwd)

APP="$OUT/TEMPRIX.app"
APPEX="$APP/Contents/PlugIns/TemprixFinder.appex"
rm -rf "$APP"
mkdir -p "$APP/Contents/MacOS" "$APPEX/Contents/MacOS"

# Compile once per architecture, then merge into one universal binary.
# swift_build <out> <swiftc args…>
swift_build() {
    local out=$1; shift
    local slices=()
    for arch in $ARCHS; do
        swiftc -O -target "$arch-apple-macos$MIN_MACOS" "$@" -o "$out.$arch"
        slices+=("$out.$arch")
    done
    lipo -create "${slices[@]}" -output "$out"
    rm -f "${slices[@]}"
}

# Finder extension. App extensions start in NSExtensionMain, not main().
swift_build "$APPEX/Contents/MacOS/TemprixFinder" \
    -module-name TemprixFinder -application-extension -parse-as-library \
    -Xlinker -e -Xlinker _NSExtensionMain -Xlinker -application_extension \
    -framework FinderSync \
    "$HERE/FinderExtension/TemprixFinder.swift"
sed "s/__VERSION__/$VERSION/g" "$HERE/FinderExtension/Info.plist" > "$APPEX/Contents/Info.plist"

swift_build "$APP/Contents/MacOS/TemprixApp" \
    -module-name TemprixApp \
    "$HERE/App/main.swift"
sed "s/__VERSION__/$VERSION/g" "$HERE/App/Info.plist" > "$APP/Contents/Info.plist"

cp "$CLI" "$APP/Contents/MacOS/temprix"

# Hardened runtime even for local builds, so they behave like a notarized
# release and runtime restrictions surface here first. A secure timestamp
# needs a real identity, and notarization requires one.
sign_flags=(--force --options runtime --sign "$SIGN_IDENTITY")
if [[ "$SIGN_IDENTITY" != "-" ]]; then
    sign_flags+=(--timestamp)
fi

# Inside-out: a bundle's signature seals its nested code, so nested code
# must already be signed when the outer bundle is.
codesign "${sign_flags[@]}" --entitlements "$HERE/FinderExtension/TemprixFinder.entitlements" "$APPEX"
codesign "${sign_flags[@]}" --identifier com.temprix.cli "$APP/Contents/MacOS/temprix"
codesign "${sign_flags[@]}" "$APP"
codesign --verify --strict --deep "$APP"

echo "Built $APP (signed with: $SIGN_IDENTITY)"

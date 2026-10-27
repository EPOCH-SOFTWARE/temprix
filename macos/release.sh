#!/usr/bin/env bash
# Build a release of TEMPRIX.app that any Mac will open without warnings:
# universal (Apple Silicon + Intel), signed with a Developer ID, notarized
# by Apple, ticket stapled, zipped.
#
# Usage: macos/release.sh <version>      (normally via `make release`)
#
# SIGN_IDENTITY   Developer ID Application identity. Default: the first
#                 one in your keychain.
# NOTARY_PROFILE  Credentials saved with `xcrun notarytool store-credentials`.
#                 Default: temprix-notary.
# NOTARIZE=0      Sign with the Developer ID but skip Apple's notary service.
#                 Output is named *-unnotarized.zip: other Macs will warn
#                 before opening it. For checking a signed build, not shipping.
# DRY_RUN=1       Ad-hoc sign and skip notarization, to test the pipeline
#                 without a certificate. The result runs only on this Mac.
#
# Output: dist/TEMPRIX-<version>.zip and its SHA-256.
set -euo pipefail

VERSION=$1
NOTARY_PROFILE=${NOTARY_PROFILE:-temprix-notary}
DRY_RUN=${DRY_RUN:-0}
NOTARIZE=${NOTARIZE:-1}
[[ "$DRY_RUN" == 1 ]] && NOTARIZE=0
ROOT=$(cd "$(dirname "$0")/.." && pwd)
WORK="$ROOT/target/release-app"
APP="$WORK/TEMPRIX.app"
ZIP="$ROOT/dist/TEMPRIX-$VERSION.zip"
[[ "$NOTARIZE" == 1 ]] || ZIP="$ROOT/dist/TEMPRIX-$VERSION-unnotarized.zip"

step() { printf '\n==> %s\n' "$*"; }
fail() { printf '\nerror: %s\n' "$*" >&2; exit 1; }

# ─── Preflight: fail before a 2-minute build, not after ────────────────

if [[ "$DRY_RUN" == 1 ]]; then
    SIGN_IDENTITY="-"
else
    SIGN_IDENTITY=${SIGN_IDENTITY:-$(security find-identity -v -p codesigning \
        | sed -n 's/.*"\(Developer ID Application: .*\)"/\1/p' | head -1)}
    [[ -n "$SIGN_IDENTITY" ]] || fail "no 'Developer ID Application' certificate in your keychain.
  Create one: Xcode → Settings → Accounts → (your team) → Manage Certificates → + → Developer ID Application.
  Or test the pipeline without one: make release DRY_RUN=1"
    [[ "$NOTARIZE" == 0 ]] || xcrun notarytool history --keychain-profile "$NOTARY_PROFILE" >/dev/null 2>&1 \
        || fail "no notarization credentials saved as '$NOTARY_PROFILE'. Save them once with:
  xcrun notarytool store-credentials $NOTARY_PROFILE --apple-id <your Apple ID> --team-id <TEAMID>
  It asks for an app-specific password: create one at https://account.apple.com → Sign-In and Security."
fi
for target in aarch64-apple-darwin x86_64-apple-darwin; do
    rustup target list --installed | grep -qx "$target" || fail "missing Rust target. Run: rustup target add $target"
done

# ─── Build ─────────────────────────────────────────────────────────────

step "Testing"
(cd "$ROOT" && cargo test --quiet)

step "Building temprix for Apple Silicon and Intel"
for target in aarch64-apple-darwin x86_64-apple-darwin; do
    (cd "$ROOT" && cargo build --release --quiet --target "$target")
done
mkdir -p "$WORK"
lipo -create \
    "$ROOT/target/aarch64-apple-darwin/release/temprix" \
    "$ROOT/target/x86_64-apple-darwin/release/temprix" \
    -output "$WORK/temprix"

step "Building and signing TEMPRIX.app ($SIGN_IDENTITY)"
SIGN_IDENTITY="$SIGN_IDENTITY" ARCHS="arm64 x86_64" \
    "$ROOT/macos/build-app.sh" "$WORK/temprix" "$VERSION" "$WORK"

# ─── Verify what we built, before Apple does ───────────────────────────

step "Verifying"
for bin in MacOS/TemprixApp MacOS/temprix PlugIns/TemprixFinder.appex/Contents/MacOS/TemprixFinder; do
    archs=$(lipo -archs "$APP/Contents/$bin")
    [[ "$archs" == *arm64* && "$archs" == *x86_64* ]] || fail "$bin is not universal: $archs"
done
codesign --verify --deep --strict "$APP"
if [[ "$DRY_RUN" != 1 ]]; then
    # Notarization rejects anything without a hardened runtime and a
    # secure timestamp; catch it here instead of after an upload.
    for code in "$APP" "$APP/Contents/MacOS/temprix" "$APP/Contents/PlugIns/TemprixFinder.appex"; do
        info=$(codesign -dvv "$code" 2>&1)
        grep -q "flags=.*runtime" <<<"$info" || fail "$code lacks the hardened runtime"
        grep -q "^Timestamp=" <<<"$info" || fail "$code lacks a secure timestamp"
        grep -q "^TeamIdentifier=" <<<"$info" && ! grep -q "^TeamIdentifier=not set" <<<"$info" \
            || fail "$code is not signed with a team identity"
    done
fi
echo "universal, signature valid"

mkdir -p "$ROOT/dist"
rm -f "$ZIP"

if [[ "$DRY_RUN" == 1 ]]; then
    ditto -c -k --keepParent "$APP" "$ZIP"
    step "Dry run: skipped notarization (ad-hoc signed, runs on this Mac only)"
elif [[ "$NOTARIZE" == 0 ]]; then
    ditto -c -k --keepParent "$APP" "$ZIP"
    step "Signed with $SIGN_IDENTITY; skipped notarization (other Macs will warn before opening)"
else
    # ─── Notarize and staple ───────────────────────────────────────────
    step "Submitting to Apple's notary service (usually 1–5 minutes)"
    ditto -c -k --keepParent "$APP" "$WORK/upload.zip"
    result=$(xcrun notarytool submit "$WORK/upload.zip" --keychain-profile "$NOTARY_PROFILE" \
        --wait --output-format json)
    status=$(plutil -extract status raw -o - - <<<"$result")
    id=$(plutil -extract id raw -o - - <<<"$result")
    if [[ "$status" != "Accepted" ]]; then
        xcrun notarytool log "$id" --keychain-profile "$NOTARY_PROFILE" >&2 || true
        fail "notarization $status (submission $id); Apple's log is above"
    fi

    step "Stapling the notarization ticket"
    # Stapled, the app opens without Gatekeeper needing to reach Apple.
    xcrun stapler staple "$APP"
    xcrun stapler validate "$APP"
    spctl --assess --type execute --verbose=2 "$APP" 2>&1 | tee /dev/stderr | grep -q "Notarized Developer ID" \
        || fail "Gatekeeper does not accept the app"

    # Zip again: the uploaded zip predates the stapled ticket.
    ditto -c -k --keepParent "$APP" "$ZIP"
fi

step "Done"
echo "$ZIP"
shasum -a 256 "$ZIP"

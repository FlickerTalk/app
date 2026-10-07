#!/usr/bin/env bash
# Drives FlickerTalk's subscription in the iOS simulator against a local StoreKit configuration
# (`src-tauri/platform/ios/StoreKit/FlickerTalk.storekit`): no App Store Connect, no sandbox account.
#
#   scripts/ios-storekit/run.sh <simulator-udid> <path/to/FlickerTalk.app> '<actions>'
#
# The actions are listed in Driver/Driver.swift, e.g. 'session;reset', 'button:Suscribirse',
# 'expire;list'. Build the app for the simulator first (`npm run tauri ios build -- --target
# aarch64-sim`).
#
# How the configuration reaches the app: the UI-test runner opens an `SKTestSession`, which loads
# the `.storekit` file for the test's *target application*. The runner is built once and its
# `.xctestrun` is pointed at the FlickerTalk build (`UITargetAppPath`), so the session belongs to
# `com.flickertalk.app` and what it sets stays in the simulator after the run: the app can then be
# opened and driven normally, and the StoreKit purchase sheet is the test one.
set -euo pipefail
here="$(cd "$(dirname "$0")" && pwd)"
udid="${1:?simulator udid}"
app="$(cd "$(dirname "${2:?path to FlickerTalk.app}")" && pwd)/$(basename "$2")"
actions="${3:-session}"
out="${FT_STOREKIT_BUILD:-${TMPDIR:-/tmp}/ft-storekit-driver}"

if [ ! -d "$here/FTStoreKit.xcodeproj" ]; then (cd "$here" && xcodegen generate >/dev/null); fi
run=$(find "$out/Build/Products" -maxdepth 1 -name 'Driver_*.xctestrun' 2>/dev/null | head -n 1)
if [ -z "$run" ]; then
  xcodebuild build-for-testing -project "$here/FTStoreKit.xcodeproj" -scheme Driver \
    -destination "id=$udid" -derivedDataPath "$out" CODE_SIGNING_ALLOWED=NO -quiet
  run=$(find "$out/Build/Products" -maxdepth 1 -name 'Driver_*.xctestrun' | head -n 1)
fi

bundle=$(/usr/libexec/PlistBuddy -c 'Print :CFBundleIdentifier' "$app/Info.plist")
target="$(dirname "$run")/FlickerTalk.xctestrun"
cp "$run" "$target"
plist() { /usr/libexec/PlistBuddy -c "$1" "$target" >/dev/null 2>&1 || true; }
plist "Delete :Driver:UITargetAppPath"
plist "Delete :Driver:UITargetAppBundleIdentifier"
plist "Add :Driver:UITargetAppPath string $app"
plist "Add :Driver:UITargetAppBundleIdentifier string $bundle"
plist "Add :Driver:DependentProductPaths: string $app"

TEST_RUNNER_ACTIONS="$actions" xcodebuild test-without-building -xctestrun "$target" \
  -destination "id=$udid" 2>&1 | grep -E 'DRIVER:|error|\*\* TEST' || true

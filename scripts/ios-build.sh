#!/usr/bin/env bash
#   APPLE_DEVELOPMENT_TEAM=<team> scripts/ios-build.sh [device-udid]
# The signing team goes into the Xcode project only for the build, so it never reaches the repo.
set -euo pipefail
cd "$(dirname "$0")/.."
: "${APPLE_DEVELOPMENT_TEAM:?set APPLE_DEVELOPMENT_TEAM to your Apple team id}"
project=src-tauri/gen/apple/flickertalk.xcodeproj/project.pbxproj
restore() { sed -i '' "s/DEVELOPMENT_TEAM = \"$APPLE_DEVELOPMENT_TEAM\";/DEVELOPMENT_TEAM = \"\";/" "$project"; }
trap restore EXIT
sed -i '' "s/DEVELOPMENT_TEAM = \"\";/DEVELOPMENT_TEAM = \"$APPLE_DEVELOPMENT_TEAM\";/" "$project"
# tauri ios build passes only TAURI* variables on to Xcode
if [ -n "${FT_GOOGLE_IOS_CLIENT_ID:-}" ]; then export TAURI_FT_GOOGLE_IOS_CLIENT_ID="$FT_GOOGLE_IOS_CLIENT_ID"; fi
npm run tauri ios build -- --debug --target aarch64
if [ -n "${1:-}" ]; then
  xcrun devicectl device install app --device "$1" src-tauri/gen/apple/build/arm64/flickertalk.ipa
  xcrun devicectl device process launch --terminate-existing --device "$1" com.flickertalk.app || true
fi

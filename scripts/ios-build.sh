#!/usr/bin/env bash
# Builds the debug iOS app and installs it on a connected iPhone.
#   APPLE_DEVELOPMENT_TEAM=<team> scripts/ios-build.sh [device-udid]
# The signing team is only written into the Xcode project for the build and taken out again, so
# it never reaches the (public) repository.
set -euo pipefail
cd "$(dirname "$0")/.."
: "${APPLE_DEVELOPMENT_TEAM:?set APPLE_DEVELOPMENT_TEAM to your Apple team id}"
project=src-tauri/gen/apple/flickertalk.xcodeproj/project.pbxproj
restore() { sed -i '' "s/DEVELOPMENT_TEAM = \"$APPLE_DEVELOPMENT_TEAM\";/DEVELOPMENT_TEAM = \"\";/" "$project"; }
trap restore EXIT
sed -i '' "s/DEVELOPMENT_TEAM = \"\";/DEVELOPMENT_TEAM = \"$APPLE_DEVELOPMENT_TEAM\";/" "$project"
npm run tauri ios build -- --debug --target aarch64
if [ -n "${1:-}" ]; then
  xcrun devicectl device install app --device "$1" src-tauri/gen/apple/build/arm64/flickertalk.ipa
  xcrun devicectl device process launch --terminate-existing --device "$1" com.flickertalk.flickertalk || true
fi

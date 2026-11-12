#!/usr/bin/env bash
#   scripts/ios-sim-smoke.sh [path/to/FlickerTalk.app] [seconds]
# Build a release for the simulator first:
#   npm run tauri ios build -- --target aarch64-sim
#
# A debug build hides some launch crashes (the 1.2.1 release died at every launch, debug never did).
set -euo pipefail
cd "$(dirname "$0")/.."

app="${1:-}"
seconds="${2:-20}"
bundle_id="${FT_SMOKE_BUNDLE_ID:-com.flickertalk.app}"

if [ -z "$app" ]; then
  app=$(find src-tauri/gen/apple/build -maxdepth 4 -type d -name '*.app' -path '*sim*' 2>/dev/null | head -n 1)
fi
if [ -z "$app" ] || [ ! -d "$app" ]; then
  echo "ios-sim-smoke: no simulator .app found; build one first (see the header)" >&2
  exit 2
fi

read -r runtime device_type < <(xcrun simctl list runtimes available -j | python3 -c '
import json, sys
runtimes = [r for r in json.load(sys.stdin)["runtimes"] if r["platform"] == "iOS"]
phones = [t["identifier"] for t in (runtimes[-1]["supportedDeviceTypes"] if runtimes else [])
          if t.get("productFamily") == "iPhone"]
print(runtimes[-1]["identifier"] if runtimes else "", phones[-1] if phones else "")') || true
if [ -z "$runtime" ] || [ -z "$device_type" ]; then
  echo "ios-sim-smoke: no iOS simulator runtime or iPhone device type installed" >&2
  exit 2
fi

udid=$(xcrun simctl create "ft-smoke-$$" "$device_type" "$runtime")
cleanup() {
  xcrun simctl shutdown "$udid" >/dev/null 2>&1 || true
  xcrun simctl delete "$udid" >/dev/null 2>&1 || true
}
trap cleanup EXIT

echo "ios-sim-smoke: $app on $device_type ($runtime)"
xcrun simctl boot "$udid"
xcrun simctl bootstatus "$udid" >/dev/null
xcrun simctl install "$udid" "$app"

reports="$HOME/Library/Logs/DiagnosticReports"
marker=$(mktemp)
trap 'rm -f "$marker"; cleanup' EXIT

# simctl launch prints "<bundle id>: <pid>"
pid=$(xcrun simctl launch "$udid" "$bundle_id" | awk '{print $NF}')
echo "ios-sim-smoke: launched, pid $pid; watching for ${seconds}s"

for _ in $(seq "$seconds"); do
  sleep 1
  if ! kill -0 "$pid" 2>/dev/null; then
    echo "ios-sim-smoke: FAIL: the app exited within ${seconds}s of its launch" >&2
    find "$reports" -newer "$marker" -name '*.ips' -print 2>/dev/null >&2 || true
    exit 1
  fi
done

crashes=$(find "$reports" -newer "$marker" -name "$(basename "$app" .app)*.ips" 2>/dev/null || true)
if [ -n "$crashes" ]; then
  echo "ios-sim-smoke: FAIL: crash reports written during the run:" >&2
  echo "$crashes" >&2
  exit 1
fi

xcrun simctl terminate "$udid" "$bundle_id" >/dev/null 2>&1 || true
echo "ios-sim-smoke: OK: still running after ${seconds}s"

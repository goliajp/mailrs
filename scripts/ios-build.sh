#!/usr/bin/env bash
# ios-build.sh — generate, build, test, and optionally run the iOS app.
#
# Usage:
#   ./scripts/ios-build.sh              # generate + build + test
#   ./scripts/ios-build.sh run          # ... then install and launch on sim-mailrs
#
# The simulator is a named device, not "whatever is booted": a shared
# simulator accumulates other projects' state, and a login screen that
# behaves differently because some other app left a cookie is not a
# problem worth debugging twice.
set -euo pipefail
cd "$(dirname "$0")/.."

SIM_NAME="sim-mailrs"
DEVICE_TYPE="com.apple.CoreSimulator.SimDeviceType.iPhone-17-Pro"
BUNDLE_ID="jp.golia.mailrs"

# The device is created on demand so a fresh checkout needs no setup step.
UDID=$(xcrun simctl list devices -j \
  | python3 -c "import json,sys;d=json.load(sys.stdin)['devices'];print(next((x['udid'] for v in d.values() for x in v if x['name']=='$SIM_NAME'),''))")
if [ -z "$UDID" ]; then
    RUNTIME=$(xcrun simctl list runtimes -j \
      | python3 -c "import json,sys;r=[x for x in json.load(sys.stdin)['runtimes'] if x['isAvailable'] and 'iOS' in x['name']];print(r[-1]['identifier'] if r else '')")
    [ -n "$RUNTIME" ] || { echo "!! no iOS simulator runtime installed"; exit 1; }
    UDID=$(xcrun simctl create "$SIM_NAME" "$DEVICE_TYPE" "$RUNTIME")
    echo "==> created $SIM_NAME ($UDID)"
fi
echo "==> $SIM_NAME = $UDID"

cd ios
echo "==> [1/3] xcodegen"
xcodegen generate --spec project.yml

echo "==> [2/3] build"
xcodebuild -project Mailrs.xcodeproj -scheme Mailrs -destination "id=$UDID" build \
  | grep -E "error:|warning:|\*\* BUILD" || true

echo "==> [3/3] test"
xcodebuild -project Mailrs.xcodeproj -scheme Mailrs -destination "id=$UDID" test \
  | grep -E "✔|✘|error:|\*\* TEST" || true

if [ "${1:-}" = "run" ]; then
    # `-showBuildSettings` rather than `find` over DerivedData: the path
    # carries a per-configuration hash, and globbing for the newest .app
    # picks up another project's build often enough to waste an evening.
    APP_DIR=$(xcodebuild -project Mailrs.xcodeproj -scheme Mailrs -destination "id=$UDID" \
      -showBuildSettings 2>/dev/null | awk -F' = ' '/ BUILT_PRODUCTS_DIR /{print $2; exit}')
    APP_NAME=$(xcodebuild -project Mailrs.xcodeproj -scheme Mailrs -destination "id=$UDID" \
      -showBuildSettings 2>/dev/null | awk -F' = ' '/ FULL_PRODUCT_NAME /{print $2; exit}')
    xcrun simctl boot "$UDID" 2>/dev/null || true
    xcrun simctl install "$UDID" "$APP_DIR/$APP_NAME"
    xcrun simctl launch "$UDID" "$BUNDLE_ID"
    open -a Simulator
    echo "==> launched $BUNDLE_ID on $SIM_NAME"
fi

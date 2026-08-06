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

# The UI tests drive the app against a stub, and the script owns its
# lifetime. Left to the operator it is simply absent on the run that
# matters, and the suite reports "inbox never listed" — which reads like
# an app bug and is not one.
STUB_PORT=6039
echo "==> [3/3] test (stub on :$STUB_PORT)"
python3 Testing/stub-api.py "$STUB_PORT" >/tmp/mailrs-ios-stub.log 2>&1 &
STUB_PID=$!
trap 'kill $STUB_PID 2>/dev/null || true' EXIT
for _ in $(seq 1 20); do
    curl -fsS -o /dev/null "http://127.0.0.1:$STUB_PORT/api/conversations" && break
    sleep 0.25
done
if ! curl -fsS -o /dev/null "http://127.0.0.1:$STUB_PORT/api/conversations"; then
    echo "!! stub did not come up on :$STUB_PORT — see /tmp/mailrs-ios-stub.log"
    exit 1
fi

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

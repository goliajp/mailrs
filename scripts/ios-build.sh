#!/usr/bin/env bash
# ios-build.sh — generate, build, test, and optionally run the iOS app.
#
# Usage:
#   ./scripts/ios-build.sh              # generate + build + test
#   ./scripts/ios-build.sh run          # ... then install and launch on sim-mailrs
#   ./scripts/ios-build.sh device       # build signed, install and launch on a
#                                       # paired iPhone (skips the simulator suite)
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

# A paired phone, signed with the GOLIA K.K. team's Apple Development
# certificate. The certificate is `CN=Apple Development: HAO LI (…)`,
# `OU=KF79DRC524`, `O=GOLIA K.K.` — the common name is the person, which
# is why reading `security find-identity` alone made it look as though
# the team had no iOS certificate at all. It has one.
if [ "${1:-}" = "device" ]; then
    xcodegen generate --spec project.yml
    # JSON, not the table: device names contain spaces ("panda's
    # iphone"), so positional awk over the columns picks the wrong field
    # and asks CoreDevice for a device called "iPhone".
    # Paired is not present: devicectl keeps listing a phone that was
    # unplugged an hour ago, and the build then fails at install with a
    # CoreDevice "unable to locate" that reads like a tooling bug. The
    # tunnel state is what says the phone is actually here.
    DEVICE=$(xcrun devicectl list devices --json-output - 2>/dev/null | python3 -c "
import json, sys
devices = json.load(sys.stdin)['result']['devices']
here = [d for d in devices
        if 'paired' in d.get('connectionProperties', {}).get('pairingState', '')
        and d.get('connectionProperties', {}).get('tunnelState') != 'unavailable']
print(here[0]['identifier'] if here else '')
")
    if [ -z "$DEVICE" ]; then
        echo "!! no iPhone reachable — paired devices exist but none is connected."
        echo "!! Plug the phone in (or put it on this network with Wi-Fi debugging on)."
        exit 1
    fi
    echo "==> device $DEVICE"
    xcodebuild -project Mailrs.xcodeproj -scheme Mailrs \
      -destination "platform=iOS,id=$DEVICE" -derivedDataPath /tmp/mailrs-device build \
      | grep -E "error:|Signing Identity|\*\* BUILD" || true
    APP=/tmp/mailrs-device/Build/Products/Debug-iphoneos/Mailrs.app
    [ -d "$APP" ] || { echo "!! no built app at $APP"; exit 1; }
    xcrun devicectl device install app --device "$DEVICE" "$APP" | grep -E "bundleID|Error"
    xcrun devicectl device process launch --device "$DEVICE" "$BUNDLE_ID" | tail -1
    exit 0
fi

# Costs milliseconds and compiles nothing, so it runs before the build
# rather than after a ten-minute suite.
../scripts/check-ios-l10n.sh

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
# Kill first, then start. Reusing whatever already holds the port means
# testing against whatever build of the stub that process happens to be:
# a stale one without `/debug/fetched` made the attachment-index
# assertion read `[]` and look like the app had not fetched anything.
pkill -f "Testing/stub-api.py" 2>/dev/null || true
sleep 0.3
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

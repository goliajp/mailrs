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
    # `-allowProvisioningUpdates` so Xcode may fetch a profile that
    # matches the entitlements, instead of signing with whatever it
    # cached. Without it the first push-enabled build installed happily
    # and silently *without* `aps-environment`: the profile predated the
    # capability, and an app missing that entitlement does not fail —
    # it just never receives a notification.
    xcodebuild -project Mailrs.xcodeproj -scheme Mailrs -allowProvisioningUpdates \
      -destination "platform=iOS,id=$DEVICE" -derivedDataPath /tmp/mailrs-device build \
      | grep -E "error:|warning: .*[Pp]rovisioning|Signing Identity|\*\* BUILD" || true
    # What was actually signed, not what was asked for. The entitlement
    # is the whole feature; a build without it is indistinguishable from
    # a server that is not sending.
    if ! codesign -d --entitlements - \
        /tmp/mailrs-device/Build/Products/Debug-iphoneos/Mailrs.app 2>/dev/null \
        | grep -q "aps-environment"; then
        echo "!! signed without aps-environment — push will not arrive."
        echo "!! The App ID needs the Push Notifications capability, and"
        echo "!! Xcode needs a profile issued after it was added."
        exit 1
    fi
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
# --- the stub is shared, so say who is using it ------------------------
#
# Both suites drive `ios/Testing/stub-api.py` on the same port, and the
# iOS lane used to `pkill` it before starting — for a good reason (a
# stale stub without `/debug/fetched` made an assertion read `[]` and
# look like the app had not fetched anything). The cost was invisible:
# an iOS run started beside an Android run killed the stub out from
# under it, and the Android suite reported **79 failures across every
# test**, all of them "the server refused the sign-in: unexpected end
# of stream". Nothing in that output pointed at the real cause.
#
# A lock turns that into one line. It holds a pid, so a lock left by a
# crashed run is not a lock at all.
STUB_LOCK=/tmp/mailrs-stub.lock
claim_stub_or_refuse() {
    if [ -f "$STUB_LOCK" ]; then
        holder=$(cat "$STUB_LOCK" 2>/dev/null || echo "")
        if [ -n "$holder" ] && kill -0 "$holder" 2>/dev/null; then
            echo "!! the test stub is in use by pid $holder — the other suite is running." >&2
            echo "!! run them one at a time: killing it now would fail every test in that run." >&2
            exit 1
        fi
    fi
    echo $$ > "$STUB_LOCK"
}
release_stub_lock() {
    [ -f "$STUB_LOCK" ] && [ "$(cat "$STUB_LOCK" 2>/dev/null)" = "$$" ] && rm -f "$STUB_LOCK"
    return 0
}

STUB_PORT=6039
echo "==> [3/3] test (stub on :$STUB_PORT)"
# Kill first, then start. Reusing whatever already holds the port means
# testing against whatever build of the stub that process happens to be:
# a stale one without `/debug/fetched` made the attachment-index
# assertion read `[]` and look like the app had not fetched anything.
claim_stub_or_refuse
pkill -f "Testing/stub-api.py" 2>/dev/null || true
sleep 0.3
python3 Testing/stub-api.py "$STUB_PORT" >/tmp/mailrs-ios-stub.log 2>&1 &
STUB_PID=$!
trap 'kill $STUB_PID 2>/dev/null || true; release_stub_lock' EXIT
for _ in $(seq 1 20); do
    curl -fsS -o /dev/null "http://127.0.0.1:$STUB_PORT/api/conversations" && break
    sleep 0.25
done
if ! curl -fsS -o /dev/null "http://127.0.0.1:$STUB_PORT/api/conversations"; then
    echo "!! stub did not come up on :$STUB_PORT — see /tmp/mailrs-ios-stub.log"
    exit 1
fi

# An optional second argument narrows the run:
#   ./scripts/ios-build.sh test MailrsUITests/TriageFlowTests
# Isolating a failure is how you tell a real one from a flake, and the
# way to do that is *through this script* — it owns the stub's lifetime
# and the simulator's. Reaching for `xcodebuild` directly to run one
# test is what left CoreSimulatorService dead and the simulator
# unusable on 2026-08-11.
ONLY=""
if [ -n "${2:-}" ]; then
    ONLY="-only-testing:$2"
    echo "    (only $2)"
fi
# **The status comes from xcodebuild, not from the filter.** This was
# `xcodebuild … | grep … || true`, and a pipeline's status is its last
# command's: the script exited 0 whether the tests passed or failed, so
# a run could only ever be read by eye. It was read by eye, and the
# grep it was read with (`✘`) does not match a test that *crashes* —
# swift-testing reports those under `Failing tests:` with no `✘` at all,
# and prints `Suite … passed` above them. Four crashed tests read as
# green on 2026-08-24.
TEST_LOG=/tmp/mailrs-ios-test.log
TEST_STATUS=0
# shellcheck disable=SC2086 # ONLY is one flag or nothing, not a word list
xcodebuild -project Mailrs.xcodeproj -scheme Mailrs -destination "id=$UDID" $ONLY test \
  > "$TEST_LOG" 2>&1 || TEST_STATUS=$?
grep -E "✔|✘|error:|\*\* TEST" "$TEST_LOG" || true
if [ "$TEST_STATUS" -ne 0 ]; then
    echo "!! tests failed (xcodebuild exit $TEST_STATUS)"
    # The crash list, which no ✔/✘ line carries.
    grep -A20 "Failing tests:" "$TEST_LOG" || true
    echo "!! full log: $TEST_LOG"
    exit "$TEST_STATUS"
fi

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

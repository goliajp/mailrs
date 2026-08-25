#!/usr/bin/env bash
# mac-build.sh — build and run the Mac app.
#
# The sibling of `ios-build.sh`, and deliberately smaller: there is no
# simulator to name and no device to pair, because the machine running
# this is the machine it runs on.
#
# Usage:
#   ./scripts/mac-build.sh            # build
#   ./scripts/mac-build.sh test       # build + the Mac's own UI tests
#   ./scripts/mac-build.sh run        # build, then launch against the stub
#
# The Mac target shares `Accounts`, `Wire`, `Platform` and the views
# that decide what a message looks like. It takes none of the phone's
# screens — see `MailrsMac/MacRootView.swift` for why that is the point
# rather than an omission.
set -euo pipefail
cd "$(dirname "$0")/.."

MODE="${1:-build}"
DERIVED=/tmp/mailrs-mac
APP="$DERIVED/Build/Products/Debug/MailrsMac.app"
STUB_PORT=6039

say() { printf '==> %s\n' "$*"; }

say "[1/2] xcodegen"
(cd ios && xcodegen generate --spec project.yml >/dev/null)

say "[2/2] build"
BUILD_LOG=/tmp/mailrs-mac-build.log
BUILD_STATUS=0
# The status comes from xcodebuild, not from the filter — a pipeline's
# status is its last command's, and `| grep` has swallowed a failed
# build in this repo before.
# Ad-hoc signed — see the note on the test step. `SIGN=1` asks for
# the real identity, which is what a build meant to leave this machine
# needs and what a build meant to be run does not.
SIGNING=(CODE_SIGN_IDENTITY="-" CODE_SIGNING_REQUIRED=NO CODE_SIGNING_ALLOWED=NO)
[ "${SIGN:-0}" = "1" ] && SIGNING=()
(cd ios && xcodebuild -project Mailrs.xcodeproj -scheme MailrsMac \
    -destination "platform=macOS" -derivedDataPath "$DERIVED" \
    "${SIGNING[@]}" build) \
    > "$BUILD_LOG" 2>&1 || BUILD_STATUS=$?
grep -E "error:|warning: .*deprecated|\*\* BUILD" "$BUILD_LOG" | sort -u | head -20 || true
if [ "$BUILD_STATUS" -ne 0 ]; then
    echo "!! build failed (xcodebuild exit $BUILD_STATUS); full log: $BUILD_LOG"
    exit "$BUILD_STATUS"
fi

if [ "$MODE" = "test" ]; then
    say "[3/3] test"
    TEST_LOG=/tmp/mailrs-mac-test.log
    TEST_STATUS=0
    # Its own scheme. Running these from the iOS scheme would put the
    # Mac's tests wherever that scheme points — which is how the iPad's
    # screens came to be run on a phone.
    # **Ad-hoc signed.** A test build needs to run, not to be
    # distributed, and a real identity here means the build only works
    # where that key is unlocked — over ssh the keychain is locked and
    # codesign fails with `errSecInternalComponent`, which reads like a
    # tooling fault rather than a locked keychain. The signed build for
    # release is a separate thing and keeps its identity.
    (cd ios && xcodebuild -project Mailrs.xcodeproj -scheme MailrsMac \
        -destination "platform=macOS" -derivedDataPath "$DERIVED" \
        "${SIGNING[@]}" test) \
        > "$TEST_LOG" 2>&1 || TEST_STATUS=$?
    grep -E "✔|✘|error:|\*\* TEST" "$TEST_LOG" | sort -u | head -20 || true
    if [ "$TEST_STATUS" -ne 0 ]; then
        echo "!! tests failed (xcodebuild exit $TEST_STATUS)"
        grep -A20 "Failing tests:" "$TEST_LOG" || true
        echo "!! full log: $TEST_LOG"
        exit "$TEST_STATUS"
    fi
    exit 0
fi

[ "$MODE" = "run" ] || exit 0

# The same stub the phone suites use, so a Mac run shows the same mail
# without needing an account.
if ! curl -fsS -o /dev/null "http://127.0.0.1:$STUB_PORT/api/conversations" 2>/dev/null; then
    say "starting the stub on :$STUB_PORT"
    (cd ios && nohup python3 Testing/stub-api.py "$STUB_PORT" >/tmp/mailrs-mac-stub.log 2>&1 &)
    for _ in $(seq 1 20); do
        curl -fsS -o /dev/null "http://127.0.0.1:$STUB_PORT/api/conversations" && break
        sleep 0.25
    done
fi

say "launching"
open "$APP" --args \
    -mailrsBaseURL "http://localhost:$STUB_PORT" \
    -mailrsToken stub-token \
    -mailrs.language en

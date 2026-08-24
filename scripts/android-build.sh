#!/usr/bin/env bash
# android-build.sh — build, test, and optionally install the Android app.
#
# The sibling of `ios-build.sh`, and deliberately the same shape: one
# named device rather than "whatever is attached", the same stub around
# the UI tests, and a build that fails on a warning.
#
# Usage:
#   ./scripts/android-build.sh                # build + unit + instrumented
#   ./scripts/android-build.sh build          # compile only
#   ./scripts/android-build.sh device         # install on the phone
#   ./scripts/android-build.sh test <filter>  # one class or method
#
# The emulator is a named device, not "whatever is booted": a shared
# emulator accumulates other projects' state, and this repo already
# borrowed smix's `sim-smix-android-01` once. `sim-mailrs-android` is
# ours, API 36, created because `avdmanager` cannot read the newer
# system images at all — their `<api-level>` is `36.1` and it parses
# integers, so the AVD is two ini files written by hand.
#
# The phone is `panda` in smix's registry. `adb install` against a
# physical serial is blocked by this machine's adb guard, and rightly:
# an unpinned mutation reaches whatever is plugged in. smix takes the
# device up front, which is the same discipline.
set -euo pipefail
cd "$(dirname "$0")/.."

MODE="${1:-all}"
FILTER="${2:-}"
EMULATOR_SERIAL="emulator-5570"
SMIX_EMULATOR="mailrs-android"
SMIX_PHONE="panda"
STUB_PORT=6039
# The TLS mail stub's ports, and where its certificates are generated.
CERT_DIR=/tmp/mailrs-test-ca
MAIL_STUB_IMAPS=9993
MAIL_STUB_SUBMISSION=9587
MAIL_STUB_UNTRUSTED=9994
MAIL_STUB_PROBE=9995
MAIL_STUB_PID=""
APK="android/app/build/outputs/apk/debug/app-debug.apk"

export ANDROID_HOME="${ANDROID_HOME:-$HOME/Library/Android/sdk}"

say() { printf '==> %s\n' "$*"; }

# --- the stub -----------------------------------------------------
#
# `ios/Testing/stub-api.py`, shared rather than copied: a second stub
# would be a second opinion about what the Rust handlers send, and the
# reason that file exists is to make a client model that disagrees with
# the server fail here instead of in somebody's inbox.
#
# It binds 127.0.0.1, which the emulator reaches as 10.0.2.2 — that is
# what that address means. It also silences its own request log, so an
# empty `/tmp/mailrs-stub.log` is by design and not evidence of
# anything; if you want to know whether the tests really reach it, stop
# it and watch two of them go red.
STUB_PID=""
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

start_stub() {
    claim_stub_or_refuse
    if lsof -nP -iTCP:$STUB_PORT -sTCP:LISTEN >/dev/null 2>&1; then
        say "stub already listening on $STUB_PORT — leaving it alone"
        return
    fi
    python3 ios/Testing/stub-api.py > /tmp/mailrs-stub.log 2>&1 &
    STUB_PID=$!
    for _ in $(seq 1 20); do
        curl -s -o /dev/null --max-time 1 "http://localhost:$STUB_PORT/api/conversations" && break
        sleep 0.3
    done
    say "stub up on $STUB_PORT (pid $STUB_PID)"
    # **The guest reaches the stub over adb, not over the emulator's
    # NAT.** `10.0.2.2` works, but a whole suite of short-lived
    # connections through slirp eventually stalls a connect for
    # seconds, and it showed up as one test per run failing somewhere
    # different with `SocketTimeoutException: failed to connect to
    # /10.0.2.2`. A reverse forward makes it a guest-local port.
    if adb devices | grep -q "^$EMULATOR_SERIAL[[:space:]]*device"; then
        adb -s "$EMULATOR_SERIAL" reverse "tcp:$STUB_PORT" "tcp:$STUB_PORT" >/dev/null
    fi

    # --- a mail server over real TLS -----------------------------
    #
    # Everything this repo asserts about IMAP and SMTP is asserted
    # against a scripted transport: a fake that hands the session lines
    # from a list. That covers the conversation and nothing under it —
    # the TLS handshake, the certificate check, the socket's framing.
    # On iOS the same gap hid a real defect (a refused handshake left
    # the client waiting forever rather than failing), and a fake has
    # no handshake to refuse.
    #
    # The app is **not** modified to reach this. It validates the
    # certificate as it always does; the *debug build* is told to trust
    # one more authority, via `debug-overrides` in
    # `src/debug/res/xml/network_security_config.xml`, which the
    # platform ignores in a release build. The certificate is generated
    # per run and is not in git.
    ios/Testing/make-test-ca.sh "$CERT_DIR" >/dev/null
    cp "$CERT_DIR/ca.pem" android/app/src/debug/res/raw/mailrs_test_ca.pem
    pkill -f "Testing/tls-mail-stub.py" 2>/dev/null || true
    sleep 0.2
    python3 ios/Testing/tls-mail-stub.py "$CERT_DIR" \
        --imaps "$MAIL_STUB_IMAPS" --submission "$MAIL_STUB_SUBMISSION" \
        --imaps-untrusted "$MAIL_STUB_UNTRUSTED" --probe "$MAIL_STUB_PROBE" \
        >/tmp/mailrs-tls-stub.log 2>&1 &
    MAIL_STUB_PID=$!
    for _ in $(seq 1 20); do
        nc -z 127.0.0.1 "$MAIL_STUB_IMAPS" 2>/dev/null && break
        sleep 0.25
    done
    if adb devices | grep -q "^$EMULATOR_SERIAL[[:space:]]*device"; then
        for p in "$MAIL_STUB_IMAPS" "$MAIL_STUB_SUBMISSION" "$MAIL_STUB_UNTRUSTED" \
                 "$MAIL_STUB_PROBE"; do
            adb -s "$EMULATOR_SERIAL" reverse "tcp:$p" "tcp:$p" >/dev/null
        done
    fi
    say "tls mail stub up (imaps $MAIL_STUB_IMAPS, submission $MAIL_STUB_SUBMISSION)"
}
stop_stub() {
    [ -n "$STUB_PID" ] && kill "$STUB_PID" 2>/dev/null || true
    [ -n "$MAIL_STUB_PID" ] && kill "$MAIL_STUB_PID" 2>/dev/null || true
    release_stub_lock
}
trap stop_stub EXIT

emulator_up() {
    if adb devices | grep -q "^$EMULATOR_SERIAL[[:space:]]*device"; then
        say "emulator $EMULATOR_SERIAL already up"
        return
    fi
    say "booting sim-mailrs-android"
    nohup "$ANDROID_HOME/emulator/emulator" -avd sim-mailrs-android \
        -no-snapshot-save -port "${EMULATOR_SERIAL#emulator-}" > /tmp/mailrs-emulator.log 2>&1 &
    for _ in $(seq 1 60); do
        [ "$(adb -s "$EMULATOR_SERIAL" shell getprop sys.boot_completed 2>/dev/null | tr -d '\r')" = "1" ] && break
        sleep 5
    done
}

case "$MODE" in
    build)
        say "[1/1] assemble"
        (cd android && ./gradlew :app:assembleDebug)
        ;;

    device)
        say "[1/2] assemble"
        (cd android && ./gradlew :app:assembleDebug)
        say "[2/2] install on $SMIX_PHONE"
        smix sim install "$SMIX_PHONE" "$APK"
        echo "    unlock the phone and open Mailrs"
        ;;

    test)
        emulator_up
        start_stub
        say "instrumented: ${FILTER:-all}"
        if [ -n "$FILTER" ]; then
            (cd android && ANDROID_SERIAL="$EMULATOR_SERIAL" ./gradlew :app:connectedDebugAndroidTest \
                -Pandroid.testInstrumentationRunnerArguments.class="$FILTER")
        else
            (cd android && ANDROID_SERIAL="$EMULATOR_SERIAL" ./gradlew :app:connectedDebugAndroidTest)
        fi
        ;;

    all)
        say "[1/3] assemble (warnings are errors)"
        (cd android && ./gradlew :app:assembleDebug :app:assembleDebugAndroidTest)
        say "[2/3] unit tests"
        (cd android && ./gradlew :app:testDebugUnitTest)
        emulator_up
        start_stub
        say "[3/3] instrumented tests on $EMULATOR_SERIAL"
        (cd android && ANDROID_SERIAL="$EMULATOR_SERIAL" ./gradlew :app:connectedDebugAndroidTest)
        say "installing on $SMIX_EMULATOR"
        smix sim install "$SMIX_EMULATOR" "$APK"
        ;;

    *)
        echo "usage: $0 [build|test <filter>|device|all]" >&2
        exit 1
        ;;
esac

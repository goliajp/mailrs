#!/usr/bin/env bash
# ios-shots.sh — photograph the app's screens at a given text size.
#
# The conversation row was broken at the accessibility sizes for the
# whole life of the app and every test passed, because a test reads a
# label and a label is still there when it renders as "A…". The only
# instrument that sees that is an eye; this puts screens in front of one.
#
#   ./scripts/ios-shots.sh                       # default size, dark
#   ./scripts/ios-shots.sh large light           # default size, light
#   ./scripts/ios-shots.sh large dark ja         # Japanese
#   ./scripts/ios-shots.sh accessibility-extra-extra-extra-large
#
# Writes PNGs to ios/.shots/ (gitignored — they are pictures of a stub,
# regenerated in a minute, and nothing downstream reads them).
set -euo pipefail
cd "$(dirname "$0")/../ios"

SIZE="${1:-large}"
# Light is half the users and had never been looked at. Second argument
# rather than a second script: the walk is the same walk.
APPEARANCE="${2:-dark}"
# The catalog being complete says nothing about whether the layout
# survives the translations — 件名 is not Subject, and a row that fits
# one may not fit the other.
LANGUAGE="${3:-en}"
SIM_NAME="sim-mailrs"
STUB_PORT=6039
OUT="$PWD/.shots"

UDID=$(xcrun simctl list devices -j \
  | python3 -c "import json,sys;d=json.load(sys.stdin)['devices'];print(next((x['udid'] for v in d.values() for x in v if x['name']=='$SIM_NAME'), ''))")
[ -n "$UDID" ] || { echo "!! no simulator named $SIM_NAME — run ./scripts/ios-build.sh test once"; exit 1; }

xcodegen generate >/dev/null

# `test`, not `test-without-building`. The faster verb runs whatever
# bundle was built last, so an edit to the test code — a new launch
# argument, a new stop on the walk — silently does not apply, and the
# pictures come back looking exactly like pictures. Light mode was
# photographed dark twice this way.

# The stub the tests read. Killed on the way out whatever happens: a
# leftover listener makes the next run look like it passed against
# yesterday's data.
pkill -f "Testing/stub-api.py" 2>/dev/null || true
MAILRS_STUB_REAL="${MAILRS_STUB_REAL:-}" python3 Testing/stub-api.py "$STUB_PORT" >/tmp/mailrs-ios-shots-stub.log 2>&1 &
STUB_PID=$!
cleanup() {
    kill "$STUB_PID" 2>/dev/null || true
    xcrun simctl ui "$UDID" content_size large >/dev/null 2>&1 || true
    xcrun simctl ui "$UDID" appearance dark >/dev/null 2>&1 || true
}
trap cleanup EXIT

for _ in $(seq 1 20); do
    curl -fsS -o /dev/null "http://127.0.0.1:$STUB_PORT/api/conversations" && break
    sleep 0.25
done

xcrun simctl boot "$UDID" 2>/dev/null || true
xcrun simctl ui "$UDID" content_size "$SIZE"
xcrun simctl ui "$UDID" appearance "$APPEARANCE"

RESULT=$(mktemp -d)/shots.xcresult
# `TEST_RUNNER_` prefix, not a bare variable. The tests run in a process
# on the simulator, which does not inherit this shell's environment;
# xcodebuild strips the prefix and passes the rest through to the
# runner. Set plainly, the class skipped and the run reported success
# with nothing in the output directory.
TEST_RUNNER_MAILRS_SHOTS=1 TEST_RUNNER_MAILRS_SHOTS_APPEARANCE="$APPEARANCE" \
  TEST_RUNNER_MAILRS_SHOTS_LANGUAGE="$LANGUAGE" xcodebuild -project Mailrs.xcodeproj -scheme Mailrs \
  -destination "id=$UDID" \
  -only-testing:MailrsUITests/LayoutShotTests \
  -resultBundlePath "$RESULT" \
  test 2>/dev/null \
  | grep -E "✔|✘|error:|\*\* TEST" || true

rm -rf "$OUT" && mkdir -p "$OUT"
xcrun xcresulttool export attachments --path "$RESULT" --output-path "$OUT" >/dev/null

# The exporter names files by a uuid and puts the human name in the
# manifest; renaming here is what makes the directory readable.
python3 - "$OUT" <<'PY'
import json, os, sys, shutil
out = sys.argv[1]
manifest = os.path.join(out, "manifest.json")
if not os.path.exists(manifest):
    print("!! no manifest — the run produced no attachments")
    raise SystemExit(1)
for entry in json.load(open(manifest)):
    for att in entry.get("attachments", []):
        src = os.path.join(out, att["exportedFileName"])
        name = att.get("suggestedHumanReadableName") or att.get("name") or ""
        if not name or not os.path.exists(src):
            continue
        # The attachment's own name already carries `.png`; appending
        # another produced `01-list.png.png`.
        stem = name[:-4] if name.lower().endswith(".png") else name
        shutil.move(src, os.path.join(out, f"{stem}.png"))
os.remove(manifest)
PY

COUNT=$(ls "$OUT" | wc -l | tr -d ' ')
# Zero is a failure, not a result. The first version of this script
# reported success having taken no pictures at all, which is the same
# shape as a gate that cannot come out red.
if [ "$COUNT" -eq 0 ]; then
    echo "!! no shots — did LayoutShotTests skip? it needs TEST_RUNNER_MAILRS_SHOTS=1"
    exit 1
fi
echo "==> $COUNT shots — size=$SIZE appearance=$APPEARANCE language=$LANGUAGE — in ios/.shots/"
ls "$OUT"

#!/usr/bin/env bash
# check-tracker-parity.sh — every client counts trackers from the same rules.
#
# crates/intake/src/image_prefetch/trackers.json is what the server will not
# fetch; the web reads web/src/lib/trackers.json, iOS ships
# ios/Mailrs/Resources/trackers.json and Android
# android/app/src/main/assets/trackers.json. All four must be the same bytes.
# The behaviour on top of them is held by web/src/lib/tracker-samples.json,
# which the web test reads and the phones' tests read from a byte-identical
# copy beside them.
#
# Usage: check-tracker-parity.sh [repo root]   (default: this repository)
set -euo pipefail
ROOT="${1:-$(cd "$(dirname "$0")/.." && pwd)}"
SOURCE="crates/intake/src/image_prefetch/trackers.json"
COPIES=(
    "web/src/lib/trackers.json"
    "ios/Mailrs/Resources/trackers.json"
    "android/app/src/main/assets/trackers.json"
)
SAMPLES="web/src/lib/tracker-samples.json"
SAMPLE_COPIES=(
    "ios/MailrsTests/tracker-samples.json"
    "android/app/src/test/resources/tracker-samples.json"
)

status=0
for copy in "${COPIES[@]}"; do
    if ! cmp -s "$ROOT/$SOURCE" "$ROOT/$copy"; then
        echo "!! $copy is not the same bytes as $SOURCE"
        status=1
    fi
done
for copy in "${SAMPLE_COPIES[@]}"; do
    if ! cmp -s "$ROOT/$SAMPLES" "$ROOT/$copy"; then
        echo "!! $copy is not the same bytes as $SAMPLES — copy it over"
        status=1
    fi
done
if [ "$status" -eq 0 ]; then
    echo "tracker parity: ${#COPIES[@]} rule copies and ${#SAMPLE_COPIES[@]} sample copies match"
fi
exit "$status"

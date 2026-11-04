#!/usr/bin/env bash
# check-callback-numbers.sh — the three clients point at the same phone
# numbers.
#
# web/src/lib/callback-numbers.json holds the labels whose numbers are not
# phone numbers (PO, Invoice, 注文番号 ...) and samples (text → the numbers
# the reading pane should name). The web reads it; iOS and Android ship
# copies, read the labels from them at run time and test their own rule
# against the samples in them. This checks the copies are the same bytes,
# so a label or sample changed on one side cannot leave the others reading
# yesterday's.
#
# Usage: check-callback-numbers.sh [repo root]   (default: this repository)
set -euo pipefail
ROOT="${1:-$(cd "$(dirname "$0")/.." && pwd)}"
WEB="$ROOT/web/src/lib/callback-numbers.json"
status=0
for copy in ios/Mailrs/Resources/callback-numbers.json android/app/src/main/resources/callback-numbers.json; do
  if ! cmp -s "$WEB" "$ROOT/$copy"; then
    echo "!! $copy differs from web/src/lib/callback-numbers.json — copy it over"
    status=1
  fi
done
if [ "$status" -eq 0 ]; then
  echo "callback numbers: the web, iOS and Android copies are the same bytes"
fi
exit "$status"

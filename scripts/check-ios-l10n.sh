#!/usr/bin/env bash
# check-ios-l10n.sh — no English sentence may bypass the string catalog.
#
# `Text(verbatim:)` is the spelling that says "do not translate this",
# and it is right for data: an address, a date, `p=quarantine`, a "·".
# It is wrong for a sentence, and on 2026-08-09 three of them were
# sentences — each the line in a delete confirmation that says what
# deleting will do. In Chinese and Japanese they appeared in English,
# which is the worst place in the app for that to happen.
#
# The rule: a `verbatim` literal may not contain two or more alphabetic
# words outside its interpolations. `"· \(participants)"` passes,
# `"Cc"` passes, `"its members lose every permission"` does not.
set -euo pipefail
export LC_ALL=C
cd "$(dirname "$0")/../ios"

python3 - <<'PYEOF'
import glob
import re
import sys

LITERAL = re.compile(r'Text\(verbatim:\s*"((?:[^"\\]|\\.)*)"')
found = []
for path in glob.glob("Mailrs/**/*.swift", recursive=True):
    for line_no, line in enumerate(open(path), 1):
        for m in LITERAL.finditer(line):
            # Whatever is left once the interpolated values are gone is
            # what the reader sees in every language.
            fixed = re.sub(r"\\\([^)]*\)", " ", m.group(1))
            words = re.findall(r"[A-Za-z]{2,}", fixed)
            if len(words) >= 2:
                found.append((path, line_no, m.group(1)[:70]))

if not found:
    print("iOS l10n OK — no English sentence bypasses the catalog")
    sys.exit(0)

print("!! these read in English whatever language the phone is in:")
for path, line_no, text in found:
    print(f"    {path}:{line_no}  {text}")
print()
print('Use Text("…") so the catalog picks it up, and add the ja and')
print("zh-Hans values. Text(verbatim:) is for data, not for sentences.")
sys.exit(1)
PYEOF

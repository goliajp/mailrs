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
import json
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

# The other half, and the bigger one. The first version of this check
# asked whether every catalog entry had a ja and a zh-Hans value — it
# did, all 180 of them, and the Japanese settings screen still said
# "Accounts" in English. A string the catalog never received cannot be
# missing a translation; it is simply absent, and shows in English.
# Twenty-six were.
CATALOG = json.load(open("Mailrs/Resources/Localizable.xcstrings"))["strings"]
KEYED = re.compile(
    r'(?:Text|Label|Button|Section|Toggle|Picker|LabeledContent|ContentUnavailableView)\(\s*"([^"\\]{2,})"'
    r'|title:\s*"([^"\\]{2,})"'
    r'|navigationTitle\(\s*"([^"\\]{2,})"'
    r'|placeholder:\s*"([^"\\]{2,})"'
)
absent = []
for path in glob.glob("Mailrs/**/*.swift", recursive=True):
    for line_no, line in enumerate(open(path), 1):
        if "verbatim:" in line:
            continue
        for m in KEYED.finditer(line):
            key = next(g for g in m.groups() if g)
            if key not in CATALOG:
                absent.append((path, line_no, key))

# And the half it did ask about: an entry with no value for a language
# the app ships. "Mailrs" is the app's name and has none on purpose.
untranslated = []
for key, entry in CATALOG.items():
    if key == "Mailrs":
        continue
    for lang in ("ja", "zh-Hans"):
        if not entry.get("localizations", {}).get(lang, {}).get("stringUnit", {}).get("value"):
            untranslated.append((key, lang))

if not found and not absent and not untranslated:
    print(f"iOS l10n OK — {len(CATALOG)} strings, ja and zh-Hans complete")
    sys.exit(0)

if absent:
    print("!! used in the app, never reached the catalog — these show in English:")
    for path, line_no, key in absent:
        print(f"    {path}:{line_no}  {key[:60]}")
    print()

if untranslated:
    print("!! in the catalog with no value for a language the app ships:")
    for key, lang in untranslated:
        print(f"    [{lang}] {key[:60]}")
    print()

if not found:
    sys.exit(1)

print("!! these read in English whatever language the phone is in:")
for path, line_no, text in found:
    print(f"    {path}:{line_no}  {text}")
print()
print('Use Text("…") so the catalog picks it up, and add the ja and')
print("zh-Hans values. Text(verbatim:) is for data, not for sentences.")
sys.exit(1)
PYEOF

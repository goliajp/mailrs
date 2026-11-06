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
# The first version of this skipped any literal with an interpolation in
# it, and "From \(organizer)" on the invitation card showed in English
# in every language for that reason: the catalog had "From", not
# "From %@". An interpolation is a placeholder in the key, so the literal
# is matched against the catalog with each `\(…)` standing for one.
# `AppText.string` is listed because a string resolved in code is the
# other way a key reaches the screen. Scanned over the whole file, so a
# key on the line after its `ContentUnavailableView(` is still seen.
KEYED = re.compile(
    r'(?:\b(?:Text|Label|Button|Section|Toggle|Picker|LabeledContent|ContentUnavailableView'
    r'|TextField|SecureField|navigationTitle|accessibilityLabel|accessibilityHint|alert'
    r'|confirmationDialog|AppText\.string)\(\s*'
    r'|\btitle:\s*|\bplaceholder:\s*|\bprompt:\s*)"'
)
PLACEHOLDER = r"%(?:\d+\$)?(?:@|lld|ld|d|lf|f|\.\d+f)"


def literal_parts(source, i):
    """The text pieces of the string literal opening at `i`, split where
    each interpolation stands; an interpolation may hold strings of its own."""
    parts, piece = [], ""
    while source[i] != '"':
        if source[i] == "\\" and source[i + 1] == "(":
            depth, i = 1, i + 2
            while depth:
                if source[i] == '"':
                    i += 1
                    while source[i] != '"':
                        i += 1 + (source[i] == "\\")
                elif source[i] == "(":
                    depth += 1
                elif source[i] == ")":
                    depth -= 1
                i += 1
            parts.append(piece)
            piece = ""
            continue
        if source[i] == "\\":
            piece += {"n": "\n", '"': '"', "\\": "\\", "t": "\t"}.get(source[i + 1], source[i + 1])
            i += 2
            continue
        piece += source[i]
        i += 1
    parts.append(piece)
    return parts


def in_catalog(parts):
    if len(parts) == 1:
        return parts[0] in CATALOG
    pattern = re.compile("^" + PLACEHOLDER.join(re.escape(p) for p in parts) + "$", re.S)
    return any(pattern.match(key) for key in CATALOG)


absent = []
for path in glob.glob("Mailrs/**/*.swift", recursive=True) + glob.glob("MailrsMac/**/*.swift", recursive=True):
    source = open(path).read()
    for m in KEYED.finditer(source):
        parts = literal_parts(source, m.end())
        words = " ".join(parts)
        # data or a lone interpolation, not words; an example address,
        # host or URL in a placeholder is data too
        if not re.search(r"[A-Za-z]{2,}", words):
            continue
        if len(parts) == 1 and re.fullmatch(r"\S+", words) and re.search(r"@|://|\w\.\w", words):
            continue
        literal = "\\(…)".join(parts)
        line_no = source.count("\n", 0, m.start()) + 1
        line = source.splitlines()[line_no - 1]
        if "verbatim:" in line or line.lstrip().startswith("//"):
            continue
        if not in_catalog(parts):
            absent.append((path, line_no, literal))

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

#!/usr/bin/env bash
# check-wording.sh — the web, iOS and Android call the same things by the
# same words.
#
# web/src/lib/wording.json is the wording table: for each term, its text in
# English, Japanese and Chinese, and the key each client shows it under.
# The web reads its text from the table; iOS and Android keep theirs in the
# string catalogs the systems need, so this checks they have not drifted:
#   1. the two copies (ios/Mailrs/Resources, android/app/src/main/assets)
#      are the same bytes as the web's;
#   2. no client key is claimed by two terms, and every term has all three
#      languages;
#   3. every web key's entry in web/src/i18n is `word('<lang>', '<key>')`,
#      in each of the three languages, so the web shows the table's text;
#   4. each iOS key's value in Localizable.xcstrings (the key itself, or its
#      `en` localization, for English) is the table's text;
#   5. each Android key's value in values, values-ja and values-zh-rCN is the
#      table's text.
# Placeholders are compared by position: the table writes the web's
# `{name}`, iOS `%@` / `%lld`, Android `%1$s` / `%1$d`.
#
# Usage: check-wording.sh [repo root]   (default: this repository)
set -euo pipefail
export LC_ALL=C.UTF-8 2>/dev/null || export LC_ALL=C
ROOT="${1:-$(cd "$(dirname "$0")/.." && pwd)}"

python3 - "$ROOT" <<'PYEOF'
import glob
import json
import os
import re
import sys
import xml.etree.ElementTree as ET

root = sys.argv[1]
WEB = "web/src/lib/wording.json"
COPIES = ["ios/Mailrs/Resources/wording.json", "android/app/src/main/assets/wording.json"]
LANGS = ("en", "ja", "zh")
problems = []


def read(rel):
    with open(os.path.join(root, rel), encoding="utf-8") as f:
        return f.read()


text = read(WEB)
for copy in COPIES:
    if read(copy) != text:
        problems.append(f"{copy} differs from {WEB}")

terms = json.loads(text)["terms"]
ids = [t["id"] for t in terms]
if len(ids) != len(set(ids)):
    problems.append("an id is listed twice in wording.json")
for client in ("web", "ios", "android"):
    seen = {}
    for t in terms:
        for k in t[client]:
            if k in seen:
                problems.append(f"{client} key {k!r} is claimed by {seen[k]} and {t['id']}")
            seen[k] = t["id"]
for t in terms:
    for lang in LANGS:
        if not t["text"].get(lang):
            problems.append(f"{t['id']} has no {lang} text")

WEB_SLOT = re.compile(r"\{\w+\}")
IOS_SLOT = re.compile(r"%(?:\d+\$)?(?:@|lld|ld|d)")
ANDROID_SLOT = re.compile(r"%\d+\$[sd]")


def shape(s, slot):
    return slot.sub("\u2022", s)


# 3. the web
web_files = {lang: [] for lang in LANGS}
for path in sorted(glob.glob(os.path.join(root, "web/src/i18n/**/*.ts"), recursive=True)):
    name = os.path.basename(path)[:-3]
    if name in LANGS:
        web_files[name].append(read(os.path.relpath(path, root)))
for t in terms:
    for k in t["web"]:
        for lang in LANGS:
            # `\s*` after the colon also takes the line break prettier puts
            # there when the entry is too long for one line
            line = re.compile(r"^\s*'" + re.escape(k) + r"':\s*(.*?),?[ \t]*$", re.M)
            found = [m.group(1) for src in web_files[lang] for m in line.finditer(src)]
            want = f"word('{lang}', '{k}')"
            if found != [want]:
                problems.append(f"web {lang} {k}: {found or 'missing'}, should be {want}")

# 4. iOS
catalog = json.loads(read("ios/Mailrs/Resources/Localizable.xcstrings"))["strings"]
IOS_LANG = {"en": "en", "ja": "ja", "zh": "zh-Hans"}
for t in terms:
    for k in t["ios"]:
        entry = catalog.get(k)
        if entry is None:
            problems.append(f"ios {k!r} ({t['id']}) is not in Localizable.xcstrings")
            continue
        for lang in LANGS:
            unit = entry.get("localizations", {}).get(IOS_LANG[lang], {}).get("stringUnit")
            value = unit["value"] if unit else (k if lang == "en" else None)
            if value is None or shape(value, IOS_SLOT) != shape(t["text"][lang], WEB_SLOT):
                problems.append(f"ios {lang} {k!r}: {value!r}, the table says {t['text'][lang]!r}")


# 5. Android
def strings(folder):
    out = {}
    path = f"android/app/src/main/res/{folder}/strings.xml"
    for s in ET.fromstring(read(path)).iter("string"):
        v = "".join(s.itertext())
        if len(v) >= 2 and v[0] == '"' and v[-1] == '"':
            v = v[1:-1]
        out[s.get("name")] = re.sub(r"\\(['\"@?])", r"\1", v)
    return out


ANDROID_FOLDER = {"en": "values", "ja": "values-ja", "zh": "values-zh-rCN"}
android = {lang: strings(f) for lang, f in ANDROID_FOLDER.items()}
for t in terms:
    for k in t["android"]:
        for lang in LANGS:
            value = android[lang].get(k)
            if value is None or shape(value, ANDROID_SLOT) != shape(t["text"][lang], WEB_SLOT):
                problems.append(f"android {lang} {k}: {value!r}, the table says {t['text'][lang]!r}")

if problems:
    print("!! the three clients' wording disagrees with wording.json:")
    for p in problems:
        print("   " + p)
    sys.exit(1)
keys = sum(len(t[c]) for t in terms for c in ("web", "ios", "android"))
print(f"wording: {len(terms)} terms, {keys} client keys, the same text on web, iOS and Android")
PYEOF

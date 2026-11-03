#!/usr/bin/env bash
# check-android-l10n.sh — the Android app speaks English, Japanese and Chinese.
#
# Two checks:
#   1. res/values, values-ja and values-zh-rCN carry the same keys and the
#      same placeholders, and the Japanese and Chinese are punctuated as
#      CJK (no trailing full stop on interface text, no half-width comma
#      between Han characters). A key in English only shows English on a
#      Japanese phone and nothing fails.
#   2. No string literal in the Kotlin sources looks like interface text
#      unless android/l10n-allowlist.txt says it is not (protocol words,
#      file names, formats, brand names). A new literal shown to a person
#      belongs in strings.xml; one that is not shown goes in the allowlist,
#      with its file, so the exemption covers that place only.
set -euo pipefail
export LC_ALL=C.UTF-8 2>/dev/null || export LC_ALL=C
cd "$(dirname "$0")/.."

python3 - <<'PYEOF'
import glob
import os
import re
import sys
import xml.etree.ElementTree as ET

RES = "android/app/src/main/res"
SRC = "android/app/src/main/kotlin/jp/golia/mailrs"
ALLOW = "android/l10n-allowlist.txt"
problems = []


def table(folder):
    out, fixed = {}, set()
    for s in ET.parse(f"{RES}/{folder}/strings.xml").getroot().iter("string"):
        out[s.get("name")] = s.text or ""
        if s.get("translatable") == "false":
            fixed.add(s.get("name"))
    return out, fixed


placeholder = re.compile(r"%\d\$[sd]")
en, fixed = table("values")
for folder in ("values-ja", "values-zh-rCN"):
    other, _ = table(folder)
    want = set(en) - fixed
    for k in sorted(want - set(other)):
        problems.append(f"{folder}: missing {k}")
    for k in sorted(set(other) - want):
        problems.append(f"{folder}: {k} is not in values (or is marked untranslatable there)")
    for k, text in other.items():
        if k not in en:
            continue
        if sorted(placeholder.findall(text)) != sorted(placeholder.findall(en[k])):
            problems.append(f"{folder}: {k} has placeholders {placeholder.findall(text)}, English has {placeholder.findall(en[k])}")
        if text.endswith("。"):
            problems.append(f"{folder}: {k} ends with a full stop: {text}")
        if re.search(r"[一-鿿], ?[一-鿿]", text):
            problems.append(f"{folder}: {k} has a half-width comma between CJK: {text}")

literal = re.compile(r'"((?:[^"\\$]|\\.|\$\{[^}]*\}|\$[A-Za-z_][A-Za-z0-9_.]*)*)"')
skip_line = re.compile(r'^\s*(//|\*|/\*|import |package |@)|Log\.|testTag\(|println|require\(|check\(|error\("')
not_text = re.compile(r'^(/|https?:|mailto:|application/|text/|image/|multipart|[a-z0-9_.\-]+$|[A-Z_0-9]+$|%|\*|#|\.)')
allowed = set()
for line in open(ALLOW, encoding="utf-8"):
    line = line.rstrip("\n")
    if line and not line.startswith("#"):
        allowed.add(line)
seen = set()
for path in sorted(glob.glob(f"{SRC}/**/*.kt", recursive=True)):
    rel = os.path.relpath(path, SRC)
    for n, ln in enumerate(open(path, encoding="utf-8"), 1):
        if skip_line.search(ln):
            continue
        for m in literal.finditer(ln):
            v = m.group(1)
            if not re.search(r"[A-Za-z]{2,}", v) or not_text.match(v) or re.match(r"^[a-z][a-zA-Z0-9]*$", v):
                continue
            entry = f"{rel}\t{v}"
            seen.add(entry)
            if entry not in allowed:
                problems.append(f"{rel}:{n}: \"{v}\" looks like interface text; put it in strings.xml, or in {ALLOW} if it is not shown")
for entry in sorted(allowed - seen):
    problems.append(f"{ALLOW}: no longer in the source, remove it: {entry!r}")

if problems:
    print("\n".join(problems))
    sys.exit(1)
print(f"android l10n: {len(en)} strings in three languages; {len(seen)} non-interface literals allowed")
PYEOF

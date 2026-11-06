#!/usr/bin/env bash
# check-action-parity.sh — the web, iOS and Android offer the same actions.
#
# web/src/lib/actions.json is the action table. The web builds its table
# from it; iOS and Android keep their own typed enums and test them field by
# field against the copy each ships. This checks what those tests cannot:
#   1. the two copies (ios/Mailrs/Resources, android/app/src/main/assets) are
#      the same bytes as the web's;
#   2. each surface's code declares exactly the actions the table lists for
#      it in `surfaces` — the web's ActionId union, iOS's `RowAction` cases,
#      Android's `RowAction` entries;
#   3. every `undo` names an action the table has;
#   4. the swipe choices and defaults (`swipe`) are the server's SwipeAction
#      and Swipe::default (crates/core-sidestate/src/families/swipe.rs).
# An action added on one surface and not the table, or dropped from one, fails
# here rather than as a menu item one phone lacks.
#
# Usage: check-action-parity.sh [repo root]   (default: this repository)
set -euo pipefail
export LC_ALL=C.UTF-8 2>/dev/null || export LC_ALL=C
ROOT="${1:-$(cd "$(dirname "$0")/.." && pwd)}"

python3 - "$ROOT" <<'PYEOF'
import json
import os
import re
import sys

root = sys.argv[1]
WEB = "web/src/lib/actions.json"
COPIES = ["ios/Mailrs/Resources/actions.json", "android/app/src/main/assets/actions.json"]
problems = []


def read(rel):
    with open(os.path.join(root, rel), encoding="utf-8") as f:
        return f.read()


text = read(WEB)
for copy in COPIES:
    if read(copy) != text:
        problems.append(f"{copy} differs from {WEB}")

entries = json.loads(text)["actions"]
ids = [e["id"] for e in entries]
if len(ids) != len(set(ids)):
    problems.append("an id is listed twice in actions.json")
for e in entries:
    if e["undo"] is not None and e["undo"] not in ids:
        problems.append(f"{e['id']} is undone by {e['undo']}, which the table does not have")


def kebab(name):
    return re.sub(r"(?<!^)([A-Z])", r"-\1", name).lower()


def web_ids():
    src = read("web/src/lib/actions.ts")
    block = re.search(r"export type ActionId =(.*?)\n\n", src, re.S).group(1)
    return set(re.findall(r"'([a-z-]+)'", block))


def ios_ids():
    src = read("ios/Mailrs/Wire/RowActions.swift")
    body = re.search(r"enum RowAction\b[^{]*\{(.*?)\n    /// The catalog key", src, re.S).group(1)
    out = set()
    for line in body.splitlines():
        m = re.match(r"\s*case\s+(.+)$", line)
        if m:
            out |= {kebab(w.strip()) for w in m.group(1).split(",")}
    return out


def android_ids():
    src = read("android/app/src/main/kotlin/jp/golia/mailrs/wire/RowActions.kt")
    return {kebab(n) for n in re.findall(r"^\s{4}([A-Z]\w*)\(R\.string\.", src, re.M)}


for surface, found in (("web", web_ids()), ("ios", ios_ids()), ("android", android_ids())):
    listed = {e["id"] for e in entries if surface in e["surfaces"]}
    for a in sorted(found - listed):
        problems.append(f"{surface} offers {a}, which actions.json does not list for it")
    for a in sorted(listed - found):
        problems.append(f"actions.json lists {a} for {surface}, which {surface} does not offer")

swipe = json.loads(text)["swipe"]
for c in swipe["choices"]:
    if c != "none" and c not in ids:
        problems.append(f"swipe choice {c} is not an action in the table")
for side, c in swipe["default"].items():
    if c not in swipe["choices"]:
        problems.append(f"the default {side} swipe {c} is not a swipe choice")
rust = read("crates/core-sidestate/src/families/swipe.rs")
variants = re.search(r"pub enum SwipeAction \{(.*?)\}", rust, re.S).group(1)
if [v.lower() for v in re.findall(r"(\w+),", variants)] != swipe["choices"]:
    problems.append("the server's SwipeAction is not the table's swipe choices, in order")
default = re.search(r"impl Default for Swipe \{.*?\n\}", rust, re.S).group(0)
for side, c in swipe["default"].items():
    m = re.search(side + r": SwipeAction::(\w+)", default)
    if m is None or m.group(1).lower() != c:
        problems.append(f"the server's default {side} swipe is not the table's {c}")

if problems:
    print("!! the three surfaces' actions disagree:")
    for p in problems:
        print("   " + p)
    sys.exit(1)
print(f"actions: {len(entries)} in the table, the same on web, iOS and Android")
PYEOF

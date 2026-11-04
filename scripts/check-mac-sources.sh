#!/usr/bin/env bash
# check-mac-sources.sh — the Mac target compiles only what it lists.
#
# `MailrsMac` takes `App/` (less the phone's shell) and a hand-picked
# set of `Features/` files: the phone's screens stay out on purpose.
# A shared view that starts using a new Features file therefore needs
# that file added to the Mac list too, and forgetting it is invisible
# until somebody builds the Mac app — which is how the Mac stopped
# compiling for a week.
#
# This reads the Mac target's sources from `ios/project.yml`, collects
# the types and functions declared in the Features files it does not
# take, and fails when any file it does take names one of them. It is a
# text check, not a compiler: a name declared in both an excluded and an
# included file is skipped, and so are names shorter than six letters.
set -euo pipefail
cd "$(dirname "$0")/../ios"

python3 - <<'PY'
import re
import sys
from pathlib import Path

spec = Path("project.yml").read_text().splitlines()

# the `MailrsMac:` target's `sources:` block; a scheme of the same name
# comes first, and it has no `type:`
start = next(
    i
    for i, l in enumerate(spec)
    if l.rstrip() == "  MailrsMac:" and spec[i + 1].startswith("    type:")
)
paths, excludes = [], {}
current = None
i = spec.index("    sources:", start) + 1
while i < len(spec) and (spec[i].startswith("      ") or not spec[i].strip()):
    line = spec[i].strip()
    if line.startswith("- path: "):
        current = line[len("- path: "):]
        paths.append(current)
        excludes[current] = []
    elif line.startswith("- ") and current and spec[i].startswith("          "):
        excludes[current].append(line[2:])
    i += 1

included = set()
for p in paths:
    root = Path(p)
    if root.is_file():
        included.add(root)
    elif root.is_dir():
        for f in root.rglob("*.swift"):
            if f.name not in excludes.get(p, []):
                included.add(f)

features = set(Path("Mailrs/Features").glob("*.swift"))
left_out = features - included

# what another file can name: a top-level type, or a non-private function
# at the top level or one level into an extension. A view's own private
# helpers (`create`, `discard`) are not reachable from elsewhere.
top_type = re.compile(
    r"^(?:@\w+\s+)*(?:public\s+|internal\s+)?(?:final\s+)?"
    r"(?:struct|class|enum|protocol|typealias)\s+(\w+)"
)
shared_func = re.compile(r"^(?: {4})?(?:@\w+\s+)*(?:public\s+|internal\s+|static\s+)*func\s+(\w+)")


def code(f):
    # comments name files and types too, and naming is not using
    return re.sub(r"//.*", "", f.read_text())


def declared(files):
    names = {}
    for f in files:
        for line in code(f).splitlines():
            if "private" in line:
                continue
            m = top_type.match(line) or shared_func.match(line)
            if m and len(m.group(1)) >= 6:
                names.setdefault(m.group(1), f)
    return names

missing_names = declared(left_out)
for name in declared(included):
    missing_names.pop(name, None)

problems = []
for f in sorted(included):
    text = code(f)
    for name, home in missing_names.items():
        if re.search(r"(?<![\w.])\.?" + re.escape(name) + r"\b", text):
            problems.append(f"{f}: uses {name}, declared in {home}, which MailrsMac does not compile")

if problems:
    print("\n".join(sorted(set(problems))))
    print("!! add those files to the MailrsMac sources in ios/project.yml")
    sys.exit(1)
print(f"ok: {len(included)} Mac sources, none names a Features file left out ({len(left_out)} left out)")
PY

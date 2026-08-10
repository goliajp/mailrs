#!/usr/bin/env bash
# check-inert-fields.sh — a field written to a thread hash must be read.
#
# `snoozed_until` was written by `set_snoozed` and parsed by nothing:
# not on `ThreadRow`, so `GET /api/conversations` never returned it,
# so the web's `snoozed_until: z.number().nullish()` was satisfied by
# its absence on every row, so the snooze action dropped the row
# optimistically and the next refetch brought it back. The feature has
# never done anything, and every layer looked correct on its own.
#
# The write sites and the parse arms are both literals in the same
# crate, which makes this mechanical.
set -euo pipefail
export LC_ALL=C
cd "$(dirname "$0")/.."

python3 <<'PYEOF'
import glob
import re
import sys

written = {}
for path in glob.glob("crates/mailbox-kevy/src/**/*.rs", recursive=True):
    if "tests" in path:
        continue
    for line_no, line in enumerate(open(path), 1):
        # `(b"field", value)` and `(b"field" as &[u8], value)` — the
        # two spellings an hset pair takes here.
        for m in re.finditer(r'\(\s*b"([a-z_]+)"(?:\s+as\s+&\[u8\])?\s*,', line):
            written.setdefault(m.group(1), (path, line_no))

read = set()
for path in glob.glob("crates/mailbox-kevy/src/**/*.rs", recursive=True):
    text = open(path).read()
    # The decode arms: `"field" => ...`
    read.update(re.findall(r'"([a-z_]+)"\s*=>', text))
    # And anything named in a projection list the row is built from.
    read.update(re.findall(r'kv!\("([a-z_]+)"', text))
    # A declared column is read by the engine, not by a match arm —
    # `unread` has no decode arm and is queried by name off the index,
    # which is the whole point of declaring it. Treat a column
    # declaration, and a query naming a flag, as readers.
    read.update(re.findall(r'col\(\s*"([a-z_]+)"', text))
    read.update(re.findall(r'_via_table\([^,]*,\s*"([a-z_]+)"', text))

inert = sorted(f for f in written if f not in read)
if inert:
    print("!! written to a thread hash, parsed by no reader:")
    for field in inert:
        path, line_no = written[field]
        print(f"   {field:20} {path}:{line_no}")
    print("\n   A field nothing reads is a feature nothing does. Either give")
    print("   the row a decode arm for it, or stop writing it.")
    sys.exit(1)
print(f"thread hash fields: {len(written)} written, all parsed")
PYEOF

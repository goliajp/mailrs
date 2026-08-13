#!/usr/bin/env bash
# check-blocking-pops.sh — a blocking kevy pop must not run on a runtime
# worker.
#
# `BRPOP` / `BLPOP` / `BZPOPMIN` block the calling thread. Called from an
# async context without `spawn_blocking`, the call pins one tokio worker for
# as long as it waits — and if the surrounding loop has no `.await` in it,
# for the life of the process. Two consequences, both measured on
# `fastcore/src/bounce.rs` on 2026-08-13:
#
#   - one of four workers gone on a four-core host, permanently;
#   - the process cannot shut down. A worker parked in a blocking syscall
#     with no yield point never observes the runtime's shutdown, so fastcore
#     flushed kevy on SIGTERM, logged that it had, and then sat there. It
#     exited in 0.55 s without `MAILRS_KEVY_URL` and was still alive 40 s
#     later with one set — the production configuration. Every deploy was
#     waiting out `docker stop`'s grace period and taking a SIGKILL.
#
# `.claude/rules/kevy-patterns.md` → `kevy/no-blocking-pop-wrap` has required
# the wrapper since kevy-client 1.14, and listed that exact call site among
# its compliant callers. Prose could not tell that it had stopped being true.
#
# The check is per file, not per call: a file that reaches for a blocking pop
# must also *call* `spawn_blocking`. That is coarse — it cannot prove the pop
# is inside the wrapper — but it is mechanical, and it separates the two
# callers that exist. Anything finer needs to parse Rust.
#
# Comments are stripped before either side is looked for, and that is not
# fastidiousness: the first version of this gate matched the bare word, and
# the paragraph above — which explains the wrapper — was enough to make the
# defect it was written for pass. A check a comment can satisfy is a check
# that reports on its own documentation.
#
# A caller that genuinely has no runtime — a plain `fn main`, a dedicated
# `std::thread` — belongs in the allow-file with the reason.
#
# Exit 0 = every file with a blocking pop also wraps.
set -euo pipefail
cd "$(dirname "$0")/.."
export LC_ALL=C

ALLOW=scripts/blocking-pops-allowed.txt

# Found by content. The verbs, as method calls — `.brpop(` rather than
# `brpop` — so a doc comment discussing the pattern is not a finding, which
# is what the prose rule's own text would otherwise trip.
#
# `while read` rather than `mapfile`: macOS ships bash 3.2, where mapfile
# does not exist, and this gate has to run on the machine people develop on.
report="$(python3 - "$ALLOW" <<'PY'
import re, sys
from pathlib import Path

allow_path = Path(sys.argv[1])
allowed = set()
if allow_path.exists():
    for line in allow_path.read_text().splitlines():
        line = line.split("#", 1)[0].strip()
        if line:
            allowed.add(line)

POP = re.compile(r"\.(brpop|blpop|bzpopmin|bzpopmax)\(")
WRAP = re.compile(r"\bspawn_blocking\(")
# Line comments only. Block comments are rare here and a `/* */` around a
# blocking pop would be dead code, which is a different problem.
COMMENT = re.compile(r"//.*$")

total, bare = 0, []
for path in sorted(Path("crates").rglob("*.rs")):
    code = [COMMENT.sub("", ln) for ln in path.read_text().splitlines()]
    pops = [(i + 1, ln.strip()) for i, ln in enumerate(code) if POP.search(ln)]
    if not pops:
        continue
    total += 1
    if any(WRAP.search(ln) for ln in code):
        continue
    if str(path) in allowed:
        continue
    bare.append((str(path), pops))

print(f"COUNT {total}")
for path, pops in bare:
    print(f"BARE {path}")
    for lineno, text in pops:
        print(f"  AT {lineno}: {text}")
PY
)"

count="$(printf '%s\n' "$report" | awk '/^COUNT /{print $2}')"
echo "files with a blocking kevy pop: ${count:-0}"

if ! printf '%s\n' "$report" | grep -q '^BARE '; then
    if [ "${count:-0}" = 0 ]; then
        echo "no blocking pops in the tree"
    else
        echo "blocking pops OK — every one is on a blocking thread"
    fi
    exit 0
fi

echo
echo "!! A BLOCKING POP IS RUNNING ON A RUNTIME WORKER"
printf '%s\n' "$report" | sed -n 's/^BARE /  /p; s/^  AT /      /p'
echo
echo "Wrap the loop in tokio::task::spawn_blocking. If this caller has no"
echo "runtime at all, add the path to $ALLOW with the reason."
exit 1

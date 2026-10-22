#!/usr/bin/env bash
# remote-gate.sh — the debug half of the deploy gate, on a Linux runner.
#
# Usage: ./scripts/remote-gate.sh <commit>
#   GATE_HOST (default lx64)                  ssh host that runs it
#   GATE_DIR  (default /root/work/mailrs-gate) checkout on that host; its
#                                             target/ stays there between runs
#
# clippy, the spg axis of the SQL core and the whole test suite. It runs on
# Linux because two of its costs exist only on a Mac: every freshly linked
# test binary is checked by Gatekeeper on its first exec (117 s across 231
# binaries in one measured run), and container tests go through a VM
# (221 s there, 87 s on the Linux runner). The release perf budgets stay
# on the Mac they were measured on; see perf-gates.sh.
#
# What is tested is the commit, not the working directory: `git archive`
# of the given sha. It is unpacked beside the checkout and copied in with
# `rsync --checksum` and without -t, so a file whose content changed gets
# the runner's current time and one that did not keeps its old one. Tar's
# own timestamps are commit times, which can be older than the runner's
# last build, and cargo would then keep the old binary and test that.
set -euo pipefail

SHA="${1:?usage: remote-gate.sh <commit>}"
HOST="${GATE_HOST:-lx64}"
DIR="${GATE_DIR:-/root/work/mailrs-gate}"
cd "$(dirname "$0")/.."

echo "    remote gate on $HOST:$DIR at $(git rev-parse --short "$SHA")"
git archive --format=tar "$SHA" | gzip -1 | ssh "$HOST" "set -e
  rm -rf '$DIR.stage' && mkdir -p '$DIR.stage' '$DIR'
  tar xz -C '$DIR.stage'
  rsync -rlpD --checksum --delete --exclude /target/ '$DIR.stage/' '$DIR/'
  rm -rf '$DIR.stage'"

# Two time limits, because nothing else ends a hung run: on 2026-10-18 a
# test binary spun for two and a half days, and its run outlived the ssh
# session that started it. Each test binary gets 10 minutes (the slowest
# took 26 s on a busy runner, 2026-10-21); the whole gate gets 90, enough
# for a cold build of the workspace. timeout kills its process group, so
# cargo and the tests it started go with it.
ssh "$HOST" timeout --kill-after=30 90m bash -s <<EOF
set -euo pipefail
cd '$DIR'
source ~/.cargo/env
triple=\$(rustc -vV | sed -n 's/^host: //p' | tr 'a-z-' 'A-Z_')
export "CARGO_TARGET_\${triple}_RUNNER=timeout --kill-after=30 10m"

# testcontainers goes through bollard, which does not pick up the
# credentials the Docker CLI has, so a cold pull fails with
# "401 authentication required" and reads like an auth bug rather
# than a missing image. Warm them first; each is a no-op once cached.
docker pull -q postgres:11-alpine >/dev/null
docker pull -q axllent/mailpit:latest >/dev/null
docker pull -q pgvector/pgvector:pg18 >/dev/null

cargo clippy --workspace --all-targets -- -D warnings

# rustdoc, which clippy does not run. Here rather than on the Mac
# because docs.rs builds on Linux, and some modules exist only there.
bash scripts/check-docs.sh

# The SQL core's spg axis, which nothing above reaches: the feature is
# off by default, so --workspace --all-targets builds the SQL core over
# real PostgreSQL only. --all-targets matters: the in-memory pg-core
# suite needs spg, the real-Postgres sync test needs it off, so each
# axis type-checks a different half of the tests.
cargo check -p mailrs-server --features spg --all-targets

# The core-api contract and the protocol conversations on in-memory spg.
# The real-PostgreSQL axis of the same suites runs in the workspace test
# below; each engine has accepted SQL the other rejects.
cargo test -p mailrs-server --features spg --lib --no-fail-fast -- core_rpc::tests protocols::tests

# The OCR tests read real models; kept in target/, which survives syncs.
sh scripts/fetch-ocr-models.sh

# --no-fail-fast: cargo stops at the first failing test binary, which
# hides every red behind it. Three separate failures were found this
# way on 2026-07-29, each only after the previous one was fixed.
cargo test --workspace --no-fail-fast
EOF

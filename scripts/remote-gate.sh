#!/usr/bin/env bash
# remote-gate.sh — the debug half of the deploy gate, on a Linux runner.
#
# Usage: ./scripts/remote-gate.sh <commit>
#   GATE_HOST (default lx64)                  ssh host that runs it
#   GATE_DIR  (default /root/work/mailrs-gate) checkout on that host; its
#                                             target/ stays there between runs
#
# clippy, the two dormant-lane checks and the whole test suite. It runs on
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

ssh "$HOST" bash -s <<EOF
set -euo pipefail
cd '$DIR'
source ~/.cargo/env

# testcontainers goes through bollard, which does not pick up the
# credentials the Docker CLI has, so a cold pull fails with
# "401 authentication required" and reads like an auth bug rather
# than a missing image. Warm them first; each is a no-op once cached.
docker pull -q postgres:11-alpine >/dev/null
docker pull -q axllent/mailpit:latest >/dev/null
docker pull -q pgvector/pgvector:pg18 >/dev/null

cargo clippy --workspace --all-targets -- -D warnings

# The dormant pg/spg lane, which nothing above reaches: features are off
# by default, so --workspace --all-targets never sees the code behind
# spg or core-rpc, and test.yml — the only thing that did build them —
# runs on master/release/hotfix, whose tip predates develop by weeks.
#
# It had been broken since the 2026-08-02 file split, in 23 places. All
# of them one root cause: that commit moved these files a directory
# deeper and nothing that referenced position followed — relative
# include_str! paths, super::* globs, and a re-export widened past its
# own visibility. Plus a cfg with two comma-separated predicates, which
# is malformed, so that file had never compiled at all.
#
# --all-targets matters and both axes matter: the lane's two test files
# sit on opposite sides of the spg switch (the in-memory pg-core suite
# needs it, the real-Postgres bidirectional sync test needs it off), so
# a single invocation type-checks exactly one of them. Test-only rot is
# the kind this lane had.
cargo check -p mailrs-server --features core-rpc,spg --all-targets
cargo check -p mailrs-server --features core-rpc --all-targets

# --no-fail-fast: cargo stops at the first failing test binary, which
# hides every red behind it. Three separate failures were found this
# way on 2026-07-29, each only after the previous one was fixed.
cargo test --workspace --no-fail-fast
EOF

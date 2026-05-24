#!/usr/bin/env bash
# Pre-flight security gate: cargo audit + cargo deny.
# Fails non-zero on any unhandled vulnerability, license issue,
# advisory, or registry source.
#
# Run before release.sh. Both tools require:
#   cargo install cargo-audit cargo-deny
set -euo pipefail

cd "$(dirname "$0")/.."

echo "==> cargo audit"
cargo audit

echo ""
echo "==> cargo deny check"
cargo deny check

echo ""
echo "OK: cargo audit + cargo deny clean"

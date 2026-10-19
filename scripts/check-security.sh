#!/usr/bin/env bash
# Pre-flight security gate: cargo audit + cargo deny + osv-scanner.
# Fails non-zero on any unhandled vulnerability, license issue,
# advisory, or registry source.
#
# Part of the direct-deploy.sh gate. Needs network (advisory database,
# crates.io index for yanked checks) and the tools:
#   cargo install cargo-audit cargo-deny
#   brew install osv-scanner
set -euo pipefail

cd "$(dirname "$0")/.."

echo "==> cargo audit"
cargo audit

echo ""
echo "==> cargo deny check"
cargo deny check

echo ""
./scripts/check-osv.sh

echo ""
echo "OK: cargo audit + cargo deny + osv-scanner clean"

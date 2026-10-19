#!/usr/bin/env bash
# Known-vulnerability check over every tracked lockfile, against the OSV
# database, which carries GitHub's advisories (GHSA) as well as RustSec's.
# cargo audit reads RustSec alone and treats "unsound" advisories as
# warnings, so advisories GitHub alone published, and npm packages, went
# past the gate unseen. Accepted findings, each with its reason, live in
# osv-scanner.toml.
#
#   brew install osv-scanner
set -euo pipefail
cd "$(dirname "$0")/.."

args=()
while IFS= read -r f; do
    args+=(--lockfile "$f")
done < <(git ls-files | grep -E '(^|/)(Cargo\.lock|bun\.lock)$')

echo "==> osv-scanner over ${#args[@]} lockfile argument(s)"
osv-scanner scan --config osv-scanner.toml "${args[@]}"

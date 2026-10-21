#!/usr/bin/env bash
# check-docs.sh — the workspace documentation builds with no warnings.
#
# The crates are published, and docs.rs builds their documentation; a
# broken intra-doc link or a stray `<placeholder>` read as an HTML tag
# fails that build, and nothing else in the gate runs rustdoc.
set -euo pipefail
cd "$(dirname "$0")/.."

RUSTDOCFLAGS="-D warnings" cargo doc --workspace --no-deps --keep-going

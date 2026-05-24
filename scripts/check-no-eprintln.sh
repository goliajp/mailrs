#!/usr/bin/env bash
# Forbid eprintln!/println! in production server code.
# Tests, benches, and the bench-print in smtp_session/headers.rs are allowed.
# Run before release; fails fast if a regression slips in.
set -euo pipefail

cd "$(dirname "$0")/.."

# Build exclude list: integration test files + the one #[cfg(test)] bench
# block in smtp_session/headers.rs that uses eprintln for nocapture output.
hits=$(grep -rn 'eprintln!' crates/server/src \
    --include='*.rs' \
    | grep -vE '/tests/|/benches/|tests/integration\.rs|smtp_session/headers\.rs' \
    || true)

if [ -n "$hits" ]; then
    echo "ERROR: forbidden eprintln! in production code:" >&2
    echo "$hits" >&2
    echo "" >&2
    echo "Use tracing::{error,warn,info,debug}! with event= field instead." >&2
    echo "See REFACTOR-V2-v0.4-log-audit.md for schema." >&2
    exit 1
fi

println_hits=$(grep -rn 'println!' crates/server/src \
    --include='*.rs' \
    | grep -v eprintln \
    || true)

if [ -n "$println_hits" ]; then
    echo "ERROR: forbidden println! in production code:" >&2
    echo "$println_hits" >&2
    exit 1
fi

echo "OK: no forbidden eprintln!/println! in production server code"

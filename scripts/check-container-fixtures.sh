#!/usr/bin/env bash
# check-container-fixtures.sh — every test that starts a container takes
# the shared startup lock and the shared timeout.
#
# `cargo test --workspace` runs 231 test binaries at once, and six
# fixtures across four crates each start their own container. Under that
# spike a start takes minutes: on 2026-08-17 eleven container-backed
# tests took 541 s together and 55 s alone, and nine tests across three
# crates failed two separate release gates on `WaitContainer(
# StartupTimeout)`.
#
# `mailrs-test-docker` exists to fix that once. Its own doc comment said
# "six fixtures in four crates" — and `crates/outbound-queue/tests/
# common/pg.rs` was not one of them, which is where six of the nine
# failures came from. A shared fix that a caller can silently not use is
# the same defect as no fix.
set -euo pipefail
export LC_ALL=C
cd "$(dirname "$0")/.."

fail=0
while IFS= read -r file; do
    case "$file" in
        crates/test-docker/*) continue ;;
    esac
    if ! grep -q "startup_lock" "$file"; then
        echo "!! $file starts a container without mailrs_test_docker::startup_lock()"
        fail=1
    fi
    if ! grep -q "STARTUP_TIMEOUT" "$file"; then
        echo "!! $file starts a container without mailrs_test_docker::STARTUP_TIMEOUT"
        fail=1
    fi
done < <(grep -rl "AsyncRunner\|\.start()" --include='*.rs' crates/*/tests crates/*/src 2>/dev/null \
         | xargs grep -l "testcontainers\|GenericImage\|Postgres::default\|Mailpit" 2>/dev/null || true)

if [ "$fail" -ne 0 ]; then
    echo
    echo "A fixture that opts out moves the contention rather than removing it."
    exit 1
fi
echo "container fixtures OK — every start is serialised and given time"

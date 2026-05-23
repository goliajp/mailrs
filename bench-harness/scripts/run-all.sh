#!/usr/bin/env bash
# Cross-language bench harness driver.
#
# Builds and runs every available runner (Rust always; C and Go are
# best-effort — skipped with a note if the toolchain / library isn't
# installed). Captures output to results/REPORT.md.
set -uo pipefail

cd "$(dirname "$0")/.."
ROOT="$(pwd)"
RESULTS="$ROOT/results"
REPORT="$RESULTS/REPORT.md"
mkdir -p "$RESULTS"

ITERS_FAST=1000000      # ~1M iters for sub-µs ops
ITERS_MED=100000        # ~100K iters for µs-scale

echo "# Cross-language bench harness — $(date -u +%FT%TZ)" >> "$REPORT"
echo "" >> "$REPORT"
echo "Run from $(uname -smr)" >> "$REPORT"
echo '```' >> "$REPORT"

# --- Rust runners (always available) ---
echo "Building Rust runners..."
(cd rust-runner && cargo build --release 2>&1 | tail -3) >> "$REPORT" 2>&1 || true
echo "## Rust" >> "$REPORT"
# Cargo respects CARGO_TARGET_DIR, so the binaries may not be under
# rust-runner/target/release. Use `cargo metadata` to locate them.
RUST_BIN="$(cd rust-runner && cargo metadata --no-deps --format-version 1 2>/dev/null \
    | python3 -c 'import json,sys; print(json.load(sys.stdin)["target_directory"])')/release"
"$RUST_BIN/spf"     corpus/spf_simple.txt        "$ITERS_FAST" 2>&1 | tee -a "$REPORT" || true
"$RUST_BIN/spf"     corpus/spf_complex.txt       "$ITERS_FAST" 2>&1 | tee -a "$REPORT" || true
"$RUST_BIN/dkim"    corpus/dkim_realistic.txt    "$ITERS_FAST" 2>&1 | tee -a "$REPORT" || true
"$RUST_BIN/ical"    corpus/ical_simple.ics       "$ITERS_MED"  2>&1 | tee -a "$REPORT" || true
"$RUST_BIN/rfc5322" corpus/rfc5322_message.eml   "$ITERS_FAST" 2>&1 | tee -a "$REPORT" || true
"$RUST_BIN/mime"    corpus/rfc5322_message.eml   "$ITERS_FAST" 2>&1 | tee -a "$REPORT" || true

# --- C runners (best-effort) ---
echo "" >> "$REPORT"
echo "## C" >> "$REPORT"
if command -v cc >/dev/null 2>&1; then
    cd c
    if cc -O2 spf_libspf2.c -lspf2 -o spf_libspf2 2>/dev/null; then
        ./spf_libspf2 ../corpus/spf_simple.txt "$ITERS_FAST" 2>&1 | tee -a "$REPORT" || true
    else
        echo "skip: libspf2 not installed (brew install libspf2 / apt install libspf2-dev)" >> "$REPORT"
    fi
    # Augment PKG_CONFIG_PATH with Homebrew Cellar paths if present —
    # Homebrew on Apple Silicon doesn't auto-add these.
    if [ -d /opt/homebrew/Cellar ]; then
        for pc in /opt/homebrew/Cellar/libical/*/lib/pkgconfig \
                  /opt/homebrew/Cellar/icu4c*/*/lib/pkgconfig; do
            [ -d "$pc" ] && export PKG_CONFIG_PATH="${PKG_CONFIG_PATH:-}:$pc"
        done
    fi
    if pkg-config --exists libical 2>/dev/null && \
       cc -O2 ical_libical.c $(pkg-config --cflags --libs libical) -o ical_libical 2>/dev/null; then
        ./ical_libical ../corpus/ical_simple.ics "$ITERS_MED" 2>&1 | tee -a "$REPORT" || true
    else
        echo "skip: libical not installed (brew install libical / apt install libical-dev)" >> "$REPORT"
    fi
    cd ..
else
    echo "skip: no C compiler in PATH" >> "$REPORT"
fi

# --- Go runners (best-effort) ---
echo "" >> "$REPORT"
echo "## Go" >> "$REPORT"
if command -v go >/dev/null 2>&1; then
    cd go/rfc5322_netmail
    if go build -o rfc5322_netmail . 2>/dev/null; then
        ./rfc5322_netmail ../../corpus/rfc5322_message.eml "$ITERS_FAST" 2>&1 | tee -a "$REPORT" || true
    else
        echo "skip: go build failed" >> "$REPORT"
    fi
    cd ../..
else
    echo "skip: no Go toolchain in PATH" >> "$REPORT"
fi

echo '```' >> "$REPORT"
echo "" >> "$REPORT"
echo "Results appended to $REPORT"

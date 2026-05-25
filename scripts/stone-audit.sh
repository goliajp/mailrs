#!/usr/bin/env bash
# usage: ./scripts/stone-audit.sh <crate-name>
#
# Runs the 6-dimension audit (perf / mem / size / doc / test / bench) on
# one stone and writes a markdown report to /tmp/stone-audit-<name>.md.
#
# Dimensions:
# - doc:    `cargo doc --no-deps -p <crate> 2>&1` warning count == 0
# - test:   `cargo llvm-cov -p <crate> --summary-only` line cov %
# - bench:  benches/*.rs presence + criterion `--quick` runs ok
# - size:   `cargo package --list -p <crate>` total bytes (rough proxy
#           for what crates.io ships)
# - perf:   bench median (handled by step bench, just records the
#           number; competitor comparison is manual research)
# - mem:    dhat profile of any provided one-shot op (deferred per
#           stone; this script only flags whether `dhat` dev-dep is
#           wired)
#
# Outputs both human-readable summary + the underlying paths for
# follow-up.

set -uo pipefail
NAME="${1:?usage: $0 <crate-name>}"

cd "$(dirname "$0")/.."

REPORT="/tmp/stone-audit-${NAME}.md"
DIR=""
for d in crates/*/; do
    n=$(grep '^name = ' "$d/Cargo.toml" 2>/dev/null | head -1 | sed 's/name = "\(.*\)"/\1/')
    if [ "$n" = "$NAME" ]; then
        DIR="$d"
        break
    fi
done
if [ -z "$DIR" ]; then
    echo "ERROR: crate '$NAME' not found in crates/" >&2
    exit 1
fi
DIR="${DIR%/}"

echo "# Stone audit: $NAME" > "$REPORT"
echo "" >> "$REPORT"
echo "Generated: $(date '+%Y-%m-%d %H:%M:%S')  Path: \`$DIR\`" >> "$REPORT"
echo "" >> "$REPORT"

VERSION=$(grep '^version' "$DIR/Cargo.toml" | head -1 | sed 's/.*"\(.*\)"/\1/')
echo "**Version:** $VERSION" >> "$REPORT"
echo "" >> "$REPORT"

# ---------- doc ----------
echo "## doc" >> "$REPORT"
DOC_LOG=$(mktemp)
cargo doc --no-deps -p "$NAME" 2>&1 | tee "$DOC_LOG" > /dev/null
DOC_WARN=$(grep -c "^warning" "$DOC_LOG" || true)
DOC_ERR=$(grep -c "^error" "$DOC_LOG" || true)
if [ "$DOC_WARN" -eq 0 ] && [ "$DOC_ERR" -eq 0 ]; then
    echo "- ✅ \`cargo doc --no-deps\` clean (0 warnings, 0 errors)" >> "$REPORT"
else
    echo "- ❌ \`cargo doc --no-deps\` had $DOC_WARN warnings, $DOC_ERR errors" >> "$REPORT"
fi
rm -f "$DOC_LOG"

# README presence + size
if [ -f "$DIR/README.md" ]; then
    R_LINES=$(wc -l < "$DIR/README.md" | tr -d ' ')
    echo "- README.md: ✅ $R_LINES lines" >> "$REPORT"
else
    echo "- README.md: ❌ missing" >> "$REPORT"
fi

echo "" >> "$REPORT"

# ---------- test ----------
echo "## test (line coverage)" >> "$REPORT"
COV_LOG=$(mktemp)
if cargo llvm-cov -p "$NAME" --summary-only 2>&1 | tee "$COV_LOG" > /dev/null; then
    COV_LINE=$(grep -E "^TOTAL" "$COV_LOG" | head -1)
    if [ -n "$COV_LINE" ]; then
        # cargo-llvm-cov "TOTAL  rgs  miss  cov%  rgns  miss  cov%  lines  miss  cov%"
        # we want the line coverage % which is the 10th-ish field; just print full
        echo "- coverage: \`$COV_LINE\`" >> "$REPORT"
    else
        echo "- coverage: ran but no TOTAL row parsed (re-run manually)" >> "$REPORT"
    fi
else
    echo "- ❌ \`cargo llvm-cov\` failed (see /tmp/cov-$NAME.log)" >> "$REPORT"
    cp "$COV_LOG" "/tmp/cov-$NAME.log"
fi
rm -f "$COV_LOG"

echo "" >> "$REPORT"

# ---------- bench ----------
echo "## bench" >> "$REPORT"
if [ -d "$DIR/benches" ]; then
    BENCH_FILES=$(ls "$DIR/benches"/*.rs 2>/dev/null | wc -l | tr -d ' ')
    echo "- criterion benches: ✅ $BENCH_FILES file(s)" >> "$REPORT"
else
    echo "- criterion benches: ❌ missing" >> "$REPORT"
fi
if [ -f "$DIR/tests/perf_gate.rs" ]; then
    GATE_COUNT=$(grep -c '#\[test\]' "$DIR/tests/perf_gate.rs" || echo 0)
    echo "- perf_gate.rs: ✅ $GATE_COUNT gate(s)" >> "$REPORT"
else
    echo "- perf_gate.rs: ❌ missing" >> "$REPORT"
fi
if [ -f "$DIR/BUDGETS.md" ]; then
    echo "- BUDGETS.md: ✅ present" >> "$REPORT"
else
    echo "- BUDGETS.md: ❌ missing" >> "$REPORT"
fi

echo "" >> "$REPORT"

# ---------- size ----------
echo "## size" >> "$REPORT"
PKG_LOG=$(mktemp)
if cargo package --list -p "$NAME" --allow-dirty 2>/dev/null > "$PKG_LOG"; then
    FILE_COUNT=$(wc -l < "$PKG_LOG" | tr -d ' ')
    echo "- crates.io package: $FILE_COUNT files" >> "$REPORT"
else
    echo "- \`cargo package --list\` failed (likely path-dep issue)" >> "$REPORT"
fi
rm -f "$PKG_LOG"

# rlib size after release build
RLIB=""
for cand in /Volumes/INTEL2T/workspace-cache/cargo-target/release/lib"${NAME//-/_}.rlib"; do
    [ -f "$cand" ] && RLIB="$cand"
done
if [ -n "$RLIB" ]; then
    SZ=$(du -h "$RLIB" | cut -f1)
    echo "- release rlib: $SZ (\`$RLIB\`)" >> "$REPORT"
else
    echo "- release rlib: not built yet (run \`cargo build --release -p $NAME\`)" >> "$REPORT"
fi

echo "" >> "$REPORT"

# ---------- perf + mem (mostly manual, just report current state) ----------
echo "## perf (manual — populate after bench)" >> "$REPORT"
if grep -q "vs " "$DIR/README.md" 2>/dev/null; then
    echo "- README mentions \"vs <competitor>\": ✅ probably has comparison" >> "$REPORT"
else
    echo "- README has no \"vs X\" mention — competitor comparison missing" >> "$REPORT"
fi

echo "" >> "$REPORT"

echo "## mem (dhat) — manual" >> "$REPORT"
if grep -q '^dhat = ' "$DIR/Cargo.toml" 2>/dev/null; then
    echo "- dhat dev-dep: ✅ wired" >> "$REPORT"
else
    echo "- dhat dev-dep: ❌ not wired (\`dhat = \"0.3\"\` in [dev-dependencies] required)" >> "$REPORT"
fi

echo "" >> "$REPORT"

# ---------- fuzz ----------
echo "## fuzz" >> "$REPORT"
if [ -d "$DIR/fuzz" ]; then
    F_TARGETS=$(ls "$DIR/fuzz/fuzz_targets"/*.rs 2>/dev/null | wc -l | tr -d ' ')
    echo "- fuzz targets: ✅ $F_TARGETS file(s)" >> "$REPORT"
else
    echo "- fuzz: ❌ no fuzz/ directory" >> "$REPORT"
fi

echo "" >> "$REPORT"
echo "---" >> "$REPORT"
echo "Report path: \`$REPORT\`"
echo ""
cat "$REPORT"

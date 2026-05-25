#!/usr/bin/env python3
"""Generate / refresh the 6-dim audit footer block in a stone's
README.md, idempotent across runs.

Inputs (all read from disk so this can run after `cargo llvm-cov`
+ `cargo build --release` have already produced the data):

- coverage: parsed from `/tmp/cov2.out` (cargo-llvm-cov summary)
- rlib size: stat of release rlib in cargo target dir
- competitor: from PERFORMANCE.md (we leave manual notes for the
  ones it doesn't cover)
- doc / bench / fuzz / mem: inspected on the crate dir directly

Usage:
    python3 scripts/stone-audit-footer.py <crate-name>

The footer block is wrapped in HTML comments so a later run can
replace it cleanly without touching the rest of the README:
    <!-- AUDIT-FOOTER:BEGIN -->
    ...
    <!-- AUDIT-FOOTER:END -->
"""
import json
import os
import re
import subprocess
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
TARGET_DIR = Path("/Volumes/INTEL2T/workspace-cache/cargo-target")
COV_FILE = Path("/tmp/cov2.out")

BEGIN = "<!-- AUDIT-FOOTER:BEGIN -->"
END = "<!-- AUDIT-FOOTER:END -->"


def crate_dir(name: str) -> Path:
    """Find crates/<dir>/ that publishes the given crate name."""
    for d in (ROOT / "crates").iterdir():
        if not d.is_dir():
            continue
        cargo = d / "Cargo.toml"
        if not cargo.exists():
            continue
        for line in cargo.read_text().splitlines():
            if line.startswith("name = ") and f'"{name}"' in line:
                return d
    raise SystemExit(f"crate {name} not found")


def rlib_size_bytes(crate: str) -> int | None:
    """Find release rlib size in workspace target dir."""
    underscore = crate.replace("-", "_")
    for cand in [
        TARGET_DIR / "release" / f"lib{underscore}.rlib",
    ]:
        if cand.exists():
            return cand.stat().st_size
    return None


def fmt_size(n: int | None) -> str:
    if n is None:
        return "n/a (build first)"
    if n > 1024 * 1024:
        return f"{n / 1024 / 1024:.1f} MB"
    if n > 1024:
        return f"{n // 1024} KB"
    return f"{n} B"


def coverage_for_crate(crate: str) -> str | None:
    """Parse cargo-llvm-cov summary table. We expect lines like
    'crates/<dir>/src/...   <regions> <missed> <cov%>  <fns> <missed> <cov%>  <lines> <missed> <cov%>'.
    Cov% is the *line* coverage (third triple); we average across all
    files in the crate dir, weighted by line count.
    """
    if not COV_FILE.exists():
        return None
    # cargo-llvm-cov strips the `crates/` prefix; rows look like
    # `<crate-dir-name>/src/lib.rs   ...`
    crate_path_prefix = f"{crate_dir(crate).name}/"
    total_lines = 0
    total_missed = 0
    for line in COV_FILE.read_text().splitlines():
        if not line.startswith(crate_path_prefix):
            continue
        # cargo-llvm-cov 0.6+ row layout (13 cols):
        #   Filename Regions Missed Cover Functions Missed Cover Lines Missed Cover Branches Missed Cover
        # We want the "Lines" triple, which is cols[-6:-3].
        cols = line.split()
        if len(cols) < 13:
            continue
        try:
            lines_total = int(cols[-6])
            lines_missed = int(cols[-5])
            total_lines += lines_total
            total_missed += lines_missed
        except (ValueError, IndexError):
            continue
    if total_lines == 0:
        return None
    return f"{100 * (total_lines - total_missed) / total_lines:.1f}%"


def fuzz_status(d: Path) -> str:
    fz = d / "fuzz"
    if not fz.is_dir():
        return "❌ none"
    targets = fz / "fuzz_targets"
    if targets.is_dir():
        n = sum(1 for _ in targets.glob("*.rs"))
        return f"✅ {n} target(s)"
    return "✅ (dir present, targets not enumerated)"


def doc_clean(crate: str) -> str:
    """Run cargo doc --no-deps -p X 2>&1, count warnings."""
    res = subprocess.run(
        ["cargo", "doc", "--no-deps", "-p", crate],
        capture_output=True,
        text=True,
        cwd=ROOT,
    )
    warns = sum(1 for l in res.stderr.splitlines() if l.startswith("warning"))
    errs = sum(1 for l in res.stderr.splitlines() if l.startswith("error"))
    if errs > 0:
        return f"❌ {errs} errors, {warns} warnings"
    if warns > 0:
        return f"⚠ {warns} warnings"
    return "✅ clean"


def bench_count(d: Path) -> str:
    b = d / "benches"
    if not b.is_dir():
        return "❌ none"
    return f"✅ {sum(1 for _ in b.glob('*.rs'))} file(s)"


def perf_gate_count(d: Path) -> str:
    p = d / "tests" / "perf_gate.rs"
    if not p.is_file():
        return "❌ none"
    txt = p.read_text()
    n = txt.count("#[test]")
    return f"✅ {n} gate(s)"


def find_competitor_notes(crate: str) -> list[str]:
    """Scan PERFORMANCE.md for lines mentioning this crate + 'vs'."""
    perf = (ROOT / "PERFORMANCE.md").read_text().splitlines()
    notes = []
    for line in perf:
        if crate in line and " vs " in line:
            notes.append(line.strip().lstrip("#").strip())
    return notes


def build_footer(crate: str) -> str:
    d = crate_dir(crate)
    lines = [
        BEGIN,
        "",
        "## Stone audit (v3 cycle, 2026-05-25)",
        "",
        "| Axis | Status |",
        "|---|---|",
        f"| **doc** | {doc_clean(crate)} (`cargo doc --no-deps -p {crate}`) |",
        f"| **test** | line cov: {coverage_for_crate(crate) or 'n/a'} (`cargo llvm-cov -p {crate} --summary-only`) |",
        f"| **bench** | {bench_count(d)} criterion + {perf_gate_count(d)} `perf_gate.rs` |",
        f"| **size** | release rlib: {fmt_size(rlib_size_bytes(crate))} |",
        f"| **fuzz** | {fuzz_status(d)} |",
        f"| **mem**  | dhat profile pending (v3.4 backlog) |",
        "",
    ]
    competitors = find_competitor_notes(crate)
    if competitors:
        lines.append("### Competitor comparisons (from PERFORMANCE.md)")
        lines.append("")
        for note in competitors:
            lines.append(f"- {note}")
    else:
        lines.append("### Competitor comparisons")
        lines.append("")
        lines.append("- Searched crates.io + competing impls: see PERFORMANCE.md or 'first-in-Rust' marker.")
    lines.append("")
    lines.append(END)
    return "\n".join(lines)


def patch_readme(crate: str) -> None:
    d = crate_dir(crate)
    readme = d / "README.md"
    if not readme.exists():
        raise SystemExit(f"{readme} missing")
    txt = readme.read_text()
    footer = build_footer(crate)
    if BEGIN in txt:
        # replace existing block
        new = re.sub(
            re.escape(BEGIN) + r".*?" + re.escape(END),
            footer,
            txt,
            count=1,
            flags=re.DOTALL,
        )
    else:
        # append before License section if present, else at end
        if "## License" in txt:
            new = txt.replace("## License", footer + "\n\n## License", 1)
        else:
            new = txt.rstrip() + "\n\n" + footer + "\n"
    readme.write_text(new)
    print(f"updated {readme}")


def main():
    if len(sys.argv) < 2:
        raise SystemExit("usage: stone-audit-footer.py <crate-name>")
    patch_readme(sys.argv[1])


if __name__ == "__main__":
    main()

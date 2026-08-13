#!/usr/bin/env python3
"""Put the arms side by side, once they have all run.

  scripts/bench-api-e2e.sh --compare          # reads bench-results/

Reads every `<arm>.ndjson` written by bench-api-e2e.sh and prints one table
per measure with a column per arm.

**It fails if the arms disagree about the dataset.** Each run records a
fingerprint of its first conversation page — (thread_id, subject, message
count) in order, hashed. Two arms with different fingerprints were serving
different data, or the same data in a different order, and a latency
comparison between them is a comparison of two workloads. That check comes
first and nothing is printed past it.

Order is part of the fingerprint on purpose: identical membership in a
different order is what a paginated reader turns into rows the user sees
twice or not at all, and it is invisible to a set comparison.

Differences are annotated against the first column, and only when they
exceed the pooled spread of the two cells. A change smaller than the
measurement's own deviation is noise; printing it as a percentage invites
someone to act on it.
"""

import json
import statistics
import sys
from pathlib import Path

ARM_ORDER = ["pg", "spg", "fastcore"]


def pct(sorted_ms, p):
    if not sorted_ms:
        return float("nan")
    idx = max(1, min(len(sorted_ms), int(len(sorted_ms) * p)))
    return sorted_ms[idx - 1]


def load(path: Path):
    lat, res, disk, eng, meta = {}, {}, [], [], {}
    for line in path.read_text().splitlines():
        line = line.strip()
        if not line.startswith("{"):
            continue
        rec = json.loads(line)
        k = rec.get("kind")
        if k == "latency":
            lat.setdefault(rec["endpoint"], {})[rec["round"]] = [float(x) for x in rec["ms"]]
        elif k == "resource":
            slot = res.setdefault(rec["proc"], {"rss": [], "cpu": []})
            slot["rss"].append(float(rec["rss_kb"]))
            slot["cpu"].append(float(rec["cpu_pct"]))
        elif k == "disk":
            disk.append((rec["label"], int(rec["kb"])))
        elif k == "engine":
            eng.append((rec["label"], str(rec["value"])))
        elif k == "meta":
            meta = rec
    return {"lat": lat, "res": res, "disk": disk, "eng": eng, "meta": meta}


def stat_cell(per_round, p):
    """Median across rounds of that round's percentile, and the spread."""
    vals = [pct(sorted(ms), p) for _, ms in sorted(per_round.items())]
    if not vals:
        return None, None
    sd = statistics.stdev(vals) if len(vals) > 1 else 0.0
    return statistics.median(vals), sd


def render(value, sd, base=None, base_sd=None):
    if value is None:
        return "—"
    cell = f"{value:.1f} ±{sd:.1f}"
    if base is None or base == 0 or value == base:
        return cell
    # Pooled spread of the two cells. Below it, the delta is not separable
    # from run-to-run variation and gets no percentage.
    pooled = (sd or 0) + (base_sd or 0)
    if abs(value - base) <= pooled:
        return f"{cell} (noise)"
    return f"{cell} ({(value - base) / base * 100:+.0f}%)"


def main() -> int:
    out_dir = Path(sys.argv[1] if len(sys.argv) > 1 else "bench-results")
    if not out_dir.is_dir():
        print(f"no results directory: {out_dir}", file=sys.stderr)
        return 1

    runs = {}
    for path in sorted(out_dir.glob("*.ndjson")):
        runs[path.stem] = load(path)
    if not runs:
        print(f"no runs in {out_dir} — run an arm first", file=sys.stderr)
        return 1

    arms = [a for a in ARM_ORDER if a in runs] + [a for a in runs if a not in ARM_ORDER]

    # ── the gate ────────────────────────────────────────────────────────
    prints = {a: runs[a]["meta"].get("fingerprint", "?") for a in arms}
    hosts = {a: runs[a]["meta"].get("host", "?") for a in arms}
    print()
    print("dataset fingerprint (first conversation page, order included)")
    for a in arms:
        print(f"  {a:<10} {prints[a]}   host={hosts[a]}  commit={runs[a]['meta'].get('commit', '?')}")

    distinct = set(prints.values())
    if len(distinct) > 1:
        print()
        print("!! THE ARMS ARE NOT HOLDING THE SAME DATASET")
        print("Comparing latency across these would compare two workloads, not two")
        print("backends. Re-seed from one bench-api-seed.py run and re-measure.")
        return 1

    if len(set(hosts.values())) > 1:
        print()
        print("!! arms measured on different hosts — the numbers are not comparable")
        return 1

    # ── latency ─────────────────────────────────────────────────────────
    endpoints = []
    for a in arms:
        for ep in runs[a]["lat"]:
            if ep not in endpoints:
                endpoints.append(ep)

    for label, p in (("p50", 0.50), ("p95", 0.95), ("p99", 0.99)):
        print()
        head = f"{label + ' ms':<30}" + "".join(f"{a:>22}" for a in arms)
        print(head)
        print("-" * len(head))
        for ep in endpoints:
            row = f"{ep:<30}"
            base = base_sd = None
            for i, a in enumerate(arms):
                per_round = runs[a]["lat"].get(ep)
                if not per_round:
                    row += f"{'—':>22}"
                    continue
                v, sd = stat_cell(per_round, p)
                if i == 0:
                    base, base_sd = v, sd
                    row += f"{render(v, sd):>22}"
                else:
                    row += f"{render(v, sd, base, base_sd):>22}"
            print(row)

    # ── footprint ───────────────────────────────────────────────────────
    print()
    head = f"{'footprint':<30}" + "".join(f"{a:>22}" for a in arms)
    print(head)
    print("-" * len(head))

    row = f"{'RSS MB peak, all procs':<30}"
    peaks = {}
    for a in arms:
        tot = sum(max(s["rss"]) / 1024 for s in runs[a]["res"].values() if s["rss"])
        peaks[a] = tot
        row += f"{tot:>22.1f}" if tot else f"{'—':>22}"
    print(row)

    row = f"{'CPU % peak, summed':<30}"
    for a in arms:
        tot = sum(max(s["cpu"]) for s in runs[a]["res"].values() if s["cpu"])
        row += f"{tot:>22.1f}" if tot else f"{'—':>22}"
    print(row)

    # Count in the column, names underneath: an arm's process count is the
    # thing that makes the summed rows above the comparable ones, and the
    # names are too long to sit in a numeric column without breaking it.
    row = f"{'processes':<30}"
    for a in arms:
        row += f"{len(runs[a]['res']):>22}"
    print(row)

    print()
    for a in arms:
        names = ", ".join(sorted(n.removeprefix("mailrs-") for n in runs[a]["res"])) or "—"
        print(f"  {a:<10} {names}")

    print()
    for a in arms:
        for label, kb in runs[a]["disk"]:
            print(f"  {a:<10} {label:<28} {kb / 1024:>10.1f} MB")
        for label, value in runs[a]["eng"]:
            print(f"  {a:<10} {label:<28} {value:>10}")

    missing = [a for a in ARM_ORDER if a not in runs]
    if missing:
        print()
        print(f"arms not yet measured: {', '.join(missing)}")
    print()
    return 0


if __name__ == "__main__":
    sys.exit(main())

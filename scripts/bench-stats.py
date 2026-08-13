#!/usr/bin/env python3
"""Reduce raw benchmark samples to a reportable panel.

Reads NDJSON on stdin, one record per line:

  {"kind":"latency","endpoint":"conversations","round":1,"ms":[12.1,11.8,...]}
  {"kind":"resource","proc":"fastcore","rss_kb":181240,"cpu_pct":4.2}
  {"kind":"disk","label":"kevy dir","kb":412880}
  {"kind":"engine","label":"keys","value":"48613"}
  {"kind":"meta","arm":"fastcore","rounds":5,"n":30,"host":"lx64"}

Prints a panel: per endpoint the median across rounds of that round's p50 /
p95 / p99, each with the sample standard deviation across rounds.

Why the median of per-round percentiles rather than one percentile over the
pooled requests: pooling hides round-to-round variance, and round-to-round
variance is the only thing that says whether a difference between two arms
is real. A number without its spread cannot be compared to another number.

Exit 1 if fewer than 3 rounds are present for any endpoint — a gap smaller
than the spread is not reportable, and with two rounds there is no spread.
"""

import json
import statistics
import sys


def pct(sorted_ms: list[float], p: float) -> float:
    """Nearest-rank percentile, the same rule the old inline awk used."""
    if not sorted_ms:
        return float("nan")
    idx = int(len(sorted_ms) * p)
    if idx < 1:
        idx = 1
    if idx > len(sorted_ms):
        idx = len(sorted_ms)
    return sorted_ms[idx - 1]


def fmt(value: float, spread: float | None) -> str:
    if value != value:  # NaN
        return "—"
    if spread is None:
        return f"{value:.1f}"
    return f"{value:.1f} ±{spread:.1f}"


def main() -> int:
    latency: dict[str, dict[int, list[float]]] = {}
    order: list[str] = []
    resource: dict[str, dict[str, list[float]]] = {}
    disk: list[tuple[str, int]] = []
    engine: list[tuple[str, str]] = []
    meta: dict = {}

    for line in sys.stdin:
        line = line.strip()
        if not line or not line.startswith("{"):
            continue
        rec = json.loads(line)
        kind = rec.get("kind")
        if kind == "latency":
            ep = rec["endpoint"]
            if ep not in latency:
                latency[ep] = {}
                order.append(ep)
            latency[ep][rec["round"]] = [float(x) for x in rec["ms"]]
        elif kind == "resource":
            p = rec["proc"]
            slot = resource.setdefault(p, {"rss_kb": [], "cpu_pct": []})
            slot["rss_kb"].append(float(rec["rss_kb"]))
            slot["cpu_pct"].append(float(rec["cpu_pct"]))
        elif kind == "disk":
            disk.append((rec["label"], int(rec["kb"])))
        elif kind == "engine":
            engine.append((rec["label"], str(rec["value"])))
        elif kind == "meta":
            meta = rec

    arm = meta.get("arm", "?")
    rounds = meta.get("rounds", "?")
    n = meta.get("n", "?")

    print()
    print(f"arm={arm}  rounds={rounds}  requests/endpoint/round={n}  host={meta.get('host', '?')}")
    print()

    thin = [ep for ep, rs in latency.items() if len(rs) < 3]

    # The witness has to have fired. It is sampled once a second for the whole
    # run, so zero samples means the sampler died or never started — and a
    # missing witness reads exactly like a quiet machine. One column was
    # recorded as "pinned, load observed" off a run that was neither, because
    # nothing here objected to its absence.
    no_witness = "host loadavg" not in resource

    head = f"{'endpoint':<30} {'p50 ms':>16} {'p95 ms':>16} {'p99 ms':>16}"
    print(head)
    print("-" * len(head))
    for ep in order:
        per_round = latency[ep]
        p50s, p95s, p99s = [], [], []
        for _, ms in sorted(per_round.items()):
            s = sorted(ms)
            p50s.append(pct(s, 0.50))
            p95s.append(pct(s, 0.95))
            p99s.append(pct(s, 0.99))

        def cell(vals: list[float]) -> str:
            med = statistics.median(vals)
            # Sample stdev needs two points; with one round there is no
            # spread to report and saying "±0.0" would claim there is.
            sd = statistics.stdev(vals) if len(vals) > 1 else None
            return fmt(med, sd)

        print(f"{ep:<30} {cell(p50s):>16} {cell(p95s):>16} {cell(p99s):>16}")

    # The host-load witness is not a process and must not be read as one: in
    # the process table it renders as 0.0 MB resident, which invites a reader
    # to skip the row. It gets its own line.
    witness = resource.pop("host loadavg", None)

    if resource:
        print()
        rhead = f"{'process':<30} {'RSS MB peak':>16} {'RSS MB mean':>16} {'CPU % peak':>16}"
        print(rhead)
        print("-" * len(rhead))
        total_peak = 0.0
        for p, slot in resource.items():
            rss = slot["rss_kb"]
            cpu = slot["cpu_pct"]
            if not rss:
                continue
            peak = max(rss) / 1024
            total_peak += peak
            print(
                f"{p:<30} {peak:>16.1f} {statistics.mean(rss) / 1024:>16.1f} "
                f"{max(cpu):>16.1f}"
            )
        if len(resource) > 1:
            # The arms are not one process each: the kevy arm runs a core and
            # a web tier, the monolith arms run one. Comparing a single
            # process against a pair understates the pair, so the sum is the
            # comparable figure and gets its own line.
            print(f"{'— all processes, summed':<30} {total_peak:>16.1f}")

    if witness and witness["cpu_pct"]:
        la = witness["cpu_pct"]
        print()
        print(
            f"{'host load average':<30} {min(la):>16.2f} {statistics.mean(la):>16.2f} "
            f"{max(la):>16.2f}   (min / mean / max over {len(la)} samples)"
        )
        print(
            "  A witness with nothing to do with the store: a percentile that moved"
        )
        print("  while this moved is not a finding.")

    if disk or engine:
        print()
        for label, kb in disk:
            print(f"{label:<30} {kb / 1024:>16.1f} MB")
        for label, value in engine:
            print(f"{label:<30} {value:>16}")

    if no_witness:
        print()
        print("NOT REPORTABLE — no host-load samples in this run.")
        print("The witness is sampled every second; none means it never ran, and")
        print("an absent witness is indistinguishable from a quiet machine.")
        return 1

    if thin:
        print()
        print(
            "NOT REPORTABLE — fewer than 3 rounds for: " + ", ".join(sorted(thin))
        )
        print("A difference smaller than its own spread is noise, and one round has no spread.")
        return 1

    print()
    return 0


if __name__ == "__main__":
    sys.exit(main())

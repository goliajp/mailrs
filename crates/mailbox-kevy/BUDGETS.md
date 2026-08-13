# mailrs-mailbox-kevy performance budgets

Latency budgets enforced by `tests/perf_gate.rs`. Run:

```bash
cargo test --release -p mailrs-mailbox-kevy --test perf_gate -- --nocapture
```

`--nocapture` prints the whole panel, which is the point: the gate reports
every measurement, not only the ones that failed.

Detailed tracking lives in `benches/store_ops.rs` (`cargo bench -p
mailrs-mailbox-kevy`). Neither existed before 2026-08-13 — this is the store
production runs on, and it had no benchmark and no gate, which is why the
last kevy upgrade shipped with no per-operation numbers at all.

## Why this crate got them now

Because the kevy 4.1.1 → 5.1 comparison needs a before column, and there was
nothing to take one with. `PERFORMANCE.md`'s account of the previous upgrade
says so outright: *"Not benched separately: throughput on individual embedded
ops."*

## Path taxonomy

`list_threads_by_activity` and the badge counts are **warm** per
`rules/rust/patterns.md` — one call per conversation-list load. `deliver_message`
is warm too, once per arriving mail. `backfill_thread_user` is **cold**: a
timer runs it, and what matters there is that its stable state is cheap, not
that any one pass is fast.

Budgets sit at roughly 5× the observed median, in line with the rest of this
workspace: these catch order-of-magnitude regressions, and per
`PERFORMANCE.md:394` they are **not publishable performance numbers**. The
comparison figures live in `PERFORMANCE.md`'s three-column panel with their
spread and their reproduce command.

## The fixture

23,508 threads for one user, the same count `scripts/bench-api-seed.py`
builds, plus one thread owned by a second account so every read has something
it must exclude. One in ten starred, one in five unread, one in twenty-five
flagged for action.

Two things about it are load-bearing:

**Per-user flags go through the per-user mutators**, not through `ThreadRow`.
`upsert_thread` deliberately plants those flags at zero on a row it has just
created — starred belongs to an owner, and the shared aggregate's copy is not
authoritative for a new membership row. The first version of this fixture set
them on the row, so every flag index was empty, and the flag-keyed page
"benchmarked" at **551 ns** because it was returning nothing. Every read now
asserts a full page before it is timed.

**`deliver_message` uses a fresh thread per iteration.** Re-using one id grew
that thread without bound during the run, so the number drifted upward while
it was being measured — 816 µs then, 43 µs now.

## Budgets

| Path | Budget | Observed median (dev) | Headroom | Notes |
| --- | ---: | ---: | ---: | --- |
| `list/all/page1` | 800 µs | 165 µs | ~5× | Default page, 50 rows hydrated. |
| `list/bucket_inbox/page1` | 600 µs | 113 µs | ~5× | The `by_user_bucket` ORDERPATH. |
| `list/category/page1` | 600 µs | 89 µs | ~7× | The `by_user_category` ORDERPATH. |
| `list/starred_flag/page1` | 6 ms | 1.23 ms | ~5× | Flag-keyed. **11× the bucket page** — see below. |
| `list/all/page200` | 5 ms | 991 µs | ~5× | Deep paging. 6× page one, which is what says whether the declared path seeks or walks. |
| `count/unread_badge` | 16 ms | 2.95 ms | ~5× | Every conversation-list load. See below. |
| `count/starred` | 7 ms | 1.40 ms | ~5× | |
| `get_thread_for_user` | 10 µs | 1.21 µs | ~8× | One membership row. |
| `backfill/converged/200` | 26 ms | 6.24 ms | ~4× | The sweep with nothing to do. Also asserts it wrote nothing. |
| `upsert_thread` | 100 µs | 18.0 µs | ~5× | One atomic: thread hash + membership row. |
| `set_thread_importance` | 80 µs | 15.1 µs | ~5× | |
| `allocate_uid/fresh` | 15 µs | 3.88 µs | ~4× | Budget from the gate, not from criterion — see open questions. |
| `allocate_uid/repeat` | 2 µs | 125 ns | ~16× | The dedupe path, and it asserts the same uid comes back. |
| `deliver_message` | 400 µs | 42.9 µs | ~9× | Thread hash, membership row, per-user message row, uid index, blob and text index, in one call. |

Measured 2026-08-13 on an M-series Mac, `--release`, median of 100 iterations,
all gates in one test so nothing measures anything else.

## Two of these gate a property, not a speed

**`backfill/converged/200`** asserts the sweep's `written` counter is zero on
a second pass over an unchanged page. `periodic-work-must-converge` asks for a
stable state that is *cheap*, not merely idempotent, and only a write counter
can falsify that — overwriting a value with the one already there is
idempotent and is not convergent, which cost a core hours on 2026-07-19 while
logging `sent_added=255 created=0` every 31 seconds.

The sweep passes. It is not free, though: 6.24 ms per 200 rows is ~31 µs a row
of read-compare, paid on every pass, so a full sweep of 23,508 threads is
around three quarters of a second of work to discover there is nothing to do.
Convergent in writes, linear in reads.

**`allocate_uid/repeat`** asserts the second call for a message-id returns the
same uid. A duration budget alone would pass a repeat that redid the work.

## Open questions, recorded rather than chased

**The badge count is 2.95 ms.** It runs on every conversation-list load, over
4,701 unread rows — around 630 ns a row, which is a walk, not an index count.
This is the measured target for `idx_count_claused`, new in kevy 5.1 and on
the harvest list. Recorded here so the after-number has a before-number.

**The flag-keyed page costs 11× the bucket page** (1.23 ms against 113 µs) for
the same 50 rows. Both are declared paths. Not investigated — this round is a
comparison, not a perf attack, and the rule about that is explicit.

**`allocate_uid/fresh` disagrees between instruments**: 3.88 µs here, 163 ns
under criterion, same call and same build. Every other row agrees to within a
few percent, so one of the two is measuring something else, and it has not
been run down. The budget follows the gate, because the gate is what runs in
CI. Do not quote the 163 ns.

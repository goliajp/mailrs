# Performance budgets

`tests/perf_gate.rs`, release only. Message: the 54,614-byte newsletter
built in the test (text and HTML alternatives, a small zip). Each gate
runs three rounds of 400 calls and takes the median of the rounds.

| Gate | Measured (highest run) | Budget |
|---|---|---|
| Receiver scan (attachments + raw Bayes tokens) through the worker, P95 | 543 µs | 1,700 µs |
| Its overhead per call over the same parse in process, median | 69 µs | 210 µs |
| Drain search text (html2text) through the worker, P95 | 212 µs | 650 µs |
| Its overhead per call over the same parse in process, median | 54 µs | 170 µs |

Budgets are 3× the measured figure, rounded up. The two P95 rows are the
highest of eight runs on studio (Apple M4 Max, 2026-11-05, load1 6–11).
The overhead rows are the highest of sixteen runs on studio
(2026-11-06, load1 16–25).

The overhead is the worker call minus the in-process parse timed right
after it in the same iteration: two pipe copies of the message, the JSON
answer and two context switches. It used to be the difference of the two
P95s, which put the preemption tails of two separate loops into one
number: it read 40–280 µs at load1 12 and 130–1,230 µs with every core
busy, while the per-call median read 30–70 µs and 60–280 µs on the same
runs.

The worker P95 rows still carry how long a busy machine takes to wake
the worker and then the caller. On studio at load1 around 20 the scan's
worker P95 sometimes read 1.8–4.0 ms for a whole run while its in-process
P95 and per-call overhead stayed normal. Usual in-process P95 on studio:
about 160 µs (search text) and 500 µs (scan); a failing run that prints
an in-process P95 far above these, or a normal one next to a worker P95
several times its budget, was measured on a busy machine.

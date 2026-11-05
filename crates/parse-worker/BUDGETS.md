# Performance budgets

`tests/perf_gate.rs`, release only. Message: the 54,614-byte newsletter
built in the test (text and HTML alternatives, a small zip). Each budget is
3× the highest P95 of eight release runs on studio (Apple M-series,
2026-11-05, load1 6–11), rounded up.

| Gate | Measured P95 (highest of 8) | Budget |
|---|---|---|
| Receiver scan (attachments + raw Bayes tokens) through the worker | 543 µs | 1,700 µs |
| Its overhead over the same parse in process | 53 µs | 160 µs |
| Drain search text (html2text) through the worker | 212 µs | 650 µs |
| Its overhead over the same parse in process | 48 µs | 150 µs |

The overhead is the difference of the two P95s: two pipe copies of the
message, the JSON answer and two context switches.

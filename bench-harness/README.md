# Cross-language bench harness

Compares mailrs Rust crates against C and Go reference implementations
on identical input corpora. Lives outside `Cargo.toml` because the C/Go
toolchains aren't part of the Rust workspace.

## Layout

```
bench-harness/
├── corpus/         Shared test inputs (text files, one fixture per scenario)
├── c/              C runners — one binary per competitor (libspf2, opendkim, libical, GMime…)
├── go/             Go runners — one binary per competitor
├── scripts/        Driver scripts — run-all.sh, run-one.sh
└── results/        Captured timings (markdown tables, JSON)
```

## Method (honest disclosure)

Every runner reads the same `corpus/*.txt` fixture, runs N iterations of
the equivalent operation, prints `<lang>/<lib>/<scenario>: <ns/op>` to
stdout. Wall-clock measurement on the same machine, same load, same
release flags (`-O2` for C, `-O3 -flto` where applicable; Rust uses the
workspace's performance-first release profile).

This is intentionally **simpler** than criterion's statistical machinery
— sub-µs measurements wouldn't be honest cross-process anyway because
the per-runner harness overhead is constant but non-zero. The goal here
is "**is mailrs in the same league as the C reference implementation?**",
not "is mailrs 3% faster than libfoo".

Results published in `results/REPORT.md` with the iteration counts and
build commands so anyone can reproduce.

## Status

| Scenario | Rust | C | Go | Status |
|---|---|---|---|---|
| SPF record parse | mailrs-spf 1.0.4 | libspf2 (TBD) | go-spf (TBD) | scaffolded |
| DKIM-Signature parse | mailrs-dkim 1.1.3 | opendkim (TBD) | go-dkim (TBD) | scaffolded |
| iCalendar parse | mailrs-ical 1.0.3 | libical (TBD) | gocal (TBD) | scaffolded |
| RFC 5322 message scan | mailrs-rfc5322 1.0.1 | n/a (Postfix internal) | `net/mail` | scaffolded |
| MIME tree parse | mailrs-mime 1.0.1 | GMime (TBD) | net/mail (limited) | scaffolded |

`(TBD)` = harness binary defined but C/Go competitor build not yet wired —
those need each library's `pkg-config` / `go.mod` and a small wrapper.
The runner files spell out what's needed.

## Running

From repo root:

```bash
./bench-harness/scripts/run-all.sh        # everything that's wired up
./bench-harness/scripts/run-one.sh spf    # one scenario
```

Results land in `results/REPORT.md` as a timestamped section.

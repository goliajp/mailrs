# Cross-language bench harness — 2026-05-23T07:10:59Z

Run from Darwin 25.5.0 arm64
```
   Compiling mailrs-dkim v1.1.3 (/Users/doracawl/workspace/goliajp/mailrs/crates/dkim)
   Compiling mailrs-cross-runner v0.0.0 (/Users/doracawl/workspace/goliajp/mailrs/bench-harness/rust-runner)
    Finished `release` profile [optimized] target(s) in 4.13s
## Rust
rust/mailrs-spf/parse: 65.0 ns/op (1000000 iters)
rust/mailrs-spf/parse: 400.7 ns/op (1000000 iters)
rust/mailrs-dkim/parse: 430.8 ns/op (1000000 iters)
rust/mailrs-ical/parse: 1760.5 ns/op (100000 iters)
rust/mailrs-rfc5322/parse+subject+from: 45.8 ns/op (1000000 iters)
rust/mailrs-mime/parse: 600.8 ns/op (1000000 iters)

## C
skip: libspf2 not installed (brew install libspf2 / apt install libspf2-dev)
c/libical/parse: 7032.0 ns/op (100000 iters)

## Go
go/net-mail/read-message: 1440.5 ns/op (1000000 iters)
```


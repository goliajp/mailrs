# Cross-language bench harness — 2026-05-23T06:51:23Z

Run from Darwin 25.5.0 arm64
```
    Finished `release` profile [optimized] target(s) in 0.06s
## Rust
rust/mailrs-spf/parse: 69.1 ns/op (1000000 iters)
rust/mailrs-spf/parse: 446.0 ns/op (1000000 iters)
rust/mailrs-dkim/parse: 482.2 ns/op (1000000 iters)
rust/mailrs-ical/parse: 1847.7 ns/op (100000 iters)
rust/mailrs-rfc5322/parse+subject+from: 52.9 ns/op (1000000 iters)
rust/mailrs-mime/parse: 669.7 ns/op (1000000 iters)

## C
skip: libspf2 not installed (brew install libspf2 / apt install libspf2-dev)
skip: libical not installed (brew install libical / apt install libical-dev)

## Go
go/net-mail/read-message: 1266.5 ns/op (1000000 iters)
```


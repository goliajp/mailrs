# mailrs-clamav

[![Crates.io](https://img.shields.io/crates/v/mailrs-clamav?style=flat-square&logo=rust)](https://crates.io/crates/mailrs-clamav)
[![docs.rs](https://img.shields.io/docsrs/mailrs-clamav?style=flat-square&logo=docs.rs)](https://docs.rs/mailrs-clamav)
[![License](https://img.shields.io/crates/l/mailrs-clamav?style=flat-square)](#license)

Async ClamAV (`clamd`) TCP client. Implements the `zINSTREAM` malware
scanning protocol plus `PING` / `VERSION` daemon commands. Tokio-based,
no external state.

## Quickstart

```rust,no_run
use mailrs_clamav::{scan, ClamavResult};

# async fn run(data: &[u8]) {
match scan("127.0.0.1:3310", data).await {
    ClamavResult::Clean => {
        // accept the message
    }
    ClamavResult::Virus(name) => {
        eprintln!("rejected: virus={name}");
        // reject 5xx — DO NOT silently drop, the sender needs the bounce
    }
    ClamavResult::Error(e) => {
        eprintln!("clamd unreachable: {e}");
        // fail-open or fail-closed is a policy choice — handle here
    }
}
# }
```

## What this crate does

- **`scan(addr, data)`** — `zINSTREAM` scan with 30s default timeout
- **`scan_with_timeout(addr, data, timeout)`** — same with custom timeout
- **`ping(addr, timeout) -> bool`** — `zPING` → `PONG` health check
- **`version(addr, timeout) -> Option<String>`** — `zVERSION` for ops
  dashboards
- **`parse_response(bytes)`** — exposed for testing / reusing the
  reply parser without going through the socket

## Wire protocol

```text
client → clamd:  zINSTREAM\0
client → clamd:  [u32 BE: chunk_len][chunk_bytes...]     (repeat)
client → clamd:  [u32 BE: 0]                              (terminator)
clamd  → client: stream: OK\0                             (clean)
clamd  → client: stream: <virus-name> FOUND\0             (detection)
clamd  → client: <error text> ERROR\0                     (error)
```

Chunks are capped at [`CHUNK_SIZE`] (2 MiB). For very large attachments,
verify your `clamd.conf` `StreamMaxLength` setting (default 25 MB).

## What this crate does not

- **No connection pool / re-use.** Each `scan` opens a fresh TCP
  connection. ClamAV is typically deployed alongside the mail server
  on localhost; the connect overhead is negligible vs. the scan itself.
  Pooling could be added in a future minor without breaking compat.
- **No Unix domain socket.** TCP only. `clamd` supports both; this
  crate covers the TCP form. If you need UDS, file an issue.
- **No `MULTISCAN` / `CONTSCAN` / `STATS` / `RELOAD`.** Only the
  three commands listed above. The full clamd command set is large
  and most are admin-only; this crate covers the per-message data
  path.
- **No SCAN-with-path.** `INSTREAM` only (the daemon never reads
  files from the filesystem; the bytes go over the socket). This is
  the safe shape for sandboxed deployments where `clamd` doesn't
  share a filesystem with the mail server.

## Performance

Measured (criterion, M-series Mac, release, 100-sample median):

| Operation | Median |
|---|---:|
| `parse_response` (clean reply) | **~9 ns** |
| `parse_response` (virus, short name) | **~60 ns** |
| `parse_response` (virus, long name) | **~78 ns** |
| `parse_response` (error reply) | **~49 ns** |
| `parse_response` (empty input) | **~21 ns** |

`scan` itself is network-bound — the bench numbers above are the CPU
pieces. A localhost `clamd` scan of a 100 KB payload typically completes
in 10-30 ms; the CPU portion of that is microseconds.

Reproduce: `cargo bench -p mailrs-clamav --bench clamav`. Workspace
[PERFORMANCE.md](../../PERFORMANCE.md) carries the same table.

## License

Apache-2.0 OR MIT.

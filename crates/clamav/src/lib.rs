#![doc = include_str!("../README.md")]
#![deny(missing_docs)]
#![deny(rustdoc::broken_intra_doc_links)]

use std::time::Duration;

use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;

/// Max chunk size per ClamAV INSTREAM frame.
///
/// ClamAV's `StreamMaxLength` default is 25 MB; INSTREAM splits the
/// payload into length-prefixed chunks. 2 MiB per chunk balances syscall
/// overhead against memory use.
pub const CHUNK_SIZE: usize = 2 * 1024 * 1024;

/// Default per-call timeout. Applied to connect + write + read together.
/// ClamAV scans of typical 100 KB mails finish in <100 ms; large
/// attachments (5-10 MB) can take seconds. Pick something that won't
/// stall your inbound pipeline.
pub const DEFAULT_TIMEOUT: Duration = Duration::from_secs(30);

/// Outcome of a ClamAV scan.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ClamavResult {
    /// The payload was scanned and no signature matched.
    Clean,
    /// A signature matched. The string is the virus name as reported by
    /// `clamd` (e.g. `"Eicar-Test-Signature"`).
    Virus(String),
    /// The scan didn't complete. The string is a human-readable
    /// description (TCP connect failure, protocol error, unparseable
    /// reply, etc).
    Error(String),
}

/// Scan `data` against a `clamd` daemon listening at `addr` (e.g.
/// `"127.0.0.1:3310"`) using the **zINSTREAM** protocol variant.
///
/// `zINSTREAM` is the null-terminated form of INSTREAM (the `z` prefix
/// means "command terminator is `\0`"). It's the most widely supported
/// form across `clamd` versions.
///
/// The default timeout (see [`DEFAULT_TIMEOUT`]) applies. Use
/// [`scan_with_timeout`] to override.
///
/// Returns:
/// - [`ClamavResult::Clean`] if the payload was scanned and is clean
/// - [`ClamavResult::Virus`] with the virus name on detection
/// - [`ClamavResult::Error`] on any I/O / protocol / timeout failure
///   (never panics)
pub async fn scan(addr: &str, data: &[u8]) -> ClamavResult {
    scan_with_timeout(addr, data, DEFAULT_TIMEOUT).await
}

/// Like [`scan`] but with a caller-supplied timeout.
pub async fn scan_with_timeout(addr: &str, data: &[u8], timeout: Duration) -> ClamavResult {
    match tokio::time::timeout(timeout, scan_inner(addr, data)).await {
        Ok(r) => r,
        Err(_) => ClamavResult::Error(format!("timeout after {timeout:?}")),
    }
}

async fn scan_inner(addr: &str, data: &[u8]) -> ClamavResult {
    let mut stream = match TcpStream::connect(addr).await {
        Ok(s) => s,
        Err(e) => return ClamavResult::Error(format!("connect failed: {e}")),
    };

    // zINSTREAM = INSTREAM with NUL terminator instead of newline.
    if stream.write_all(b"zINSTREAM\0").await.is_err() {
        return ClamavResult::Error("write command failed".into());
    }

    // Length-prefixed chunks. ClamAV refuses chunks above StreamMaxLength
    // (typically 25 MB), so we cap each frame at CHUNK_SIZE.
    for chunk in data.chunks(CHUNK_SIZE) {
        let len = (chunk.len() as u32).to_be_bytes();
        if stream.write_all(&len).await.is_err() || stream.write_all(chunk).await.is_err() {
            return ClamavResult::Error("write data failed".into());
        }
    }

    // Zero-length chunk terminates the INSTREAM frame.
    if stream.write_all(&0u32.to_be_bytes()).await.is_err() {
        return ClamavResult::Error("write terminator failed".into());
    }

    // Replies fit in 1 KB easily — `stream: Eicar-Test-Signature FOUND\n`
    // is ~40 bytes; even verbose error strings are well under 200.
    let mut response = vec![0u8; 1024];
    match stream.read(&mut response).await {
        Ok(n) => parse_response(&response[..n]),
        Err(e) => ClamavResult::Error(format!("read failed: {e}")),
    }
}

/// Parse a `clamd` INSTREAM reply.
///
/// Expected wire shapes:
/// - `"stream: OK"` → Clean
/// - `"stream: Eicar-Test-Signature FOUND"` → Virus("Eicar-Test-Signature")
/// - anything else → Error (includes `clamd` error strings like
///   `"INSTREAM size limit exceeded. ERROR"`)
pub fn parse_response(response: &[u8]) -> ClamavResult {
    let s = String::from_utf8_lossy(response);
    let s = s.trim_end_matches('\0').trim();

    if s.ends_with("OK") {
        ClamavResult::Clean
    } else if let Some(found_pos) = s.find("FOUND") {
        // Wire form: "stream: VirusName FOUND" — virus name lives
        // between the last ':' and the trailing " FOUND".
        let virus = s[..found_pos]
            .trim()
            .rsplit(':')
            .next()
            .unwrap_or("")
            .trim();
        ClamavResult::Virus(virus.to_string())
    } else {
        ClamavResult::Error(s.to_string())
    }
}

/// PING the daemon. Returns `true` if `clamd` is up and replies `PONG`
/// within the timeout.
pub async fn ping(addr: &str, timeout: Duration) -> bool {
    let Ok(Ok(mut stream)) = tokio::time::timeout(timeout, TcpStream::connect(addr)).await else {
        return false;
    };
    if tokio::time::timeout(timeout, stream.write_all(b"zPING\0"))
        .await
        .is_err()
        || stream.flush().await.is_err()
    {
        return false;
    }
    let mut buf = [0u8; 16];
    let Ok(Ok(n)) = tokio::time::timeout(timeout, stream.read(&mut buf)).await else {
        return false;
    };
    let reply = String::from_utf8_lossy(&buf[..n]);
    reply.trim_end_matches('\0').trim() == "PONG"
}

/// Ask `clamd` for its version string. Useful for ops dashboards.
///
/// Returns `None` if the daemon doesn't respond within `timeout` or
/// reports a malformed reply.
pub async fn version(addr: &str, timeout: Duration) -> Option<String> {
    let Ok(Ok(mut stream)) = tokio::time::timeout(timeout, TcpStream::connect(addr)).await else {
        return None;
    };
    if tokio::time::timeout(timeout, stream.write_all(b"zVERSION\0"))
        .await
        .is_err()
    {
        return None;
    }
    let mut buf = [0u8; 256];
    let Ok(Ok(n)) = tokio::time::timeout(timeout, stream.read(&mut buf)).await else {
        return None;
    };
    let v = String::from_utf8_lossy(&buf[..n]);
    let v = v.trim_end_matches('\0').trim();
    if v.is_empty() {
        None
    } else {
        Some(v.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_clean_reply() {
        assert_eq!(parse_response(b"stream: OK\n"), ClamavResult::Clean);
        assert_eq!(parse_response(b"stream: OK"), ClamavResult::Clean);
        // Trailing NUL (zINSTREAM)
        assert_eq!(parse_response(b"stream: OK\0"), ClamavResult::Clean);
    }

    #[test]
    fn parse_virus_reply_eicar() {
        assert_eq!(
            parse_response(b"stream: Eicar-Test-Signature FOUND\n"),
            ClamavResult::Virus("Eicar-Test-Signature".into())
        );
    }

    #[test]
    fn parse_virus_reply_with_extended_name() {
        // ClamAV signatures can have dots, dashes, slashes
        assert_eq!(
            parse_response(b"stream: Trojan.Win32.Generic-7654321 FOUND\n"),
            ClamavResult::Virus("Trojan.Win32.Generic-7654321".into())
        );
    }

    #[test]
    fn parse_virus_reply_no_stream_prefix() {
        // Some daemons / proxies strip the prefix.
        assert_eq!(
            parse_response(b"Eicar FOUND"),
            ClamavResult::Virus("Eicar".into())
        );
    }

    #[test]
    fn parse_error_reply_size_limit() {
        // Real clamd reply when payload exceeds StreamMaxLength
        let r = parse_response(b"INSTREAM size limit exceeded. ERROR");
        assert!(matches!(r, ClamavResult::Error(_)));
        if let ClamavResult::Error(e) = r {
            assert!(e.contains("size limit"));
        }
    }

    #[test]
    fn parse_empty_reply_is_error() {
        let r = parse_response(b"");
        assert!(matches!(r, ClamavResult::Error(_)));
    }

    #[test]
    fn parse_whitespace_only_reply_is_error() {
        // Empty after trim — still parsed as error (no OK / FOUND)
        let r = parse_response(b"   \n\t  ");
        assert!(matches!(r, ClamavResult::Error(_)));
    }

    #[test]
    fn parse_strips_trailing_nuls() {
        // zINSTREAM wire form has NUL terminator
        let r = parse_response(b"stream: OK\0\0\0");
        assert_eq!(r, ClamavResult::Clean);
    }

    #[test]
    fn parse_invalid_utf8_lossy_still_works() {
        let bad: &[u8] = &[0xFF, 0xFE, b's', b't', b'r', b'e', b'a', b'm', b':', b' ', b'O', b'K'];
        let r = parse_response(bad);
        // From-utf8-lossy replaces invalid bytes; "OK" still matches.
        assert_eq!(r, ClamavResult::Clean);
    }

    #[test]
    fn clamav_result_equality_for_same_virus_name() {
        let a = ClamavResult::Virus("X".into());
        let b = ClamavResult::Virus("X".into());
        assert_eq!(a, b);
    }

    #[test]
    fn clamav_result_inequality_for_different_virus_names() {
        assert_ne!(
            ClamavResult::Virus("A".into()),
            ClamavResult::Virus("B".into())
        );
    }

    #[test]
    fn scan_unreachable_addr_returns_error_not_panic() {
        // Use a port that's almost certainly not bound.
        let rt = tokio::runtime::Runtime::new().unwrap();
        let r = rt.block_on(scan_with_timeout(
            "127.0.0.1:1",
            b"payload",
            Duration::from_millis(200),
        ));
        assert!(matches!(r, ClamavResult::Error(_)));
    }

    #[test]
    fn scan_with_zero_timeout_returns_timeout_error() {
        let rt = tokio::runtime::Runtime::new().unwrap();
        let r = rt.block_on(scan_with_timeout(
            "127.0.0.1:1",
            b"payload",
            Duration::from_micros(1),
        ));
        assert!(matches!(r, ClamavResult::Error(_)));
        if let ClamavResult::Error(e) = r {
            // Either timeout or connect-failure depending on race
            assert!(e.contains("timeout") || e.contains("connect"));
        }
    }

    #[test]
    fn ping_returns_false_when_unreachable() {
        let rt = tokio::runtime::Runtime::new().unwrap();
        let r = rt.block_on(ping("127.0.0.1:1", Duration::from_millis(200)));
        assert!(!r);
    }

    #[test]
    fn version_returns_none_when_unreachable() {
        let rt = tokio::runtime::Runtime::new().unwrap();
        let r = rt.block_on(version("127.0.0.1:1", Duration::from_millis(200)));
        assert!(r.is_none());
    }
}

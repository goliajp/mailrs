//! A process abort on a marked message, for the release smoke of the
//! spool's quarantine (`scripts/spool-abort-smoke.sh`). Compiled only with
//! the `abort-hook` feature, which no shipped binary is built with.
//!
//! With `MAILRS_TEST_ABORT=1`, delivering a message whose header block
//! carries `X-Mailrs-Test-Abort: yes` aborts the process after the maildir
//! copy is written and before it is indexed: where a parser crash used to
//! land, and the window the spool's attempt record has to cover.

const MARKER: &[u8] = b"X-Mailrs-Test-Abort: yes";

pub(crate) fn before_ingest(body: &[u8]) {
    if std::env::var_os("MAILRS_TEST_ABORT").is_none_or(|v| v != "1") {
        return;
    }
    let head_end = body
        .windows(4)
        .position(|w| w == b"\r\n\r\n")
        .unwrap_or(body.len());
    if body[..head_end].windows(MARKER.len()).any(|w| w == MARKER) {
        std::process::abort();
    }
}

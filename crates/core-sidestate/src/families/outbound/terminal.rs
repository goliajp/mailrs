//! The terminal state of a job on the job hash, written by the sender.

use super::*;

/// Terminal transition on the job hash: `state` is `delivered`,
/// `bounced` or `failed` (no retry), kept 24 h for inspection.
pub fn dual_write_terminal(conn: &mut kevy_client::Connection, id: i64, state: &[u8]) {
    let now_str = now_secs().to_string();
    let job_k = job_key(id);
    let _ = conn.pipeline(|p| {
        p.cmd(&[
            b"HSET",
            job_k.as_bytes(),
            b"state",
            state,
            b"updated_at",
            now_str.as_bytes(),
        ]);
        p.cmd(&[b"HDEL", job_k.as_bytes(), b"claimed_at"]);
        // 24 h retention on the terminal-state hash — enough for
        // post-mortem inspection without ballooning AOF (per RFC §9).
        p.cmd(&[b"EXPIRE", job_k.as_bytes(), b"86400"]);
    });
}

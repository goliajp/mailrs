//! The writing end of the bounce hand-off queue: a message for a local
//! sender, delivered into their INBOX by the core that drains the queue.

use base64::Engine as _;

/// Pending-queue key in the network kevy.
pub const BOUNCE_PENDING: &[u8] = b"mailrs:bounce:pending";

pub fn entry_key(id: &str) -> String {
    format!("mailrs:bounce:{id}")
}

/// Queue `message` for delivery into local `recipient`'s INBOX. Unknown
/// recipients are dropped by the drain, so nothing queued here leaves.
pub fn enqueue_notice(
    conn: &mut kevy_client::Connection,
    recipient: &str,
    message: &[u8],
) -> std::io::Result<()> {
    let mut nonce = [0u8; 6];
    rand_core::RngCore::fill_bytes(&mut rand_core::OsRng, &mut nonce);
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let id = format!(
        "{now}-{}",
        nonce.iter().map(|b| format!("{b:02x}")).collect::<String>()
    );
    let recipient = recipient.trim().trim_matches(['<', '>']);
    let b64 = base64::engine::general_purpose::STANDARD.encode(message);
    conn.hset(
        entry_key(&id).as_bytes(),
        &[
            (b"recipient".as_slice(), recipient.as_bytes()),
            (b"blob", b64.as_bytes()),
        ],
    )?;
    conn.lpush(BOUNCE_PENDING, &[id.as_bytes()])?;
    Ok(())
}

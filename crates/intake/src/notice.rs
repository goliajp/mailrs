//! The bounce hand-off queue: a message for a local sender — a DSN from the
//! sender process, a simulated reply — delivered into their INBOX by
//! whichever core runs.

use base64::Engine as _;

use crate::drain::Core;

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

/// Deliver the queue for as long as the process runs.
///
/// On a blocking thread, not a runtime worker: the pop blocks, and a worker
/// parked in it is lost to the runtime and keeps the process from shutting
/// down.
pub fn spawn_drain<C>(core: std::sync::Arc<C>, url: String) -> tokio::task::JoinHandle<()>
where
    C: Core + Send + Sync + 'static,
{
    tokio::task::spawn_blocking(move || {
        loop {
            drain_one(&*core, &url);
        }
    })
}

/// Deliver what is queued, waiting up to ten seconds for the first entry.
/// Unknown recipients (no account, no maildir) are dropped with a warn — a
/// bounce must never itself bounce.
pub fn drain_one<C: Core + ?Sized>(core: &C, url: &str) {
    let Ok(mut conn) = kevy_client::Connection::connect(url) else {
        return;
    };
    // First pop blocks up to 10 s (matches prior wake-up cadence); after
    // that first hit we drain remaining items with non-blocking RPOPs to
    // avoid re-arming the blocking timer per item on a bursty queue.
    let mut first = true;
    loop {
        let popped_bytes: Option<Vec<u8>> = if first {
            first = false;
            match conn.brpop(&[BOUNCE_PENDING], Some(std::time::Duration::from_secs(10))) {
                Ok(Some((_key, value))) => Some(value),
                Ok(None) | Err(_) => None,
            }
        } else {
            conn.rpop(BOUNCE_PENDING, 1)
                .unwrap_or_default()
                .into_iter()
                .next()
        };
        let Some(id_bytes) = popped_bytes else {
            return;
        };
        let Ok(id) = String::from_utf8(id_bytes) else {
            continue;
        };
        let key = entry_key(&id);
        let recipient = conn
            .hget(key.as_bytes(), b"recipient")
            .ok()
            .flatten()
            .and_then(|b| String::from_utf8(b).ok())
            .unwrap_or_default();
        let blob = conn
            .hget(key.as_bytes(), b"blob")
            .ok()
            .flatten()
            .and_then(|b| base64::engine::general_purpose::STANDARD.decode(b).ok());
        let _ = conn.del(&[key.as_bytes()]);
        let Some(bytes) = blob else { continue };
        // local-account guard: never forward a DSN outward
        let known = matches!(core.is_account(&recipient), Ok(true));
        let Some((local, domain)) = recipient.split_once('@') else {
            continue;
        };
        let base = std::path::PathBuf::from(core.maildir_root())
            .join(domain)
            .join(local);
        if !known && !base.join("new").is_dir() {
            tracing::warn!(%recipient, "bounce for unknown local sender dropped");
            continue;
        }
        for leaf in ["cur", "new", "tmp"] {
            let _ = std::fs::create_dir_all(base.join(leaf));
        }
        let filename = format!(
            "{}.Mdsn{}.bounce",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_secs())
                .unwrap_or(0),
            id
        );
        let target = base.join("new").join(&filename);
        if let Err(e) = std::fs::write(&target, &bytes) {
            tracing::warn!(error = %e, %recipient, "bounce maildir write failed");
            continue;
        }
        // Bounce DSNs are inbound-side notifications to the sender —
        // always route to INBOX, never Junk (v2.4.0 Phase 2).
        core.ingest(&recipient, &filename, &bytes, "INBOX", None);
        crate::live_sync::audit_system("mail.bounce", &recipient, "DSN delivered to sender");
        tracing::info!(%recipient, %id, "bounce DSN delivered");
    }
}

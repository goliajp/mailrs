//! Fetch mail from accounts somewhere else, and file it as this user's.
//!
//! The account row and the secret were added in RFC step one and
//! nothing read them — an account could be connected and no mail ever
//! appeared, which is `one-side-of-the-wire.md` waiting to happen. This
//! is the reader.
//!
//! **Fetched mail enters through `ingest_delivered_file`**, the same
//! door the spool uses. Threading, the per-user message row, the
//! declared axes, sender trust and sieve all run unchanged; nothing
//! downstream knows or needs to know that this message came from
//! somebody else's server.
//!
//! What is *not* run on it: the spam pipeline. Gmail has already
//! filtered this mailbox, and a second opinion here disagrees with the
//! verdict the person can see on the other side — mail they expect to
//! find in the inbox would move. That is a deliberate choice and it is
//! in the RFC's open questions.

use std::sync::Arc;
use std::time::Duration;

use mailrs_core_sidestate::families::external_accounts::{self as ext, AccountRow};
use mailrs_imap_client as imap;

use crate::FastcoreState;

/// How often the loop looks for due accounts when it last found one.
const BUSY_INTERVAL: Duration = Duration::from_secs(60);

/// Longest interval when nothing has been due for a while.
///
/// The same shape as `calendar_sync`: a loop with no cheap resting
/// state is what burned a shared host on 2026-07-19, and with no
/// external account connected anywhere this would otherwise enumerate
/// every account and read every key once a minute forever.
const IDLE_INTERVAL: Duration = Duration::from_secs(300);

fn now_secs() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

/// The deployment's sealing key, if one is configured.
fn sealing_key() -> Option<mailrs_secretbox::Key> {
    std::env::var("MAILRS_ACCOUNT_KEY")
        .ok()
        .filter(|v| !v.trim().is_empty())
        .map(|v| mailrs_secretbox::Key::from_passphrase(&v))
}

/// Poll connected accounts until the process ends.
pub async fn spawn(state: Arc<FastcoreState>) {
    if sealing_key().is_none() {
        // Said once, plainly. Without the key no secret can be opened,
        // so every account would fail identically on every tick — and a
        // loop that cannot succeed should not run at all rather than
        // fill the log with the same line.
        tracing::warn!(
            "external accounts: MAILRS_ACCOUNT_KEY is not set, so no connected \
             mailbox can be opened — sync not started"
        );
        return;
    }
    tracing::info!("external account sync started");
    let mut idle_rounds = 0u32;
    loop {
        let synced = tick(&state).await;
        idle_rounds = match synced {
            0 => idle_rounds.saturating_add(1),
            _ => 0,
        };
        tokio::time::sleep(crate::idle_backoff::idle_backoff(
            BUSY_INTERVAL,
            IDLE_INTERVAL,
            idle_rounds,
        ))
        .await;
    }
}

/// One pass. Returns how many accounts were actually fetched from,
/// which is what tells the loop whether the round accomplished
/// anything — counted only when something was done, per
/// `periodic-work-must-converge.md`.
async fn tick(state: &Arc<FastcoreState>) -> usize {
    let users = match state.mailbox.list_account_addresses() {
        Ok(u) => u,
        Err(e) => {
            tracing::warn!(err = %e, "external sync: cannot list accounts");
            return 0;
        }
    };
    let now = now_secs();
    let mut worked = 0usize;
    for user in users {
        for row in due_accounts(state, &user, now) {
            worked += 1;
            let outcome = sync_one(state, &user, &row).await;
            let updated = match &outcome {
                Ok(n) => {
                    if *n > 0 {
                        tracing::info!(%user, account = %row.email, fetched = n, "external sync");
                    }
                    ext::with_success(row.clone(), now)
                }
                Err(why) => {
                    tracing::warn!(%user, account = %row.email, %why, "external sync failed");
                    ext::with_failure(row.clone(), now, why)
                }
            };
            save(state, &user, &updated);
        }
    }
    worked
}

/// The accounts of one user that are due now.
fn due_accounts(state: &Arc<FastcoreState>, user: &str, now: i64) -> Vec<AccountRow> {
    let Some(mut conn) = state.net_conn() else {
        return Vec::new();
    };
    let key = format!("ext:accts:{user}");
    let flat = conn.hgetall(key.as_bytes()).unwrap_or_default();
    let mut out = Vec::new();
    let mut i = 0;
    while i + 1 < flat.len() {
        if let Ok(row) = serde_json::from_slice::<AccountRow>(&flat[i + 1])
            && ext::is_due(&row, now)
        {
            out.push(row);
        }
        i += 2;
    }
    out
}

fn save(state: &Arc<FastcoreState>, user: &str, row: &AccountRow) {
    let Some(mut conn) = state.net_conn() else {
        return;
    };
    let key = format!("ext:accts:{user}");
    if let Ok(json) = serde_json::to_string(row) {
        let _ = conn.hset(key.as_bytes(), &[(row.id.as_bytes(), json.as_bytes())]);
    }
}

/// Fetch one account. `Ok(n)` is how many new messages were filed.
///
/// Returns the failure as a string because that is what goes on the row
/// and onto the screen where the account was added — the person who
/// connected it is the only one who can fix a rotated password.
async fn sync_one(
    state: &Arc<FastcoreState>,
    user: &str,
    row: &AccountRow,
) -> Result<usize, String> {
    if row.incoming.protocol != "imap" {
        return Err(format!(
            "{} is not a protocol this can read yet",
            row.incoming.protocol
        ));
    }
    let secret = open_secret(state, user, &row.id)?;
    let tls = match row.incoming.tls {
        ext::Tls::Implicit => imap::Tls::Implicit,
        ext::Tls::StartTls => imap::Tls::StartTls,
        ext::Tls::None => imap::Tls::None,
    };
    let mut session = imap::Session::connect(&row.incoming.host, row.incoming.port, tls)
        .await
        .map_err(|e| e.to_string())?;

    let login_name = row.username.clone().unwrap_or_else(|| row.email.clone());
    match row.auth {
        ext::AuthKind::OAuth2 => session
            .authenticate_xoauth2(&login_name, &secret)
            .await
            .map_err(|e| e.to_string())?,
        _ => session
            .login(&login_name, &secret)
            .await
            .map_err(|e| e.to_string())?,
    }

    let mut filed = 0usize;
    for folder in folders_to_read(&mut session).await? {
        filed += read_folder(state, user, row, &mut session, &folder).await?;
    }
    Ok(filed)
}

/// Which folders are worth reading.
///
/// Two are skipped for the same reason a person would skip them: a
/// provider's own view holding a copy of everything doubles the
/// download, and a folder that cannot be opened produces one error per
/// sync forever.
async fn folders_to_read(session: &mut imap::Session) -> Result<Vec<String>, String> {
    let all = session.list().await.map_err(|e| e.to_string())?;
    Ok(all
        .into_iter()
        .filter(worth_reading)
        .map(|f| f.name)
        .collect())
}

/// Whether a folder is worth downloading.
///
/// Three exclusions, each for a concrete cost:
///
/// - `\Noselect` cannot be opened at all, so trying produces one error
///   per folder per sync, forever.
/// - `\All` is the provider's own view holding a copy of every
///   message — Gmail's `[Gmail]/All Mail`. Reading it downloads the
///   whole mailbox a second time and files every message twice.
/// - `\Trash` and `\Junk` are what the person already threw away.
///   Bringing them back is the opposite of what they did.
fn worth_reading(f: &imap::List) -> bool {
    !f.selectable_is_false && !f.is_all && !f.is_trash && !f.is_junk
}

/// Read one folder, file what is new, and remember where we got to.
async fn read_folder(
    state: &Arc<FastcoreState>,
    user: &str,
    row: &AccountRow,
    session: &mut imap::Session,
    folder: &str,
) -> Result<usize, String> {
    let mut state_of = session.select(folder).await.map_err(|e| e.to_string())?;
    let (remembered_validity, highest) = read_marker(state, &row.id, folder);
    state_of.remembered_uidvalidity = remembered_validity;

    let Some(plan) = imap::plan_fetch(&state_of, highest) else {
        return Ok(0);
    };
    if let imap::FetchPlan::Everything { because } = &plan {
        // Said out loud: a full re-download is expensive and looks like
        // a bug when it is not, so the reason goes in the log rather
        // than being inferred from the volume.
        tracing::info!(%user, account = %row.email, %folder, %because, "reading the whole folder");
    }

    let fetched = session
        .fetch_full(&plan.range())
        .await
        .map_err(|e| e.to_string())?;
    let mut top = highest.unwrap_or(0);
    let mut filed = 0usize;
    for (uid, meta, body) in fetched {
        // The same door the spool uses: threading, the per-user message
        // row, the declared axes, sender trust and sieve all run
        // unchanged from here.
        let blob = format!("ext-{}-{}-{}", row.id, sanitise(folder), uid);
        crate::ingest::ingest_delivered_file(state, user, &blob, &body, "INBOX");
        let _ = meta;
        top = top.max(uid);
        filed += 1;
    }
    write_marker(state, &row.id, folder, state_of.uidvalidity, top);
    Ok(filed)
}

/// A folder name that is safe in a blob reference.
fn sanitise(folder: &str) -> String {
    folder
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '_' })
        .collect()
}

fn marker_key(account_id: &str, folder: &str) -> String {
    format!("ext:sync:{account_id}:{}", sanitise(folder))
}

/// What we remember of this folder: the validity, and the highest uid.
///
/// Returned together and never apart. A uid without the validity it was
/// issued under is a number that means nothing, and separating them is
/// how a client comes to carry on from a uid the server has reused.
fn read_marker(
    state: &Arc<FastcoreState>,
    account_id: &str,
    folder: &str,
) -> (Option<u32>, Option<u32>) {
    let Some(mut conn) = state.net_conn() else {
        return (None, None);
    };
    let key = marker_key(account_id, folder);
    let flat = conn.hgetall(key.as_bytes()).unwrap_or_default();
    let mut validity = None;
    let mut highest = None;
    let mut i = 0;
    while i + 1 < flat.len() {
        let v = String::from_utf8_lossy(&flat[i + 1]).parse::<u32>().ok();
        match flat[i].as_slice() {
            b"uidvalidity" => validity = v,
            b"highest_uid" => highest = v,
            _ => {}
        }
        i += 2;
    }
    (validity, highest)
}

fn write_marker(
    state: &Arc<FastcoreState>,
    account_id: &str,
    folder: &str,
    validity: Option<u32>,
    highest: u32,
) {
    let Some(mut conn) = state.net_conn() else {
        return;
    };
    let key = marker_key(account_id, folder);
    let (v, h, at) = (
        validity.unwrap_or(0).to_string(),
        highest.to_string(),
        now_secs().to_string(),
    );
    let _ = conn.hset(
        key.as_bytes(),
        &[
            (b"uidvalidity".as_slice(), v.as_bytes()),
            (b"highest_uid".as_slice(), h.as_bytes()),
            (b"at".as_slice(), at.as_bytes()),
        ],
    );
}

/// The account's secret, opened.
fn open_secret(state: &Arc<FastcoreState>, user: &str, id: &str) -> Result<String, String> {
    let key = sealing_key().ok_or("MAILRS_ACCOUNT_KEY is not set on this server")?;
    let mut conn = state
        .net_conn()
        .ok_or("the side-state store is unreachable")?;
    let sealed = conn
        .get(format!("ext:secret:{user}:{id}").as_bytes())
        .map_err(|e| e.to_string())?
        .ok_or("no stored password for this account")?;
    let sealed = String::from_utf8(sealed).map_err(|_| "the stored password is not text")?;
    let opened = mailrs_secretbox::open(&key, &sealed).map_err(|e| e.to_string())?;
    String::from_utf8(opened).map_err(|_| "the stored password is not text".to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn folder(name: &str) -> imap::List {
        imap::List {
            name: name.into(),
            ..imap::List::default()
        }
    }

    #[test]
    fn an_ordinary_folder_is_read() {
        assert!(worth_reading(&folder("INBOX")));
        assert!(worth_reading(&folder("Work/2026")));
    }

    /// The one that doubles the download. Gmail's All Mail holds a copy
    /// of every message in the account.
    #[test]
    fn a_view_holding_everything_is_not_a_folder_to_read() {
        let mut f = folder("[Gmail]/All Mail");
        f.is_all = true;
        assert!(!worth_reading(&f));
    }

    #[test]
    fn a_folder_that_cannot_be_opened_is_not_tried() {
        let mut f = folder("[Gmail]");
        f.selectable_is_false = true;
        assert!(!worth_reading(&f));
    }

    /// What somebody threw away stays thrown away.
    #[test]
    fn the_bin_is_left_alone() {
        for set in [
            |f: &mut imap::List| f.is_trash = true,
            |f: &mut imap::List| f.is_junk = true,
        ] {
            let mut f = folder("Trash");
            set(&mut f);
            assert!(!worth_reading(&f));
        }
    }

    /// A blob reference is built from the folder name, so a name with a
    /// slash or a space in it must not produce a path or a broken key.
    #[test]
    fn a_folder_name_becomes_safe_for_a_key() {
        assert_eq!(sanitise("[Gmail]/Sent Mail"), "_Gmail__Sent_Mail");
        assert_eq!(sanitise("../../etc/passwd"), "______etc_passwd");
        assert_eq!(sanitise("受信箱"), "___");
    }

    /// Read and write must agree on where the marker lives, or every
    /// sync starts from nothing and files the mailbox again.
    #[test]
    fn the_marker_key_is_the_same_on_both_sides() {
        assert_eq!(
            marker_key("acc_1", "[Gmail]/Sent Mail"),
            "ext:sync:acc_1:_Gmail__Sent_Mail"
        );
    }
}

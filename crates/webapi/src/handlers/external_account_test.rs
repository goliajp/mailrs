//! Proving an account works before it is stored.
//!
//! Split from `external_accounts.rs` at the file-size gate, along the
//! seam that was already there: this speaks two protocols and knows
//! nothing about rows, and that file stores rows and speaks none.

use axum::http::StatusCode;
use mailrs_core_sidestate::families::external_accounts as ext;

/// How long a set-up form waits for a server to answer.
///
/// A form that hangs is worse than one that fails: somebody who waits
/// thirty seconds assumes the app is broken and reloads, and an
/// account written halfway through that is a worse state than no
/// account at all.
const CONNECT_TEST_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(10);

/// Prove the account can actually be read, before anything is stored.
///
/// The RFC has promised this since it was written and nothing did it:
/// a password typed with a character missing was saved as a working
/// account, and the person found out minutes later from a row that
/// said "Not syncing" rather than "that password was wrong".
///
/// **Which step failed is the whole value.** Answering "could not
/// connect" for a wrong password sends somebody to check their
/// hostname; these four failures have four different next steps.
pub(crate) async fn prove_it_works(
    row: &ext::AccountRow,
    secret: &str,
) -> Result<(), (StatusCode, String)> {
    // An OAuth account has just been through the provider's own sign-in.
    // There is nothing left to prove, and asking would spend a token.
    if row.auth == ext::AuthKind::OAuth2 {
        return Ok(());
    }
    let bad = |m: String| (StatusCode::BAD_REQUEST, m);
    match row.incoming.protocol.as_str() {
        "imap" => {
            let tls = match row.incoming.tls {
                ext::Tls::Implicit => mailrs_imap_client::Tls::Implicit,
                ext::Tls::StartTls => mailrs_imap_client::Tls::StartTls,
                ext::Tls::None => mailrs_imap_client::Tls::None,
            };
            let attempt = async {
                let mut s = mailrs_imap_client::Session::connect(
                    &row.incoming.host,
                    row.incoming.port,
                    tls,
                )
                .await
                .map_err(|e| format!("could not reach {}: {e}", row.incoming.host))?;
                let user = row.username.clone().unwrap_or_else(|| row.email.clone());
                s.login(&user, secret).await.map_err(|e| {
                    format!("{} did not accept that password: {e}", row.incoming.host)
                })?;
                // Authenticated, but a mailbox that cannot be listed
                // syncs nothing — and that is a different message from
                // a wrong password.
                s.list()
                    .await
                    .map_err(|e| format!("signed in, but the mailbox could not be read: {e}"))?;
                Ok::<(), String>(())
            };
            tokio::time::timeout(CONNECT_TEST_TIMEOUT, attempt)
                .await
                .map_err(|_| {
                    bad(format!(
                        "{} did not answer in ten seconds",
                        row.incoming.host
                    ))
                })?
                .map_err(bad)
        }
        "pop3" => {
            let tls = match row.incoming.tls {
                ext::Tls::Implicit => mailrs_pop3_client::Tls::Implicit,
                ext::Tls::StartTls => mailrs_pop3_client::Tls::StartTls,
                ext::Tls::None => mailrs_pop3_client::Tls::None,
            };
            let attempt = async {
                let mut s = mailrs_pop3_client::Session::connect(
                    &row.incoming.host,
                    row.incoming.port,
                    tls,
                )
                .await
                .map_err(|e| format!("could not reach {}: {e}", row.incoming.host))?;
                let user = row.username.clone().unwrap_or_else(|| row.email.clone());
                s.login(&user, secret).await.map_err(|e| {
                    format!("{} did not accept that password: {e}", row.incoming.host)
                })?;
                // The one POP3 refusal that is permanent, and the one
                // worth refusing the account over: without UIDL its
                // mail cannot be told apart between syncs, so every
                // sync would download the mailbox again.
                s.uidl().await.map_err(|e| e.to_string())?;
                Ok::<(), String>(())
            };
            tokio::time::timeout(CONNECT_TEST_TIMEOUT, attempt)
                .await
                .map_err(|_| {
                    bad(format!(
                        "{} did not answer in ten seconds",
                        row.incoming.host
                    ))
                })?
                .map_err(bad)
        }
        // JMAP's own session fetch is the test, and it needs a token
        // rather than a password — which is the OAuth path above.
        other => Err(bad(format!("{other} accounts cannot be tested yet"))),
    }
}

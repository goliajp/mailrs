//! Top-level POP3 connection driver: read line → dispatch into
//! `Pop3Session` → write response.

use std::sync::Arc;

use tokio::net::TcpStream;

use mailrs_mailbox::PgMailboxStore;

use crate::domain_store::DomainStore;
use crate::inbound::auth_guard::AuthGuard;
use crate::users::UserStore;

use super::Pop3Session;


/// Drive a single POP3 connection from accept to close. Wires
/// the session state machine (auth → transaction → quit) to the
/// raw TCP stream: greeting → read-line → handle → write
/// response, repeated until session signals close or the socket
/// dies.
#[allow(clippy::too_many_arguments)]
#[tracing::instrument(
    name = "pop3.conn",
    skip(stream, mailbox_store, users, auth_guard, domain_store, ldap_config, maildir_root),
    fields(peer = %addr),
)]
pub async fn handle_connection(
    stream: TcpStream,
    addr: std::net::SocketAddr,
    mailbox_store: Arc<PgMailboxStore>,
    users: Arc<UserStore>,
    auth_guard: Arc<AuthGuard>,
    domain_store: Option<Arc<DomainStore>>,
    ldap_config: Option<Arc<crate::ldap_auth::LdapConfig>>,
    maildir_root: &str,
) {
    use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};

    let (reader, mut writer) = stream.into_split();
    let mut reader = BufReader::new(reader);

    let mut session = Pop3Session::new(mailbox_store, users)
        .with_maildir_root(maildir_root)
        .with_auth_guard(auth_guard, addr.ip());
    if let Some(ds) = domain_store {
        session = session.with_domain_store(ds);
    }
    if let Some(ldap) = ldap_config {
        session = session.with_ldap_config(ldap);
    }

    let greeting = session.greeting();
    if writer.write_all(greeting.as_bytes()).await.is_err() {
        return;
    }

    let mut line = String::new();
    loop {
        line.clear();
        match reader.read_line(&mut line).await {
            Ok(0) | Err(_) => break,
            Ok(_) => {}
        }

        let responses = session.handle_line(&line).await;
        let should_close = session.should_close(&responses);

        for resp in &responses {
            if writer.write_all(resp.as_bytes()).await.is_err() {
                return;
            }
        }
        if writer.flush().await.is_err() {
            return;
        }

        if should_close {
            break;
        }
    }
}


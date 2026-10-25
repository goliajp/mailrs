//! Every IMAP, POP3 and ManageSieve sign-in goes through the shared login
//! guard: a locked attempt is refused before the password is looked at.

use std::net::{IpAddr, Ipv4Addr};
use std::sync::Arc;

use mailrs_auth_guard::login::{LoginGuard, Verdict};

use crate::backend::{Directory, Error, Protocol, Result};

tokio::task_local! {
    /// The client of the connection this task serves; set by the accept
    /// loop for the life of the connection.
    static PEER: IpAddr;
}

/// Run `f` as the task serving a connection from `peer`.
pub(crate) async fn serving<F: std::future::Future>(peer: IpAddr, f: F) -> F::Output {
    PEER.scope(peer.to_canonical(), f).await
}

/// The client of the current connection; unspecified outside one (a
/// session driven directly, as tests do).
pub fn peer() -> IpAddr {
    PEER.try_with(|p| *p)
        .unwrap_or(IpAddr::V4(Ipv4Addr::UNSPECIFIED))
}

/// A [`Directory`] whose sign-ins the login guard counts and can refuse.
pub struct Guarded<D> {
    dir: Arc<D>,
    guard: Arc<dyn LoginGuard>,
}

impl<D> Guarded<D> {
    pub fn new(dir: Arc<D>, guard: Arc<dyn LoginGuard>) -> Self {
        Self { dir, guard }
    }
}

fn locked_text(protocol: Protocol, verdict: Verdict) -> String {
    let minutes = verdict.retry_after_minutes();
    let text = format!("Too many failed sign-in attempts, try again in {minutes} minutes");
    match protocol {
        Protocol::Imap => format!("[UNAVAILABLE] {text}"),
        Protocol::Pop3 | Protocol::ManageSieve => text,
    }
}

impl<D: Directory> Directory for Guarded<D> {
    type Account = D::Account;

    fn hostname(&self) -> String {
        self.dir.hostname()
    }

    async fn login(
        &self,
        user: &str,
        password: &str,
        protocol: Protocol,
    ) -> Result<Option<Self::Account>> {
        let source = peer();
        let verdict = self.guard.check(source, user).await;
        if let Verdict::Locked { .. } = verdict {
            tracing::warn!(event = "auth", channel = ?protocol, %source, account = user, result = "locked");
            return Err(Error::Refused(locked_text(protocol, verdict)));
        }
        let signed_in = self.dir.login(user, password, protocol).await;
        match &signed_in {
            Ok(Some(_)) => self.guard.succeeded(source, user).await,
            Ok(None) => {
                tracing::warn!(event = "auth", channel = ?protocol, %source, account = user, result = "bad-password");
                self.guard.failed(source, user).await
            }
            Err(_) => {}
        }
        signed_in
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn locked_replies_name_the_wait() {
        let v = Verdict::Locked {
            retry_after_secs: 1800,
        };
        assert_eq!(
            locked_text(Protocol::Imap, v),
            "[UNAVAILABLE] Too many failed sign-in attempts, try again in 30 minutes"
        );
        assert_eq!(
            locked_text(Protocol::Pop3, v),
            "Too many failed sign-in attempts, try again in 30 minutes"
        );
    }

    /// Five wrong IMAP passwords lock the account for the source on every
    /// protocol: the right password is then refused on IMAP and on POP3.
    #[tokio::test]
    async fn failures_on_one_protocol_lock_all_of_them() {
        use crate::test_support::{Client, PASSWORD, USER, directory, listener};
        let guard: Arc<dyn LoginGuard> =
            Arc::new(mailrs_auth_guard::login::MemoryLoginGuard::default());
        let dir = Arc::new(Guarded::new(directory(), guard));
        let (imap, imap_addr) = listener().await;
        tokio::spawn(crate::imap::serve_plain(imap, dir.clone(), None));
        let (pop3, pop3_addr) = listener().await;
        tokio::spawn(crate::pop3::serve_plain(pop3, dir, None));

        let mut c = Client::connect(imap_addr).await;
        c.line().await;
        for i in 0..5 {
            c.send(&format!("b{i} LOGIN \"{USER}\" \"wrong\"\r\n"))
                .await;
            let r = c.line().await;
            assert!(
                r.starts_with(&format!("b{i} NO invalid credentials")),
                "{r}"
            );
        }
        let mut c = Client::connect(imap_addr).await;
        c.line().await;
        c.send(&format!("c1 LOGIN \"{USER}\" \"{PASSWORD}\"\r\n"))
            .await;
        let r = c.line().await;
        assert!(
            r.starts_with("c1 NO [UNAVAILABLE] Too many failed sign-in attempts"),
            "{r}"
        );

        let mut p = Client::connect(pop3_addr).await;
        p.line().await;
        p.send(&format!("USER {USER}\r\n")).await;
        p.line().await;
        p.send(&format!("PASS {PASSWORD}\r\n")).await;
        let r = p.line().await;
        assert!(
            r.starts_with("-ERR [SYS/TEMP] Too many failed sign-in attempts"),
            "{r}"
        );
    }

    #[tokio::test]
    async fn the_peer_is_the_connections_client() {
        assert_eq!(peer(), IpAddr::V4(Ipv4Addr::UNSPECIFIED));
        let mapped: IpAddr = "::ffff:192.0.2.7".parse().unwrap();
        let seen = serving(mapped, async { peer() }).await;
        assert_eq!(seen, "192.0.2.7".parse::<IpAddr>().unwrap());
    }
}

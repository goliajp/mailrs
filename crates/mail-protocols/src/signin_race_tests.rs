//! A connection whose credentials were checked just before its account's
//! sign-ins ended, and which registered just after, is closed all the
//! same: whether the end was a published sign-out or a sweep for cleared
//! accounts. The directory is held inside the credential check while the
//! end happens, so the order is the same on every run.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use mailrs_auth_guard::signins::ENDED;
use tokio::net::TcpStream;
use tokio::sync::Notify;

use crate::Guarded;
use crate::backend::{Directory, Protocol, Result};
use crate::test_support::{Client, Memory, MemoryAccount, PASSWORD, USER, directory, listener};

/// [`Memory`], whose password check waits to be let go.
struct Held {
    inner: Arc<Memory>,
    asked: Notify,
    answer: Notify,
    cleared: AtomicBool,
}

impl Directory for Held {
    type Account = MemoryAccount;

    fn hostname(&self) -> String {
        self.inner.hostname()
    }

    async fn login(
        &self,
        user: &str,
        password: &str,
        protocol: Protocol,
    ) -> Result<Option<MemoryAccount>> {
        self.asked.notify_one();
        self.answer.notified().await;
        self.inner.login(user, password, protocol).await
    }

    async fn open(&self, user: &str, protocol: Protocol) -> Result<Option<MemoryAccount>> {
        self.inner.open(user, protocol).await
    }

    async fn password_cleared(&self, _: &str) -> bool {
        self.cleared.load(Ordering::SeqCst)
    }
}

fn held() -> (Arc<Held>, Arc<Guarded<Held>>) {
    let held = Arc::new(Held {
        inner: directory(),
        asked: Notify::new(),
        answer: Notify::new(),
        cleared: AtomicBool::new(false),
    });
    let guard = Arc::new(mailrs_auth_guard::login::MemoryLoginGuard::default());
    (held.clone(), Arc::new(Guarded::new(held, guard)))
}

async fn next_line(c: &mut Client<TcpStream>) -> String {
    tokio::time::timeout(Duration::from_secs(5), c.line())
        .await
        .unwrap_or_else(|_| "<nothing within 5s>".into())
}

#[tokio::test]
async fn imap_a_sweep_between_the_check_and_the_registration_still_closes_it() {
    let (held, dir) = held();
    let (l, addr) = listener().await;
    tokio::spawn(crate::imap::serve_plain(l, dir.clone(), None));
    let mut c = Client::connect(addr).await;
    c.line().await;
    c.send(&format!("a1 LOGIN \"{USER}\" \"{PASSWORD}\"\r\n"))
        .await;

    held.asked.notified().await;
    held.cleared.store(true, Ordering::SeqCst);
    crate::end_cleared(&dir).await;
    held.answer.notify_one();

    let r = c.line().await;
    assert!(r.starts_with("a1 OK"), "signed in: {r}");
    assert_eq!(
        next_line(&mut c).await.trim_end(),
        format!("* BYE [ALERT] {ENDED}"),
        "after a sweep that could not see the connection"
    );
}

#[tokio::test]
async fn pop3_a_sign_out_between_the_check_and_the_registration_still_closes_it() {
    let (held, dir) = held();
    let (l, addr) = listener().await;
    tokio::spawn(crate::pop3::serve_plain(l, dir.clone(), None));
    let mut c = Client::connect(addr).await;
    c.line().await;
    c.send(&format!("USER {USER}\r\n")).await;
    c.line().await;
    c.send(&format!("PASS {PASSWORD}\r\n")).await;

    held.asked.notified().await;
    held.cleared.store(true, Ordering::SeqCst);
    dir.signed_in().end(USER);
    held.answer.notify_one();

    let r = c.line().await;
    assert!(r.starts_with("+OK"), "signed in: {r}");
    assert_eq!(
        next_line(&mut c).await.trim_end(),
        format!("-ERR {ENDED}"),
        "after a sign-out published while the connection was not registered"
    );
}

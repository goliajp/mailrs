//! Connections signed in, by account, so that ending an account's sign-ins
//! (its password cleared) also closes the ones already open: an IMAP client
//! sitting in IDLE would otherwise keep reading mail for as long as it
//! stays connected.

use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, Weak};
use std::time::Instant;

use tokio::sync::Notify;

/// What a closed connection is told, in every protocol. ASCII only: it
/// goes into protocol responses.
pub const ENDED: &str = "Signed out: an administrator ended this account's sign-ins";

#[derive(Default)]
struct Flag {
    ended: AtomicBool,
    notify: Notify,
}

/// One registry per process, shared by every listener.
#[derive(Clone, Default)]
pub struct Signins {
    by_account: Arc<Mutex<HashMap<String, Vec<Weak<Flag>>>>>,
    /// When each account's sign-ins were last ended, for a connection that
    /// signs in while that happens; see [`Signins::enter_since`].
    last_ended: Arc<Mutex<HashMap<String, Instant>>>,
    /// When a sweep last listed the accounts signed in; see
    /// [`Signins::sweep`].
    last_swept: Arc<Mutex<Option<Instant>>>,
}

/// A connection's place in the registry, held for as long as it is signed
/// in; dropping it leaves.
pub struct SignedIn {
    flag: Arc<Flag>,
    missed_sweep: bool,
}

impl Signins {
    fn enter(&self, account: &str) -> SignedIn {
        let flag = Arc::new(Flag::default());
        let mut map = self.by_account.lock().expect("signins lock");
        let list = map.entry(account.to_ascii_lowercase()).or_default();
        // connections that have closed since leave dead entries behind
        list.retain(|w| w.strong_count() > 0);
        list.push(Arc::downgrade(&flag));
        SignedIn {
            flag,
            missed_sweep: false,
        }
    }

    /// Register a connection that has signed in as `account`. `since` is a
    /// moment taken before its credentials were checked: an end or a sweep
    /// landing between the check and this registration finds nothing to
    /// close, and the connection would stay open for good.
    ///
    /// The connection is ended already if the account's sign-ins were
    /// ended at or after `since`. If a sweep listed the accounts at or
    /// after `since`, it may have passed over this one: the connection
    /// learns it from [`SignedIn::missed_sweep`] and has to ask, as the
    /// sweep would have, whether the account's password is cleared.
    pub fn enter_since(&self, account: &str, since: Instant) -> SignedIn {
        let mut signed_in = self.enter(account);
        let ended = self.last_ended.lock().expect("signins lock");
        if ended
            .get(&account.to_ascii_lowercase())
            .is_some_and(|at| *at >= since)
        {
            signed_in.flag.ended.store(true, Ordering::SeqCst);
        }
        drop(ended);
        // read after registering: a sweep stamps its time before listing,
        // so one that listed before this registration is seen here
        signed_in.missed_sweep = self
            .last_swept
            .lock()
            .expect("signins lock")
            .is_some_and(|at| at >= since);
        signed_in
    }

    /// The accounts with a connection open now, for a sweep that ends
    /// those whose password has been cleared. A connection registering
    /// while the sweep runs is not listed, and learns it from
    /// [`SignedIn::missed_sweep`].
    pub fn sweep(&self) -> Vec<String> {
        *self.last_swept.lock().expect("signins lock") = Some(Instant::now());
        self.accounts()
    }

    fn accounts(&self) -> Vec<String> {
        let mut map = self.by_account.lock().expect("signins lock");
        map.retain(|_, list| {
            list.retain(|w| w.strong_count() > 0);
            !list.is_empty()
        });
        map.keys().cloned().collect()
    }

    /// End every open connection signed in as `account`. Returns how many
    /// there were.
    pub fn end(&self, account: &str) -> usize {
        self.last_ended
            .lock()
            .expect("signins lock")
            .insert(account.to_ascii_lowercase(), Instant::now());
        let list = self
            .by_account
            .lock()
            .expect("signins lock")
            .remove(&account.to_ascii_lowercase())
            .unwrap_or_default();
        let mut ended = 0;
        for flag in list.iter().filter_map(Weak::upgrade) {
            flag.ended.store(true, Ordering::SeqCst);
            flag.notify.notify_waiters();
            ended += 1;
        }
        ended
    }
}

impl SignedIn {
    /// Whether a sweep listed the accounts between this connection's
    /// credential check and its registration; see [`Signins::enter_since`].
    pub fn missed_sweep(&self) -> bool {
        self.missed_sweep
    }

    /// Resolves once the account's sign-ins are ended, at once if they
    /// already were.
    pub async fn ended(&self) {
        loop {
            let notified = self.flag.notify.notified();
            if self.flag.ended.load(Ordering::SeqCst) {
                return;
            }
            notified.await;
        }
    }
}

/// [`SignedIn::ended`] for a connection that may not have signed in yet:
/// one that has not signed in never ends this way.
pub async fn ended(signed_in: Option<&SignedIn>) {
    match signed_in {
        Some(s) => s.ended().await,
        None => std::future::pending().await,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    #[tokio::test]
    async fn ending_an_account_ends_its_connections_and_no_others() {
        let signins = Signins::default();
        let (a1, a2, b) = (
            signins.enter("A@x.test"),
            signins.enter("a@x.test"),
            signins.enter("b@x.test"),
        );
        assert_eq!(signins.end("a@x.test"), 2);
        tokio::time::timeout(Duration::from_secs(1), a1.ended())
            .await
            .expect("a1 ended");
        tokio::time::timeout(Duration::from_secs(1), a2.ended())
            .await
            .expect("a2 ended");
        assert!(
            tokio::time::timeout(Duration::from_millis(50), b.ended())
                .await
                .is_err(),
            "another account's connection ended"
        );
    }

    #[tokio::test]
    async fn a_connection_waiting_when_it_ends_wakes() {
        let signins = Signins::default();
        let a = signins.enter("a@x.test");
        let waiting = tokio::spawn(async move { a.ended().await });
        tokio::time::sleep(Duration::from_millis(20)).await;
        signins.end("a@x.test");
        tokio::time::timeout(Duration::from_secs(1), waiting)
            .await
            .expect("woke")
            .expect("task");
    }

    /// The SMTP race: the client is told it signed in, the account's
    /// sign-ins end, and only then does the connection register.
    #[tokio::test]
    async fn an_end_between_the_answer_and_the_registration_still_ends_it() {
        let signins = Signins::default();
        let since = Instant::now();
        assert_eq!(signins.end("A@x.test"), 0, "nothing registered yet");
        let late = signins.enter_since("a@x.test", since);
        tokio::time::timeout(Duration::from_secs(1), late.ended())
            .await
            .expect("the late registration was not ended");
    }

    #[tokio::test]
    async fn an_end_before_the_sign_in_does_not_end_a_later_one() {
        let signins = Signins::default();
        signins.end("a@x.test");
        let after = signins.enter_since("a@x.test", Instant::now());
        assert!(
            tokio::time::timeout(Duration::from_millis(50), after.ended())
                .await
                .is_err(),
            "a sign-in after the end was ended"
        );
    }

    /// The sweep race: the credentials are checked, a sweep lists the
    /// accounts signed in, and only then does the connection register.
    #[test]
    fn a_sweep_between_the_check_and_the_registration_is_reported() {
        let signins = Signins::default();
        let since = Instant::now();
        assert!(signins.sweep().is_empty(), "nothing registered yet");
        assert!(signins.enter_since("a@x.test", since).missed_sweep());
        assert!(
            !signins
                .enter_since("a@x.test", Instant::now())
                .missed_sweep(),
            "a sweep before the check could not have missed this sign-in"
        );
    }

    #[test]
    fn only_accounts_with_an_open_connection_are_listed() {
        let signins = Signins::default();
        let _a = signins.enter("A@x.test");
        drop(signins.enter("b@x.test"));
        assert_eq!(signins.sweep(), vec!["a@x.test".to_string()]);
    }

    #[test]
    fn a_closed_connection_is_not_counted() {
        let signins = Signins::default();
        drop(signins.enter("a@x.test"));
        assert_eq!(signins.end("a@x.test"), 0);
    }
}

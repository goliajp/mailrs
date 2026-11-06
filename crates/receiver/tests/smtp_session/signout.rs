//! A submission connection whose credentials were checked just before a
//! sweep for cleared accounts ran, and which registered just after it, is
//! closed all the same. The core is held inside the credential check while
//! the password is cleared and the sweep runs, so the order is the same on
//! every run.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use base64::Engine as _;
use mailrs_core::users::UserStore;
use mailrs_core_api::method::admin::EffectivePermissionsResponse;
use mailrs_receiver::{AccountStoreError, CredentialStore};
use tokio::sync::Notify;

use crate::harness::{Client, receiver_asking};

const DAVE: &str = "dave@golia.test";
const PW: &str = "a-real-password-1";

/// A core that knows [`DAVE`] and, asked for his password, waits to be let
/// go before answering.
struct Held {
    hash: String,
    asked: Notify,
    answer: Notify,
    cleared: AtomicBool,
}

#[async_trait::async_trait]
impl CredentialStore for Held {
    async fn password_hash(&self, address: &str) -> Result<Option<String>, AccountStoreError> {
        if address != DAVE {
            return Ok(None);
        }
        self.asked.notify_one();
        self.answer.notified().await;
        Ok(Some(self.hash.clone()))
    }
    async fn effective_permissions(
        &self,
        _: &str,
    ) -> Result<Option<EffectivePermissionsResponse>, AccountStoreError> {
        Ok(None)
    }
    async fn password_cleared(&self, address: &str) -> Result<bool, AccountStoreError> {
        Ok(address == DAVE && self.cleared.load(Ordering::SeqCst))
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_sweep_between_the_password_check_and_the_registration_still_closes_it() {
    let core = Arc::new(Held {
        hash: UserStore::hash_password(PW).unwrap(),
        asked: Notify::new(),
        answer: Notify::new(),
        cleared: AtomicBool::new(false),
    });
    let rx = receiver_asking("signout-sweep", core.clone()).await;
    let mut c = Client::connect(rx.port).await;
    let plain = base64::engine::general_purpose::STANDARD.encode(format!("\0{DAVE}\0{PW}"));
    c.send(&format!("AUTH PLAIN {plain}")).await;

    core.asked.notified().await;
    core.cleared.store(true, Ordering::SeqCst);
    mailrs_receiver::smtp_session::end_cleared(&rx.ctx).await;
    core.answer.notify_one();

    let r = c.reply().await;
    assert!(r.starts_with("235"), "signed in: {r}");
    let r = tokio::time::timeout(Duration::from_secs(5), c.reply())
        .await
        .unwrap_or_else(|_| "<nothing within 5s>".into());
    assert!(
        r.starts_with("421") && r.contains(mailrs_auth_guard::signins::ENDED),
        "after a sweep that could not see the connection: {r}"
    );
}

//! Creating an account never rewrites an existing one, and a password
//! change touches only the password.
//!
//! The admin console used to change a password by re-posting the account
//! to the create route, which replaced the whole blob: the quota went back
//! to the default and the recovery email was cleared.

use std::sync::Arc;

use mailrs_core_api::client::Client;
use mailrs_core_api::method::admin as adm;

async fn serve_fastcore() -> Client {
    let store =
        Arc::new(kevy_embedded::Store::open(kevy_embedded::Config::default()).expect("kevy"));
    let mailbox = mailrs_mailbox_kevy::KevyMailboxStore::new(store);
    let state = Arc::new(mailrs_fastcore::FastcoreState::new(mailbox));
    let router = mailrs_fastcore::build_router(state);
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind");
    let addr = listener.local_addr().expect("addr");
    tokio::spawn(async move { axum::serve(listener, router).await });
    Client::new(format!("http://{addr}"), "secret")
}

// the client's set_quota and set_recovery_email decode a body from fastcore's
// 204 and report an error although the write landed; the reads below are
// what these tests rely on
fn set(_written: mailrs_core_api::error::ApiResult<()>) {}

fn create(address: &str, password: &str) -> adm::AddAccountRequest {
    adm::AddAccountRequest {
        address: address.into(),
        display_name: "Alice".into(),
        password: password.into(),
        password_hash: None,
    }
}

#[tokio::test]
async fn a_second_create_is_a_conflict_and_leaves_the_account_alone() {
    let core = serve_fastcore().await;
    core.add_account(&create("alice@example.com", "first"))
        .await
        .expect("create");
    set(core
        .set_quota(
            "alice@example.com",
            &adm::SetQuotaRequest { quota_bytes: 1234 },
        )
        .await);

    let again = core
        .add_account(&create("alice@example.com", "second"))
        .await
        .expect_err("the address is taken");
    assert_eq!(again.status_code(), 409);

    let acc = core
        .get_account_with_hash("alice@example.com")
        .await
        .expect("read");
    assert_eq!(acc.public.quota_bytes, 1234);
    let hash = acc.password_hash.expect("hash");
    assert!(mailrs_core::users::UserStore::verify_hash("first", &hash));
}

#[tokio::test]
async fn an_empty_password_is_refused() {
    let core = serve_fastcore().await;
    assert!(
        core.add_account(&create("bob@example.com", ""))
            .await
            .is_err()
    );
    assert!(
        core.get_account_with_hash("bob@example.com")
            .await
            .is_err_and(|e| e.status_code() == 404),
        "nothing was stored"
    );
}

#[tokio::test]
async fn a_password_change_keeps_quota_and_recovery_email() {
    let core = serve_fastcore().await;
    core.add_account(&create("carol@example.com", "old"))
        .await
        .expect("create");
    set(core
        .set_quota(
            "carol@example.com",
            &adm::SetQuotaRequest { quota_bytes: 42 },
        )
        .await);
    set(core
        .set_recovery_email(
            "carol@example.com",
            &adm::UpdateRecoveryEmailRequest {
                recovery_email: "carol@backup.example".into(),
            },
        )
        .await);

    core.set_account_password(
        "carol@example.com",
        &adm::SetPasswordRequest {
            password_hash: hash_of("new"),
        },
    )
    .await
    .expect("password");

    let acc = core
        .get_account_with_hash("carol@example.com")
        .await
        .expect("read");
    assert_eq!(acc.public.quota_bytes, 42);
    assert_eq!(
        acc.public.recovery_email.as_deref(),
        Some("carol@backup.example")
    );
    let hash = acc.password_hash.expect("hash");
    assert!(mailrs_core::users::UserStore::verify_hash("new", &hash));
    assert!(!mailrs_core::users::UserStore::verify_hash("old", &hash));
}

// the caller hashes; fastcore stores what it is given
fn hash_of(password: &str) -> String {
    use argon2::password_hash::{PasswordHasher, SaltString, rand_core::OsRng};
    argon2::Argon2::default()
        .hash_password(password.as_bytes(), &SaltString::generate(&mut OsRng))
        .expect("hash")
        .to_string()
}

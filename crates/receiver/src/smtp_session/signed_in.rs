//! Submission connections signed in, so that ending an account's sign-ins
//! closes the ones already open.

use mailrs_auth_guard::signins::SignedIn;
use mailrs_smtp_proto::session::{Session, State};

use super::ConnectionContext;

/// Register the connection once its AUTH has succeeded. `since` is taken
/// before the command that signed in was read, so an end or a sweep that
/// lands after the credentials were checked still closes it.
pub(super) async fn note_signin(
    ctx: &ConnectionContext,
    session: &Session,
    signed_in: &mut Option<SignedIn>,
    since: std::time::Instant,
) {
    if signed_in.is_some() {
        return;
    }
    let State::Authenticated { username, .. } = &session.state else {
        return;
    };
    let entered = ctx.signins.enter_since(username, since);
    if entered.missed_sweep() && cleared(ctx, username).await {
        ctx.signins.end(username);
    }
    *signed_in = Some(entered);
}

/// Close the submission connections of every account signed in here whose
/// password has been cleared: what runs each time the sign-out
/// subscription is (re)made, for the sign-outs published while it was
/// down.
pub async fn end_cleared(ctx: &ConnectionContext) {
    for account in ctx.signins.sweep() {
        if cleared(ctx, &account).await {
            let closed = ctx.signins.end(&account);
            tracing::info!(
                account = %mailrs_auth_guard::login::loggable_account(&account),
                closed,
                "closed submission connections of an account signed out while unheard"
            );
        }
    }
}

/// `false` when the core cannot say: the connection stays as it was.
async fn cleared(ctx: &ConnectionContext, account: &str) -> bool {
    match &ctx.credential_store {
        Some(credentials) => matches!(credentials.password_cleared(account).await, Ok(true)),
        None => false,
    }
}

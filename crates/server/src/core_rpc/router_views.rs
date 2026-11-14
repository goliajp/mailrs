//! Routes for search, the sent list, marking a view as read, filing into a
//! bucket, holds and fraud verdicts, invitations, the RCPT check and
//! re-judge batches: the surface the kevy core grew while this one was not
//! running.

use std::sync::Arc;

use axum::Router;
use axum::routing::{get, post};

use mailrs_core_api::method::admin as adm;
use mailrs_core_api::method::conversation as conv;
use mailrs_core_api::method::message as msg;
use mailrs_core_api::method::thread as th;

use super::CoreRpcState;
use super::handlers::{admin, filing, holds, views};

pub(super) fn view_routes(state: Arc<CoreRpcState>) -> Router {
    Router::new()
        .route(
            conv::PATH_SEARCH_CONVERSATIONS,
            post(views::search_conversations),
        )
        .route(th::PATH_LIST_SENT_MESSAGES, get(views::list_sent_messages))
        .route(th::PATH_MARK_ALL_READ, post(views::mark_all_read))
        .route(th::PATH_MARK_LIST_READ, post(views::mark_list_read))
        .route(th::PATH_MARK_JUNK, post(filing::mark_junk))
        .route(th::PATH_MARK_NOT_JUNK, post(filing::mark_not_junk))
        .route(th::PATH_MARK_NOTIFICATION, post(filing::mark_notification))
        .route(th::PATH_MARK_PROMOTION, post(filing::mark_promotion))
        .route(th::PATH_MOVE_TO_INBOX, post(filing::move_to_inbox))
        .route(
            th::PATH_QUARANTINE_LIST,
            get(mailrs_core_routes::hold::list_held::<CoreRpcState>),
        )
        .route(
            th::PATH_QUARANTINE_RELEASE,
            post(mailrs_core_routes::hold::release::<CoreRpcState>),
        )
        .route(
            th::PATH_THREAD_RELEASES,
            get(mailrs_core_routes::hold::thread_releases::<CoreRpcState>),
        )
        .route(
            th::PATH_FRAUD_VERDICT,
            get(mailrs_core_routes::hold::fraud_verdict::<CoreRpcState>),
        )
        .route(
            adm::PATH_RECIPIENT_DELIVERABLE,
            get(holds::recipient_deliverable),
        )
        .route(
            msg::PATH_GET_INVITE,
            get(mailrs_core_routes::message::invite::<CoreRpcState>),
        )
        .route(adm::PATH_REJUDGE_BATCHES, get(admin::rejudge_batches))
        .route(
            adm::PATH_RELEASE_REJUDGE_BATCH,
            post(admin::release_rejudge_batch),
        )
        .route(
            adm::PATH_CONFIRM_REJUDGE_BATCH,
            post(admin::confirm_rejudge_batch),
        )
        .route(
            adm::PATH_DISCARD_REJUDGE_BATCH,
            post(admin::discard_rejudge_batch),
        )
        .with_state(state)
}

//! Accounts, aliases and email-group members, merged into the admin
//! surface by `rest.rs`.

use std::sync::Arc;

use crate::*;

pub(super) fn directory_routes() -> axum::Router<Arc<WebState>> {
    use crate::handlers::admin;
    use axum::routing::{delete, get, post};

    axum::Router::new()
        .route(
            "/api/admin/accounts",
            get(admin::list_accounts).post(admin::add_account),
        )
        .route(
            "/api/admin/accounts/{address}",
            delete(admin::remove_account).put(admin::update_account),
        )
        .route(
            "/api/admin/accounts/{address}/password",
            post(admin::set_account_password),
        )
        .route(
            "/api/admin/accounts/{address}/quota",
            get(admin::get_account_quota).post(admin::set_account_quota),
        )
        .route(
            "/api/admin/accounts/{address}/groups",
            get(admin::list_account_groups),
        )
        .route(
            "/api/admin/accounts/{address}/overrides",
            get(admin::get_account_overrides).put(admin::set_account_overrides),
        )
        .route(
            "/api/admin/aliases",
            get(admin::list_aliases).post(admin::add_alias),
        )
        .route("/api/admin/aliases/{id}", delete(admin::remove_alias))
        .route(
            "/api/admin/email-groups/{id}/members",
            get(admin::list_email_group_members).post(admin::add_email_group_member),
        )
        .route(
            "/api/admin/email-groups/{id}/members/{address}",
            delete(admin::remove_email_group_member),
        )
}

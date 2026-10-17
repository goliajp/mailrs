//! Who mail is for: the alias table and the recipient check.

use super::*;

pub(super) fn directory_routes(r: Router<Arc<FastcoreState>>) -> Router<Arc<FastcoreState>> {
    // Aliases live in the fastcore-embedded kevy so the spool drain (also
    // in-process) can resolve `contact@golia.jp -> lihao` and similar
    // single-hop forwards. Distinct namespace from webapi's network-kevy
    // `admin:aliases` hash — that older store is not consulted by the drain
    // and stays around only until UI wiring catches up.
    r.route(
        "/v1/admin/aliases:local",
        get(list_local_aliases).post(upsert_local_alias),
    )
    .route(
        "/v1/admin/aliases:local/{source}",
        delete(delete_local_alias_route),
    )
    .route(
        adm::PATH_RECIPIENT_DELIVERABLE,
        get(crate::spool_drain::recipient_deliverable_route),
    )
}

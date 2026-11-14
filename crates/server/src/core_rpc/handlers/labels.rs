//! The label ids on listed rows.

use mailrs_core_api::types::ConversationSummaryWire;

use crate::core_rpc::CoreRpcState;

/// `items` with each one's label ids.
pub(crate) async fn with_labels(
    state: &CoreRpcState,
    user: &str,
    mut items: Vec<ConversationSummaryWire>,
) -> Vec<ConversationSummaryWire> {
    let by_thread = match state.mailbox.labels_by_thread(user).await {
        Ok(m) => m,
        Err(e) => {
            tracing::warn!(%user, error = %e, "labels not read; rows shown without them");
            return items;
        }
    };
    for item in &mut items {
        if let Some(ids) = by_thread.get(&item.thread_id) {
            item.labels = ids.clone();
        }
    }
    items
}

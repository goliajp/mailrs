//! Core-side spool consumer (P6-S7).
//!
//! In the split topology the receiver writes accepted mail to a spool maildir
//! and emits `SpoolDelivered`. This module is the other half: fetch the spool
//! file, decode the envelope, and run the **same** resolve → sieve → quota →
//! deliver → relay path the monolith DATA handler runs — by reusing the
//! `mailrs_receiver::smtp_session::delivery` helpers (now deps-parameterized)
//! and handing each local delivery to the existing post-delivery consumer over
//! `process_tx` (so indexing / events / iTIP stay exactly as in the monolith).
//!
//! Idempotency: an in-process in-flight set dedups the `SpoolDelivered` notify
//! racing the reconcile sweep; the spool file is deleted only after all
//! recipients are handled; `index_message` is itself idempotent. An
//! undecodable spool file is deleted (dead-lettered) rather than retried.

use std::sync::Arc;

use dashmap::DashMap;

use mailrs_core::event_bus::{EventBus, SmtpEvent};
use mailrs_core::spool::decode_spool_blob;
use mailrs_delivery_executor::DeliveryExecutor;
use mailrs_message_store::{MessageId, MessageStore};
use mailrs_outbound_queue::{Notifier, QueueStore};
use mailrs_receiver::AccountStore;
use mailrs_receiver::QuotaStore;
use mailrs_receiver::smtp_session::DeliveryDeps;
use mailrs_receiver::smtp_session::delivered::{DeliveredMessage, ProcessTx};
use mailrs_receiver::smtp_session::delivery::{
    RemoteEnqueueResult, apply_sieve_actions, classify_recipients, enqueue_remote_rcpts,
};

/// Everything the spool consumer needs — the union of what the delivery
/// helpers + the local-delivery + post-delivery hand-off require, built once
/// from the core's concrete spg/kevy stores at bootstrap.
pub(crate) struct SpoolConsumeDeps {
    /// the spool maildir mailbox the receiver writes to (`{spool_root}/incoming`).
    pub spool_incoming_path: String,
    /// fetch + delete spool files.
    pub spool_store: Arc<dyn MessageStore>,
    /// group-commit delivery into the user maildir.
    pub delivery_executor: DeliveryExecutor,
    /// hand each delivered message to the existing post-delivery consumer
    /// (index / NewMessage / iTIP / post_delivery) — reuses the monolith path.
    pub process_tx: ProcessTx,
    pub account_store: Option<Arc<dyn AccountStore>>,
    pub quota_store: Option<Arc<dyn QuotaStore>>,
    pub outbound_enqueue: Option<Arc<dyn QueueStore>>,
    pub queue_notifier: Option<Arc<dyn Notifier>>,
    pub event_bus: EventBus,
    pub hostname: String,
    pub srs_secret: Option<String>,
    pub local_domains: Vec<String>,
    /// the user maildir root (delivery target), not the spool root.
    pub maildir_root: String,
    /// dedups the notify handler vs the reconcile sweep grabbing one file.
    pub in_flight: Arc<DashMap<String, ()>>,
}

/// RAII guard: remove `spool_id` from the in-flight set on every exit path.
struct InFlight<'a> {
    set: &'a DashMap<String, ()>,
    id: String,
}
impl Drop for InFlight<'_> {
    fn drop(&mut self) {
        self.set.remove(&self.id);
    }
}

/// Consume one spool file: fetch → decode → resolve/sieve/deliver/relay →
/// delete. Idempotent and safe to call from both the notify handler and the
/// reconcile sweep.
pub(crate) async fn consume_spool_file(spool_id: String, deps: &SpoolConsumeDeps) {
    // dedup: skip if another task is already handling this file.
    if deps.in_flight.insert(spool_id.clone(), ()).is_some() {
        return;
    }
    let _guard = InFlight {
        set: &deps.in_flight,
        id: spool_id.clone(),
    };

    let mid = MessageId(spool_id.clone());
    let blob = match deps
        .spool_store
        .fetch(&deps.spool_incoming_path, &mid)
        .await
    {
        Ok(Some(b)) => b,
        Ok(None) => return, // already consumed + deleted by a prior pass
        Err(e) => {
            tracing::warn!(event = "spool_fetch_failed", spool_id = %spool_id, error = %e, "spool fetch failed; reconcile will retry");
            return;
        }
    };

    let (env, body) = match decode_spool_blob(&blob) {
        Ok(x) => x,
        Err(e) => {
            // undecodable / spoofed: dead-letter (delete) so it isn't retried forever.
            tracing::error!(event = "spool_decode_failed", spool_id = %spool_id, error = %e, "spool file undecodable; deleting (dead-letter)");
            let _ = deps
                .spool_store
                .delete(&deps.spool_incoming_path, &mid)
                .await;
            return;
        }
    };

    let full_message = Arc::new(body.to_vec());
    let msg_size = full_message.len();
    let msg_message_id = mailrs_mailbox::threading::extract_message_id(&full_message);
    let msg_in_reply_to = mailrs_mailbox::threading::extract_in_reply_to(&full_message);

    let ddeps = DeliveryDeps {
        local_domains: &deps.local_domains,
        account_store: &deps.account_store,
        outbound_enqueue: &deps.outbound_enqueue,
        queue_notifier: &deps.queue_notifier,
        event_bus: &deps.event_bus,
        hostname: &deps.hostname,
        srs_secret: &deps.srs_secret,
    };

    let (local_rcpts, remote_rcpts) = classify_recipients(&env.forward_paths, &ddeps).await;

    // local delivery — mirrors the monolith DATA handler, but hands off to the
    // existing post-delivery consumer over process_tx instead of inline.
    for rcpt in &local_rcpts {
        let (rcpt_folder, skip_delivery) = apply_sieve_actions(
            rcpt,
            &env.target_folder,
            &env.reverse_path,
            &full_message,
            &ddeps,
        )
        .await;
        if skip_delivery {
            continue;
        }
        let Some((local, domain)) = rcpt.split_once('@') else {
            continue;
        };

        // quota: enforced here in the core (the receiver accepted before
        // resolution — the documented narrow-receiver behaviour change).
        if let (Some(ds), Some(qs)) = (&deps.account_store, &deps.quota_store)
            && let Ok(Some(quota)) = ds.quota(rcpt).await
            && quota > 0
        {
            let usage = qs.user_storage_usage(rcpt).await;
            if usage + msg_size as u64 > quota as u64 {
                tracing::warn!(event = "spool_quota_exceeded", user = %rcpt, usage_bytes = usage, quota_bytes = quota, "over quota; message held in spool-derived delivery dropped");
                continue;
            }
        }

        let path = format!("{}/{domain}/{local}", deps.maildir_root);
        match deps
            .delivery_executor
            .deliver(path.clone(), full_message.clone())
            .await
        {
            Ok(id) => {
                let msg = DeliveredMessage {
                    maildir_id: id.to_string(),
                    user: format!("{local}@{domain}"),
                    rcpt: rcpt.clone(),
                    rcpt_folder,
                    reverse_path: env.reverse_path.clone(),
                    full_message: full_message.clone(),
                    msg_message_id: msg_message_id.clone(),
                    msg_in_reply_to: msg_in_reply_to.clone(),
                    msg_size,
                };
                // backpressure to the consumer's rate; never drop (on disk + indexed).
                let _ = deps.process_tx.send(msg).await;
            }
            Err(e) => {
                tracing::error!(event = "spool_maildir_deliver_failed", rcpt = %rcpt, path = %path, error = %e, "maildir delivery from spool failed");
            }
        }
    }

    // FBL: ARF complaint to abuse@ / postmaster@ → suppression list.
    if local_rcpts
        .iter()
        .any(|r| r.starts_with("abuse@") || r.starts_with("postmaster@"))
        && let Some(report) = mailrs_arf::parse(&full_message)
        && let Some(reported_addr) = report.complainant()
        && let Some(ref queue) = deps.outbound_enqueue
    {
        let _ = queue
            .add_suppression(
                reported_addr,
                &format!("FBL complaint: {}", report.feedback_type),
                None,
            )
            .await;
    }

    // relay remote recipients.
    if !remote_rcpts.is_empty() {
        match enqueue_remote_rcpts(
            &remote_rcpts,
            &env.reverse_path,
            &full_message,
            env.is_authenticated,
            env.conn_id,
            &ddeps,
        )
        .await
        {
            RemoteEnqueueResult::Ok | RemoteEnqueueResult::PartialFailure => {}
            RemoteEnqueueResult::RelayDenied => {
                // the receiver's coarse guard should have blocked this already;
                // log if a relay-denied case reaches the core.
                tracing::warn!(
                    event = "spool_relay_denied",
                    "relay-denied recipients reached the core consumer"
                );
            }
        }
    }

    if !local_rcpts.is_empty() {
        deps.event_bus.emit(SmtpEvent::MessageDelivered {
            id: env.conn_id,
            from: env.reverse_path.clone(),
            to: local_rcpts,
            size: msg_size,
        });
    }

    // done — delete the spool file (all recipients handled).
    if let Err(e) = deps
        .spool_store
        .delete(&deps.spool_incoming_path, &mid)
        .await
    {
        tracing::warn!(event = "spool_delete_failed", spool_id = %spool_id, error = %e, "spool delete failed; reconcile may re-consume (idempotent index)");
    }
}

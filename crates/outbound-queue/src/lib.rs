use std::sync::Arc;

pub mod dkim_sign;
pub mod dsn;
pub mod mta_sts;
pub mod queue;
pub mod retry;
pub mod worker;

pub use dkim_sign::DkimSignConfig;
pub use queue::{QueueStatus, QueuedMessage};
pub use retry::{retry_delay_secs, should_bounce};
pub use worker::{DeliveryWorker, WorkerConfig, group_by_domain};

/// outbound delivery event for external observers
#[derive(Debug, Clone)]
pub enum DeliveryEvent {
    Attempt { queue_id: i64, domain: String },
    Success { queue_id: i64, domain: String },
    Failed { queue_id: i64, domain: String, error: String },
    Bounced { queue_id: i64, sender: String },
}

/// callback type for delivery events
pub type DeliveryEventSender = Arc<dyn Fn(DeliveryEvent) + Send + Sync>;

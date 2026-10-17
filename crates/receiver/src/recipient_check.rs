//! Refusing mail for a local address that nobody receives, at RCPT.
//!
//! The drain cannot deliver such mail and a bounce afterwards would go to
//! the envelope sender, which on this mail is nearly always forged. So the
//! receiver asks fastcore while the sender is still connected. An answer
//! that does not come within two seconds, or comes back as an error, means
//! the recipient is accepted as before: receiving mail comes first.

use std::time::Duration;

use mailrs_core_api::client::Client;

const ASK_TIMEOUT: Duration = Duration::from_secs(2);

pub struct RecipientCheck {
    client: Client,
}

impl RecipientCheck {
    pub fn new(base_url: String, secret: String) -> Self {
        Self {
            client: Client::new(base_url, secret),
        }
    }

    /// True only when fastcore answered that `address` reaches nobody.
    pub async fn is_unknown(&self, address: &str) -> bool {
        match tokio::time::timeout(ASK_TIMEOUT, self.client.recipient_deliverable(address)).await {
            Ok(Ok(deliverable)) => !deliverable,
            Ok(Err(e)) => {
                tracing::warn!(%address, error = %e, "recipient check failed; accepting");
                false
            }
            Err(_) => {
                tracing::warn!(%address, "recipient check timed out; accepting");
                false
            }
        }
    }
}

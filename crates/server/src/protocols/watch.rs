//! IDLE's view of the in-process event bus: a `NewMessage` for the user,
//! or missed events, mean the mailbox may have changed.

use std::sync::Arc;

use mailrs_mail_protocols::backend::Watch;
use tokio::sync::broadcast::{Receiver, error::RecvError};

use crate::event_bus::{BroadcastEvent, EventBus, SmtpEvent};

pub(crate) struct BusWatch {
    rx: Receiver<Arc<BroadcastEvent>>,
    user: String,
}

impl BusWatch {
    pub(crate) fn new(events: &EventBus, user: &str) -> Self {
        Self {
            rx: events.subscribe(),
            user: user.to_string(),
        }
    }
}

impl Watch for BusWatch {
    async fn changed(&mut self) {
        loop {
            match self.rx.recv().await {
                Ok(env) => {
                    if let SmtpEvent::NewMessage { user, .. } = &env.event
                        && *user == self.user
                    {
                        return;
                    }
                }
                // events were dropped; one of them may have been ours
                Err(RecvError::Lagged(_)) => return,
                Err(RecvError::Closed) => return std::future::pending().await,
            }
        }
    }
}

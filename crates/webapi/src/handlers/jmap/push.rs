//! Push over EventSource (RFC 8620 §7.3).
//!
//! A `StateChange` is sent when the account's state moves. New mail for
//! the account is noticed at once from the event bus the web client's
//! live updates use; every other change (flags, moves, deletes, from IMAP
//! or the web as much as from JMAP) is noticed by reading the state every
//! [`CHECK`], one read per account however many connections it has open.

use std::sync::Arc;
use std::time::Duration;

use axum::extract::{Extension, Query, State};
use axum::http::StatusCode;
use axum::response::sse::{Event, KeepAlive, Sse};
use axum::response::{IntoResponse, Response};
use serde::Deserialize;
use serde_json::json;
use tokio::sync::broadcast;
use tokio::time::{Interval, MissedTickBehavior};

use super::ctx::Ctx;
use super::limits::Slot;
use super::{mailbox, state, view};
use crate::WebState;
use crate::handlers::conversations::AuthedUser;

/// How often a connection reads the state.
const CHECK: Duration = Duration::from_secs(5);
/// Push connections one account may hold open.
pub const MAX_CONNECTIONS: usize = 8;

#[derive(Deserialize)]
pub struct Params {
    #[serde(default)]
    types: Option<String>,
    #[serde(default)]
    closeafter: Option<String>,
    #[serde(default)]
    ping: Option<u64>,
}

const TYPES: [&str; 3] = ["Email", "Mailbox", "Thread"];

struct Conn {
    ctx: Ctx,
    types: Vec<&'static str>,
    close_after_state: bool,
    bus: broadcast::Receiver<String>,
    check: Interval,
    ping: Option<(Interval, u64)>,
    last: Option<String>,
    done: bool,
    _slot: Slot,
}

async fn read_state(ctx: &Ctx) -> Option<String> {
    let v = view::recent(ctx, CHECK).await.ok()?;
    let mailboxes = mailbox::objects(ctx, &v).await.ok()?;
    state::now(&ctx.user, &v, &mailboxes).ok().map(|n| n.state)
}

impl Conn {
    /// The next event to send, `None` to end the response.
    async fn next(&mut self) -> Option<Event> {
        if self.done {
            return None;
        }
        loop {
            let Conn {
                check,
                bus,
                ping,
                ctx,
                ..
            } = self;
            let ping_secs = ping.as_ref().map_or(0, |(_, s)| *s);
            let ping = async {
                match ping.as_mut() {
                    Some((tick, _)) => {
                        tick.tick().await;
                    }
                    None => std::future::pending::<()>().await,
                }
            };
            let look = tokio::select! {
                _ = check.tick() => true,
                msg = bus.recv() => match msg {
                    Ok(text) => crate::handlers::events::addressed_to(&text, &ctx.user),
                    Err(broadcast::error::RecvError::Lagged(_)) => true,
                    Err(broadcast::error::RecvError::Closed) => false,
                },
                () = ping => {
                    return Some(Event::default().event("ping").data(json!({ "interval": ping_secs }).to_string()));
                }
            };
            if !look {
                continue;
            }
            let Some(now) = read_state(&self.ctx).await else {
                continue;
            };
            let moved = self.last.as_ref().is_some_and(|l| *l != now);
            let first = self.last.is_none();
            self.last = Some(now.clone());
            if first || !moved {
                continue;
            }
            self.done = self.close_after_state;
            let changed: serde_json::Map<String, serde_json::Value> = self
                .types
                .iter()
                .map(|t| (t.to_string(), json!(now)))
                .collect();
            let body = json!({ "@type": "StateChange", "changed": { &self.ctx.user: changed } });
            return Some(Event::default().event("state").data(body.to_string()));
        }
    }
}

/// The types asked for: `*` or nothing is every type.
fn types_of(raw: Option<&str>) -> Vec<&'static str> {
    let raw = raw.unwrap_or("*");
    if raw.split(',').any(|t| t.trim() == "*") {
        return TYPES.to_vec();
    }
    TYPES
        .into_iter()
        .filter(|t| raw.split(',').any(|w| w.trim() == *t))
        .collect()
}

/// GET /jmap/eventsource/?types=&closeafter=&ping=
pub async fn eventsource(
    State(state): State<Arc<WebState>>,
    Extension(AuthedUser(user)): Extension<AuthedUser>,
    Query(params): Query<Params>,
) -> Response {
    let Some(slot) = Slot::take_push(&user, MAX_CONNECTIONS) else {
        return (
            StatusCode::TOO_MANY_REQUESTS,
            "too many push connections for this account",
        )
            .into_response();
    };
    let bus = crate::handlers::events::get_or_init_bus(state.clone())
        .await
        .subscribe();
    let ctx = Ctx::new(state, user, None);
    let mut check = tokio::time::interval(CHECK);
    check.set_missed_tick_behavior(MissedTickBehavior::Skip);
    // RFC 8620 §7.3: 0 is no pings; the server may raise a small interval
    let ping = params.ping.filter(|p| *p > 0).map(|p| {
        let secs = p.clamp(5, 300);
        let mut tick = tokio::time::interval(Duration::from_secs(secs));
        tick.set_missed_tick_behavior(MissedTickBehavior::Skip);
        tick.reset();
        (tick, secs)
    });
    let conn = Conn {
        ctx,
        types: types_of(params.types.as_deref()),
        close_after_state: params.closeafter.as_deref() == Some("state"),
        bus,
        check,
        ping,
        last: None,
        done: false,
        _slot: slot,
    };
    let stream = futures_util::stream::unfold(conn, |mut c| async move {
        let event = c.next().await?;
        Some((Ok::<_, std::convert::Infallible>(event), c))
    });
    Sse::new(stream)
        .keep_alive(KeepAlive::new().interval(Duration::from_secs(15)))
        .into_response()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn types_default_to_all_and_unknown_ones_are_dropped() {
        assert_eq!(types_of(None), TYPES);
        assert_eq!(types_of(Some("*")), TYPES);
        assert_eq!(types_of(Some("Email,Foo")), ["Email"]);
        assert!(types_of(Some("Foo")).is_empty());
    }
}

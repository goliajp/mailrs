//! Server bootstrap helpers — each submodule owns one cohesive
//! piece of `async fn main`'s startup orchestration.
//!
//! Re-exports every helper at module level so callers in `main.rs`
//! can `use crate::bootstrap::*;` and call them by short name.

mod auth_guard;
mod cache;
mod inbound;
mod listeners;
mod outbound;
mod runtime_tasks;
mod system_config;
mod tls;
mod web_state;

pub(crate) use auth_guard::*;
pub(crate) use cache::*;
pub(crate) use inbound::*;
pub(crate) use listeners::*;
pub(crate) use outbound::*;
pub(crate) use runtime_tasks::*;
pub(crate) use system_config::*;
pub(crate) use tls::*;
pub(crate) use web_state::*;

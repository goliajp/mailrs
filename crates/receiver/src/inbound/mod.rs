//! Inbound receiving pipeline + anti subsystems, owned by the receiver
//! crate (S5.3). The kevy-network adapters (`kevy_backends`) stay in the
//! server crate: they bridge these trait surfaces to the network kevy
//! client, which is server-side infrastructure.

pub mod auth_guard;
pub mod content_scan;
pub mod pipeline;
pub mod rate_limit;
pub mod stages;

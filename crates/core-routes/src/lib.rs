//! The core-api routes both cores serve, written once over the storage
//! traits in [`store`]. Each core implements the traits for its own state
//! (kevy for fastcore, SQL for the SQL core) and mounts these handlers
//! instantiated with that state, so the business rules — who may act,
//! what an act tells the ranker, what the answer looks like — exist once.

pub mod message;
pub mod store;
pub mod thread;

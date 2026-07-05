//! One module per side-state family; every handler is generic over
//! `S: crate::NetKevy` so both cores mount the same code.

pub mod admin_state;
pub mod analysis;
pub mod contacts;
pub mod groups_admin;
pub mod outbound;
pub mod prefs;

#![doc = include_str!("../README.md")]

pub mod decide;
pub mod fingerprint;
pub mod template;

pub use decide::{Decision, Ground, Situation, Wait, decide};
pub use template::{MAX_DISTANCE, Shared, Template, same};

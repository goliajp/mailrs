//! Disposable test mailboxes.
//!
//! A namespace is created by a user and owns every address of the form
//! `{namespace}.{tag}@{mock domain}`; the tag needs no registration. Mail
//! to these addresses is kept apart from real mailboxes and is read back
//! through [`parsed::ParsedMessage`], which carries the one-time codes and
//! links a test usually wants.

pub mod address;
pub mod extract;
pub mod parsed;
pub mod simulation;
#[cfg(feature = "schema")]
pub mod tool_params;

pub use address::{MockAddress, NameError, is_mock_domain, parse_address, validate_namespace};
pub use parsed::{Attachment, Link, ParsedMessage, parse_message};
pub use simulation::Simulation;

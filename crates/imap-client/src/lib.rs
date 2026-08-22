#![deny(missing_docs)]
#![doc = include_str!("../README.md")]

mod parse;
mod plan;

#[cfg(feature = "net")]
mod net;
#[cfg(feature = "net")]
pub use net::{Error as NetError, Session, Tls};

pub use parse::{Fetch, List, Untagged, is_authentication_failure, parse_line};
pub use plan::{FetchPlan, FolderState, plan_fetch};

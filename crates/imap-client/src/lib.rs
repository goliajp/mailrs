#![deny(missing_docs)]
#![doc = include_str!("../README.md")]

mod parse;
mod plan;

pub use parse::{Fetch, List, Untagged, is_authentication_failure, parse_line};
pub use plan::{FetchPlan, FolderState, plan_fetch};

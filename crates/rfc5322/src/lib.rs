#![doc = include_str!("../README.md")]
#![deny(missing_docs)]
#![deny(rustdoc::broken_intra_doc_links)]

//! Internal layout: a single `Message<'a>` borrows the message bytes.
//! Header lookup is a forward scan of the header region; body access
//! locates and caches the offset of the empty line that terminates the
//! header section.

mod header;
mod message;

pub use header::{Header, HeaderIter};
pub use message::Message;

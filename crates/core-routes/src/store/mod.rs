//! What the shared routes need of a store. Every method is async: the SQL
//! store awaits its database, the kevy store answers from memory inline.

mod check;
mod message;
mod thread;

pub use check::{check_message_reads, check_thread_store};
pub use message::MessageReads;
pub use thread::{ReaderView, ThreadStore};

/// A store that could not do what it was asked. The route logs it and
/// answers 500; what was asked of a thing that is not there is not an
/// error, and each method says how it reports that.
#[derive(Debug)]
pub struct StoreError(pub String);

impl std::fmt::Display for StoreError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for StoreError {}

pub type StoreResult<T> = Result<T, StoreError>;

impl From<std::io::Error> for StoreError {
    fn from(e: std::io::Error) -> Self {
        StoreError(e.to_string())
    }
}

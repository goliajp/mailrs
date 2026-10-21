//! The SMTP receiving session lives in the `mailrs-receiver` crate;
//! re-exported here so the listeners and the test support reach it through
//! this crate.

pub use mailrs_receiver::smtp_session::*;

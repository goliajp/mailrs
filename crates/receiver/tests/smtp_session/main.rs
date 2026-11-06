//! Whole SMTP conversations against `handle_plain_connection`.
//!
//! Guessing local addresses at RCPT: past the limit a sender gets `421`
//! for every local address, existing or not, and is disconnected; who
//! counts as one sender is the account, the IP with an SPF-passing MAIL
//! FROM domain, or the IP. Submitting from a masked address: the message
//! names neither the client's IP nor its EHLO name, and the masks' own
//! submission log records the account and the IP.

mod forged_results;
mod harness;
mod masked;
mod parse_failed;
mod probing;
mod rar_and_7z;
mod signout;

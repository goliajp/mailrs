//! The Maildir + embedded-kevy side of a mailbox as IMAP numbers it:
//! folders, persistent UIDs, modseqs and the vanished log. The protocol
//! sessions are `mailrs-mail-protocols`, run over this by
//! [`crate::protocols`].

pub mod backend;

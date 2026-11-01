//! The words a masked address is made of: `<adjective>.<noun>.<6 digits>`.
//!
//! Reviewed by hand, in `adjectives.txt` and `nouns.txt`. Nothing there names
//! a role, a department or anything an organisation would answer mail at
//! (admin, billing, support, security, …), so a mask cannot pass as an
//! official address of its domain, and nothing is offensive. A test keeps a
//! deny list beside them.

use std::sync::LazyLock;

fn words(text: &'static str) -> Vec<&'static str> {
    text.split_whitespace().collect()
}

pub(super) static ADJECTIVES: LazyLock<Vec<&'static str>> =
    LazyLock::new(|| words(include_str!("adjectives.txt")));

pub(super) static NOUNS: LazyLock<Vec<&'static str>> =
    LazyLock::new(|| words(include_str!("nouns.txt")));

//! The addr-spec inside an RFC 5322 mailbox, and its comparison key.
//!
//! A mailbox (RFC 5322 §3.4) is written either as a bare addr-spec
//! (`a@b.com`) or as a name-addr (`Name <a@b.com>`). Code that needs to
//! know *which address* a header names has to reduce one to the other, and
//! in this tree eight places had grown their own version of that reduction:
//! `core-sidestate/families/suppression.rs`, `server/web/mail/common.rs`,
//! `server/smtp_session/post_delivery.rs`, `mail-builder/builder.rs`,
//! `fastcore/sieve_apply.rs` (twice), `fastcore/importance.rs`, and
//! `webapi/handlers/sends.rs`. A ninth was about to be written for
//! `senders_csv_contains_user`, which decides Sent-folder membership and
//! until now matched by **substring** — so `a@b.com` matched `xa@b.com`.
//!
//! Extraction and comparison are separate functions on purpose. They are
//! not the same operation, and the sites that conflated them are where the
//! bugs were: a display path wants the address as written, an equality
//! path wants a folded key. One function returning a lowercased string for
//! both would quietly corrupt the display of a mixed-case local part.

/// The addr-spec inside a mailbox, as written.
///
/// `Name <a@b.com>` → `a@b.com`. A bare `a@b.com` → `a@b.com`. Surrounding
/// whitespace is trimmed; nothing else is altered, so this is safe to
/// display.
///
/// The **last** `<` wins, because a display name may legitimately contain
/// one (`"a < b" <x@y>`), and an unterminated or empty bracket pair leaves
/// the input alone rather than returning something shorter than the truth.
///
/// ```
/// use mailrs_rfc5322::addr_spec;
/// assert_eq!(addr_spec("GOLIA <lihao@golia.jp>"), "lihao@golia.jp");
/// assert_eq!(addr_spec("  lihao@golia.jp "), "lihao@golia.jp");
/// assert_eq!(addr_spec("Mixed.Case@Example.COM"), "Mixed.Case@Example.COM");
/// ```
pub fn addr_spec(mailbox: &str) -> &str {
    let s = mailbox.trim();
    match (s.rfind('<'), s.rfind('>')) {
        (Some(open), Some(close)) if close > open + 1 => s[open + 1..close].trim(),
        _ => s,
    }
}

/// The comparison key for a mailbox: its addr-spec, case-folded.
///
/// Use this and only this to decide whether two headers name the same
/// mailbox. Do not display it.
///
/// The whole address is folded, local part included. RFC 5321 §2.4 reserves
/// local-part case-sensitivity to the receiving host, so `A@x` and `a@x`
/// may in principle differ — but every store in this tree keys accounts
/// case-insensitively, and treating them as two users would be a worse
/// failure than the deviation. Stated here so the choice is visible rather
/// than assumed.
///
/// ```
/// use mailrs_rfc5322::addr_key;
/// assert_eq!(addr_key("GOLIA <LiHao@Golia.JP>"), addr_key("lihao@golia.jp"));
/// // Not a substring match: this is the pair that put foreign threads in
/// // one account's Sent folder.
/// assert_ne!(addr_key("a@b.com"), addr_key("xa@b.com"));
/// ```
pub fn addr_key(mailbox: &str) -> String {
    addr_spec(mailbox).to_lowercase()
}

/// Whether any mailbox in a comma-separated list is `wanted`.
///
/// The list form headers use (`To`, `Cc`, and this tree's stored
/// `senders_csv`) with each element compared by [`addr_key`], so a longer
/// address that merely ends with the wanted one does not match.
///
/// ```
/// use mailrs_rfc5322::list_contains;
/// assert!(list_contains("GOLIA <a@b.com>, c@d.com", "a@b.com"));
/// assert!(!list_contains("xa@b.com", "a@b.com"));
/// ```
pub fn list_contains(list: &str, wanted: &str) -> bool {
    let key = addr_key(wanted);
    list.split(',').any(|m| addr_key(m) == key)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The defect this replaces: `senders_csv_contains_user` used
    /// `contains`, so any address ending with the user's put the thread in
    /// their Sent folder.
    #[test]
    fn a_longer_address_ending_with_the_wanted_one_does_not_match() {
        assert!(!list_contains("xa@b.com", "a@b.com"));
        assert!(!list_contains(
            "Someone <notlihao@golia.jp>",
            "lihao@golia.jp"
        ));
        assert!(!list_contains("a@b.com.evil.example", "a@b.com"));
    }

    /// Both mailbox forms name the same address, and prod stores both: a
    /// captured `to_csv` holds `GOLIA <goliaaccess@gmail.com>` while the
    /// queued recipient list can hold the bare form.
    #[test]
    fn the_two_mailbox_forms_agree() {
        assert_eq!(
            addr_key("GOLIA <goliaaccess@gmail.com>"),
            addr_key("goliaaccess@gmail.com")
        );
        assert!(list_contains(
            "GOLIA <goliaaccess@gmail.com>",
            "goliaaccess@gmail.com"
        ));
        assert!(list_contains(
            "goliaaccess@gmail.com",
            "GOLIA <goliaaccess@gmail.com>"
        ));
    }

    #[test]
    fn case_folds_for_comparison_and_not_for_display() {
        assert_eq!(addr_key("A@B.com"), "a@b.com");
        // Display form is untouched, which is why these are two functions.
        assert_eq!(addr_spec("Name <A@B.com>"), "A@B.com");
    }

    /// A display name may contain `<`, so the last one opens the address.
    #[test]
    fn a_bracket_in_the_display_name_does_not_confuse_it() {
        assert_eq!(addr_spec("\"a < b\" <x@y.com>"), "x@y.com");
    }

    /// Malformed input is returned as-is rather than truncated to
    /// something shorter than the truth — a wrong-but-plausible address is
    /// worse than an unparsed one.
    #[test]
    fn malformed_brackets_leave_the_input_alone() {
        assert_eq!(
            addr_spec("Name <unterminated@x.com"),
            "Name <unterminated@x.com"
        );
        assert_eq!(addr_spec("<>"), "<>");
        assert_eq!(addr_spec(""), "");
    }

    #[test]
    fn list_handles_spacing_and_empty_elements() {
        assert!(list_contains("a@x.com , b@x.com ,, c@x.com", "b@x.com"));
        assert!(!list_contains("", "a@x.com"));
    }
}

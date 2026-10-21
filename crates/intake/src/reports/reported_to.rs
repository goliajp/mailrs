/// Which address the report hooks are asked about.
///
/// **What the sender addressed, not where it will be written.** For an
/// account the two are the same string; for an **alias** the delivery
/// target is somebody's personal mailbox, and asking about that loses
/// the report — `is_fbl_mailbox("lihao@golia.jp")` is false however the
/// mail got there.
///
/// It worked for DMARC only because `dmarc@golia.jp` and
/// `tlsrpt@golia.jp` happen to be real accounts with their own
/// maildirs. Nothing recorded that this was load-bearing, and the first
/// feedback loop registered against an alias — the ordinary way to do
/// it — would have gone unprocessed with no error anywhere.
pub fn reported_to<'a>(envelope_rcpt: &'a str, _delivery_target: &'a str) -> &'a str {
    envelope_rcpt
}

#[cfg(test)]
mod addressed_to_tests {
    /// **These hooks ask who the mail was addressed to, not whose
    /// mailbox it landed in.**
    ///
    /// `resolved_addr` is the delivery target: for an account it is the
    /// recipient unchanged, but for an **alias** it is the alias's
    /// target. The FBL, DMARC and TLS-RPT hooks were keyed on it, so a
    /// complaint sent to `abuse@golia.ai` — an alias onto a person's
    /// mailbox — arrived in that mailbox and was silently not
    /// processed: `is_fbl_mailbox("lihao@golia.jp")` is false.
    ///
    /// It happened to work for DMARC only because `dmarc@golia.jp` and
    /// `tlsrpt@golia.jp` were created as real accounts with their own
    /// maildirs. Nothing said that was load-bearing, and the moment an
    /// operator registers a feedback loop against an alias — which is
    /// the ordinary thing to do, and what
    /// `sendersupport.olc.protection.outlook.com` was about to be given
    /// on 2026-08-17 — the reports stop being processed with no error
    /// anywhere.
    ///
    /// The envelope recipient is the right key, and it costs nothing:
    /// for an account the two are the same string.
    #[test]
    fn a_complaint_to_an_aliased_abuse_box_is_still_a_complaint() {
        // The shape the spool drain has: what the sender addressed, and
        // where it will be written.
        let addressed_to = "abuse@golia.ai";
        let delivered_to = "lihao@golia.jp";

        assert!(
            crate::reports::fbl::is_fbl_mailbox_for_test(addressed_to),
            "the address the complaint was sent to is an FBL mailbox"
        );
        assert!(
            !crate::reports::fbl::is_fbl_mailbox_for_test(delivered_to),
            "premise: the mailbox it lands in is not one, which is why \
             keying on the delivery target loses the report"
        );

        // And the rule the call sites go through picks the first.
        assert!(
            crate::reports::fbl::is_fbl_mailbox_for_test(super::reported_to(
                addressed_to,
                delivered_to
            )),
            "the hooks are being asked about the delivery target again"
        );
    }

    /// For a real account the two are the same string, so nothing about
    /// `dmarc@` / `tlsrpt@` changes — which is the check that this is a
    /// widening and not a swap.
    #[test]
    fn an_account_is_unaffected() {
        assert_eq!(
            super::reported_to("dmarc@golia.jp", "dmarc@golia.jp"),
            "dmarc@golia.jp"
        );
    }
}

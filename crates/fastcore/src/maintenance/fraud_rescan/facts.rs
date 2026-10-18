//! What the sweep reads off one stored message for the rules.
//!
//! The same facts the receive path assembles, from the same extractors.
//! Two assemblies of one message is how a folded header came to be
//! visible on one path and not the other.

/// Everything read off one message, owned, so [`Reading::facts`] can lend
/// it to the rules.
pub(super) struct Reading {
    pub(super) from: String,
    pub(super) subject: String,
    pub(super) host: String,
    pub(super) reply_to: String,
    registrable: String,
    x_mailer: Option<String>,
    to_display: String,
    auth: (String, String, String),
    has_zero_width: bool,
    has_bidi_override: bool,
    has_zero_width_inside_a_word: bool,
    has_zero_width_in_name: bool,
    has_executable_attachment: bool,
    offers_the_reader_a_sum: bool,
    is_bulk: bool,
    date_ahead: i64,
    unauthenticated: bool,
    peer_is_private: bool,
    attachment_text: String,
}

impl Reading {
    /// Read `raw`. `attachment_texts` is what extraction stored for this
    /// message, empty when it has none or has not been processed.
    pub(super) fn of(raw: &[u8], our_stamp: Option<&str>, attachment_texts: &[String]) -> Self {
        let from = mailrs_inbound::from_header(raw);
        let host = from
            .rfind('@')
            .map(|at| from[at + 1..].trim_end_matches('>').trim().to_string())
            .unwrap_or_default();
        let parsed = mailrs_mime::parse(raw);
        // The display name has to be read the way it renders, or the
        // sweep cannot see one that says something else.
        let deception = mailrs_inbound::deception_in_identity(raw);
        let attachments: Vec<&str> = parsed
            .attachments()
            .filter_map(|p| p.attachment_filename())
            .collect();
        Self {
            registrable: mailrs_fraud::brand::registrable(&host),
            x_mailer: mailrs_inbound::x_mailer_header(raw),
            has_zero_width: deception.unjustified_zero_width,
            has_bidi_override: deception.bidi_override,
            has_zero_width_inside_a_word: deception.zero_width_inside_a_word,
            has_zero_width_in_name: mailrs_inbound::deception_in_display_name(raw)
                .unjustified_zero_width,
            has_executable_attachment: mailrs_fraud::attachment::any_executable(
                attachments.iter().copied(),
            ),
            offers_the_reader_a_sum: mailrs_inbound::body::offers_the_reader_a_sum(&parsed),
            is_bulk: mailrs_inbound::body::is_bulk(raw),
            auth: mailrs_inbound::identity::auth_results_tokens(raw),
            subject: mailrs_inbound::subject_header(raw),
            to_display: mailrs_inbound::identity::to_display_name(raw),
            reply_to: mailrs_inbound::identity::reply_to_address(raw),
            // against the arrival this deployment stamped, not the
            // clock: once the claimed date has passed, the clock would
            // release what the live path held
            date_ahead: mailrs_inbound::dates::date_ahead_of(
                raw,
                mailrs_inbound::dates::received_epoch(raw),
            ),
            // How it was submitted, read back off the stamp the receiver
            // writes on the unauthenticated inbound path only. Without
            // `MAILRS_HOSTNAME` this process cannot tell its own stamp
            // from a forwarder's, so it says "not known" and the rules
            // that need it decline.
            unauthenticated: our_stamp.is_some_and(|host| {
                mailrs_inbound::identity::auth_results_authserv(raw).as_deref() == Some(host)
            }),
            // A message with no `Received:` line to read is treated as
            // arriving from outside, the conservative direction for the
            // only rule that asks.
            peer_is_private: mailrs_inbound::identity::first_received_peer_ip(raw)
                .is_some_and(mailrs_inbound::identity::address_is_private),
            attachment_text: mailrs_inbound::body::attachment_text_for_rules(attachment_texts),
            from,
            host,
        }
    }

    /// The facts, with the two that come from the history store.
    pub(super) fn facts(&self, domain_seen: u64, reply_rotation: u32) -> mailrs_fraud::Facts<'_> {
        mailrs_fraud::Facts {
            from: &self.from,
            subject: &self.subject,
            domain: &self.host,
            registrable: &self.registrable,
            domain_seen,
            x_mailer: self.x_mailer.as_deref(),
            has_zero_width: self.has_zero_width,
            has_bidi_override: self.has_bidi_override,
            has_zero_width_in_name: self.has_zero_width_in_name,
            has_executable_attachment: self.has_executable_attachment,
            has_zero_width_inside_a_word: self.has_zero_width_inside_a_word,
            offers_the_reader_a_sum: self.offers_the_reader_a_sum,
            is_bulk: self.is_bulk,
            to_display: &self.to_display,
            reply_rotation,
            date_ahead: self.date_ahead,
            unauthenticated: self.unauthenticated,
            peer_is_private: self.peer_is_private,
            spf: &self.auth.0,
            dkim: &self.auth.1,
            dmarc: &self.auth.2,
            attachment_text: &self.attachment_text,
            attachment_offers_the_reader_a_sum:
                mailrs_inbound::body::attachment_offers_the_reader_a_sum(&self.attachment_text),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const RAW: &[u8] = b"From: Konczol <info@nexforce.example>\r\n\
Subject: contract\r\n\
\r\n\
please see the attached contract\r\n";

    #[test]
    fn stored_attachment_text_reaches_both_facts() {
        let texts = vec!["in the sum of US$10,200,000.00, split equally, 50% apiece".to_string()];
        let reading = Reading::of(RAW, None, &texts);
        let facts = reading.facts(0, 0);
        assert!(facts.attachment_text.contains("apiece"));
        assert!(facts.attachment_offers_the_reader_a_sum);
        assert!(!facts.offers_the_reader_a_sum, "the body offers nothing");
    }

    #[test]
    fn without_stored_text_both_are_empty() {
        let reading = Reading::of(RAW, None, &[]);
        let facts = reading.facts(0, 0);
        assert_eq!(facts.attachment_text, "");
        assert!(!facts.attachment_offers_the_reader_a_sum);
    }
}

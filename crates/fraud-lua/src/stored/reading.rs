//! What is read off one stored message for the rules.
//!
//! The same facts the receive path assembles, from the same extractors.
//! Two assemblies of one message is how a folded header came to be
//! visible on one path and not the other.

/// Everything read off one message, owned, so [`Reading::facts`] can lend
/// it to the rules.
pub struct Reading {
    /// `From:`, decoded.
    pub from: String,
    /// `Subject:`, decoded.
    pub subject: String,
    /// The host of the `From:` address.
    pub host: String,
    /// The `Reply-To:` address.
    pub reply_to: String,
    /// The hosts the message's links go to.
    pub link_hosts: Vec<String>,
    /// When this deployment received it, from its own stamp.
    pub arrived: Option<i64>,
    /// The hosts the message's QR codes go to.
    pub qr_hosts: Vec<String>,
    registrable: String,
    x_mailer: Option<String>,
    to_display: String,
    auth: (String, String, String),
    has_zero_width: bool,
    has_bidi_override: bool,
    has_zero_width_inside_a_word: bool,
    has_zero_width_in_name: bool,
    inside: mailrs_parse_worker::AttachmentsRead,
    reply_marker_without_thread: bool,
    offers_the_reader_a_sum: bool,
    asks_for_the_readers_line: bool,
    shown_links: String,
    link_has_kit_placeholder: bool,
    is_bulk: bool,
    hosted_mailbox: bool,
    date_ahead: i64,
    unauthenticated: bool,
    peer_is_private: bool,
    attachment_text: String,
    payment: mailrs_fraud::payment::PaymentIds,
    html: mailrs_fraud::html_attachment::HtmlFeatures,
    dkim_signed_by_from_org: bool,
    dkim_from_org_passed: Option<bool>,
    has_links: bool,
    bucket_hosts: String,
    qr: mailrs_fraud::qr::QrFacts,
    qr_domains: String,
}

/// What extraction stored for a message after delivery: the text read out
/// of its attachments, and the web addresses its QR codes carry. Empty
/// where nothing was read, and on a core that does not read images (no QR
/// addresses).
#[derive(Debug, Clone, Copy, Default)]
pub struct Extracted<'a> {
    /// The text read out of each attachment.
    pub texts: &'a [String],
    /// The web addresses its QR codes carry.
    pub qr_urls: &'a [String],
}

/// Shared platforms, which a QR code to is never out of place.
static NEVER: std::sync::LazyLock<mailrs_reputation::NeverList> =
    std::sync::LazyLock::new(|| mailrs_reputation::NeverList::builtin(&[]));

impl Reading {
    /// Read `raw` with everything extraction stored for it.
    pub fn of_extracted(raw: &[u8], our_stamp: Option<&str>, extracted: Extracted<'_>) -> Self {
        Self::of(raw, our_stamp, extracted.texts).with_qr_urls(extracted.qr_urls)
    }

    /// The QR facts of the codes whose addresses are `urls`, against this
    /// message's sender, its links and its attachments' text.
    #[must_use]
    pub fn with_qr_urls(mut self, urls: &[String]) -> Self {
        self.qr = mailrs_fraud::qr::assess(
            urls,
            &self.host,
            &self.link_hosts,
            &self.attachment_text,
            |d| NEVER.contains(d),
        );
        self.qr_hosts = mailrs_fraud::qr::code_hosts(urls);
        self.qr_domains = mailrs_fraud::qr::code_domains(urls).join("\n");
        self
    }

    /// Read `raw`. `attachment_texts` is what extraction stored for this
    /// message, empty when it has none or has not been processed.
    pub fn of(raw: &[u8], our_stamp: Option<&str>, attachment_texts: &[String]) -> Self {
        let from = mailrs_inbound::from_header(raw);
        let host = from
            .rfind('@')
            .map(|at| from[at + 1..].trim_end_matches('>').trim().to_string())
            .unwrap_or_default();
        let parsed = mailrs_mime::parse(raw);
        // The display name has to be read the way it renders, or the
        // sweep cannot see one that says something else.
        let deception = mailrs_inbound::deception_in_identity(raw);
        // what the receiver found, only from the header it wrote; without
        // `MAILRS_HOSTNAME` nothing reads as its, and the rules that need
        // it decline
        let stamp = super::stamp::read(raw, our_stamp, &host);
        let attachment_text = mailrs_inbound::body::attachment_text_for_rules(attachment_texts);
        let link_hosts = mailrs_inbound::scan_input::link_hosts(&parsed);
        Self {
            has_links: !link_hosts.is_empty(),
            bucket_hosts: mailrs_fraud::storage_bucket::bucket_hosts(&link_hosts),
            link_hosts,
            qr_hosts: Vec::new(),
            qr: Default::default(),
            qr_domains: String::new(),
            arrived: mailrs_inbound::dates::received_epoch(raw),
            registrable: mailrs_fraud::brand::registrable(&host),
            x_mailer: mailrs_inbound::x_mailer_header(raw),
            has_zero_width: deception.unjustified_zero_width,
            has_bidi_override: deception.bidi_override,
            has_zero_width_inside_a_word: deception.zero_width_inside_a_word,
            has_zero_width_in_name: mailrs_inbound::deception_in_display_name(raw)
                .unjustified_zero_width,
            // nobody waits on a re-judging; a large message may take
            // the longer limit
            inside: mailrs_parse_worker::attachments_inside(raw, mailrs_parse_worker::Limit::Batch),
            reply_marker_without_thread: mailrs_inbound::thread_claim::reply_marker_without_thread(
                raw,
            ),
            offers_the_reader_a_sum: mailrs_inbound::body::offers_the_reader_a_sum(&parsed),
            asks_for_the_readers_line: mailrs_inbound::body::asks_for_the_readers_line(&parsed),
            shown_links: mailrs_inbound::body::shown_links(&parsed),
            link_has_kit_placeholder: mailrs_inbound::body::link_has_kit_placeholder(&parsed),
            is_bulk: mailrs_inbound::body::is_bulk(raw),
            hosted_mailbox: mailrs_fraud::burner_domain::hosted_mailbox(raw),
            auth: stamp.auth,
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
            // the receiver stamps the unauthenticated inbound path only
            unauthenticated: stamp.unauthenticated,
            // A message with no `Received:` line to read is treated as
            // arriving from outside, the conservative direction for the
            // only rule that asks.
            peer_is_private: mailrs_inbound::identity::first_received_peer_ip(raw)
                .is_some_and(mailrs_inbound::identity::address_is_private),
            payment: mailrs_inbound::body::payment_ids(&parsed, &attachment_text),
            attachment_text,
            html: mailrs_inbound::body::html_attachments(&parsed, &host),
            dkim_signed_by_from_org: mailrs_fraud::org_domain::signed_only_by(
                &host,
                &mailrs_inbound::dkim_signers::of(raw),
            ),
            dkim_from_org_passed: stamp.dkim_from_org_passed,
            from,
            host,
        }
    }

    /// The keys of the payment accounts in the message's text and its
    /// attachments' text, for the per-sender account history.
    pub fn account_keys(&self) -> &[String] {
        &self.payment.account_keys
    }

    /// The facts, with the three that come from the history store.
    pub fn facts(
        &self,
        domain_seen: u64,
        reply_rotation: u32,
        relation: mailrs_fraud::relation::Relation,
    ) -> mailrs_fraud::Facts<'_> {
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
            has_executable_attachment: self.inside.inside.carries_code,
            has_encrypted_document_archive: self.inside.inside.encrypted_document_archive,
            parser_failed_on_message: self.inside.parser_failed,
            has_unreadable_archive: self.inside.inside.unreadable_archive,
            reply_marker_without_thread: self.reply_marker_without_thread,
            has_zero_width_inside_a_word: self.has_zero_width_inside_a_word,
            offers_the_reader_a_sum: self.offers_the_reader_a_sum,
            asks_for_the_readers_line: self.asks_for_the_readers_line,
            shown_links: &self.shown_links,
            link_has_kit_placeholder: self.link_has_kit_placeholder,
            is_bulk: self.is_bulk,
            hosted_mailbox: self.hosted_mailbox,
            // from the store, by the caller that has one: [`super::site_elsewhere`]
            site_elsewhere: false,
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
            payment: self.payment.clone(),
            html: self.html,
            relation,
            novelty: Default::default(),
            reply_to_outside: mailrs_fraud::reply_rotation::is_off_domain(
                &self.host,
                &self.reply_to,
            ),
            reply_to_other_org: mailrs_fraud::org_domain::reply_elsewhere(
                &self.host,
                &self.reply_to,
            ),
            dkim_signed_by_from_org: self.dkim_signed_by_from_org,
            dkim_from_org_passed: self.dkim_from_org_passed,
            has_links: self.has_links,
            links_only_to_random_buckets: !self.bucket_hosts.is_empty(),
            bucket_hosts: &self.bucket_hosts,
            link_hosts: &self.link_hosts,
            qr: self.qr,
            qr_domains: &self.qr_domains,
            new_qr_domain: None,
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
        let facts = reading.facts(0, 0, Default::default());
        assert!(facts.attachment_text.contains("apiece"));
        assert!(facts.attachment_offers_the_reader_a_sum);
        assert!(!facts.offers_the_reader_a_sum, "the body offers nothing");
    }

    #[test]
    fn without_stored_text_both_are_empty() {
        let reading = Reading::of(RAW, None, &[]);
        let facts = reading.facts(0, 0, Default::default());
        assert_eq!(facts.attachment_text, "");
        assert!(!facts.attachment_offers_the_reader_a_sum);
    }
}

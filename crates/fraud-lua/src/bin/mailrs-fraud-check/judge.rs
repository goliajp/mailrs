//! One message read the way the production sweep reads it, and judged
//! by both bundles.
use super::{month_of, policy, reader_of, reply_key};
use mailrs_fraud::{Facts, Findings, Policy};
use mailrs_fraud_lua::Rules;
use std::{
    collections::HashMap,
    path::{Path, PathBuf},
};

/// What the comparison reads besides the two bundles: the policy, and
/// the history built from the corpus and the shared store.
pub(super) struct Corpus<'r> {
    root: &'r str,
    pub(super) policy: Policy,
    domain_counts: HashMap<String, u64>,
    replies: HashMap<String, std::collections::HashSet<String>>,
    our_stamp: Option<String>,
    hist: Option<kevy_client::Connection>,
    /// each reader's not-junk list, read once; `None` when it could not
    /// be read, which the sweep counts as naming the sender
    whitelists: HashMap<String, Option<std::collections::HashSet<String>>>,
}

impl Corpus<'_> {
    /// Whether the readers' not-junk lists are read at all.
    pub(super) fn whitelist_checked(&self) -> bool {
        self.hist.is_some()
    }

    /// Whether `reader` marked this sender not junk, as the re-judge
    /// sweep reads it: no store means no list, and a list that cannot be
    /// read names the sender.
    fn whitelisted(&mut self, reader: &str, raw: &[u8]) -> bool {
        let Some(conn) = self.hist.as_mut() else {
            return false;
        };
        let list = self
            .whitelists
            .entry(reader.to_string())
            .or_insert_with(|| {
                mailrs_core_sidestate::families::sender_lists::whitelist(conn, reader).ok()
            });
        match list {
            Some(list) => mailrs_inbound::sender_listed(
                list,
                "",
                &mailrs_inbound::identity::from_address(raw),
            ),
            None => true,
        }
    }
}

/// One message as both bundles judged it.
pub(super) struct Judged {
    pub(super) month: String,
    pub(super) from: String,
    /// the registrable domain of the `From:` address
    pub(super) domain: String,
    pub(super) whitelisted: bool,
    pub(super) subject: String,
    pub(super) old: Findings,
    pub(super) old_shadow: Findings,
    pub(super) new: Findings,
    pub(super) new_shadow: Findings,
}

/// Largest message judged; a larger one stops the run.
pub(super) const MAX_MESSAGE: u64 = 32 * 1024 * 1024;

pub(super) fn corpus<'r>(
    root: &'r str,
    files: &[PathBuf],
    maildir: &mut super::walk::Maildir,
) -> Result<Corpus<'r>, String> {
    let mut domain_counts = HashMap::<String, u64>::new();
    let mut replies = HashMap::<String, std::collections::HashSet<String>>::new();
    // History from this read-only corpus, supplied identically to both engines.
    for file in files {
        let Some((_, header)) = maildir.read(file, 64 * 1024)? else {
            continue;
        };
        let from = mailrs_inbound::from_header(&header);
        let host = mailrs_fraud::impersonation::address_of(&from)
            .and_then(|a| a.rsplit('@').next())
            .unwrap_or("");
        let reg = mailrs_fraud::brand::registrable(host);
        *domain_counts.entry(reg.clone()).or_default() += 1;
        if let Some(reply) = reply_key(host, &mailrs_inbound::identity::reply_to_address(&header)) {
            replies.entry(reply).or_default().insert(reg);
        }
    }
    Ok(Corpus {
        root,
        policy: policy()?,
        domain_counts,
        replies,
        our_stamp: std::env::var("MAILRS_HOSTNAME").ok(),
        // with the shared store: each reader's history with the sender (the
        // `bec-` rules) and when a domain was first seen, as the core's own
        // re-judge reads them
        hist: std::env::var("MAILRS_KEVY_URL")
            .ok()
            .and_then(|u| kevy_client::Connection::connect(&u).ok()),
        whitelists: HashMap::new(),
    })
}

pub(super) fn judge(
    c: &mut Corpus<'_>,
    before: &mut Option<Rules>,
    after: &mut Rules,
    file: &Path,
    raw: Vec<u8>,
) -> Result<Judged, String> {
    if raw.len() as u64 > MAX_MESSAGE {
        return Err(format!("message exceeds 32 MiB: {}", file.display()));
    }
    let from = mailrs_inbound::from_header(&raw);
    let host = mailrs_fraud::impersonation::address_of(&from)
        .and_then(|a| a.rsplit('@').next())
        .unwrap_or("");
    let reg = mailrs_fraud::brand::registrable(host);
    let subject = mailrs_inbound::subject_header(&raw);
    let to = mailrs_inbound::identity::to_display_name(&raw);
    let x = mailrs_inbound::x_mailer_header(&raw);
    let deception = mailrs_inbound::deception_in_identity(&raw);
    let name_deception = mailrs_inbound::deception_in_display_name(&raw);
    let reply = reply_key(host, &mailrs_inbound::identity::reply_to_address(&raw));
    let parsed = mailrs_mime::parse(&raw);
    // as the production sweep reads it: only from the header our receiver
    // wrote, and only MAILRS_HOSTNAME says which name is ours
    let stamp = mailrs_fraud_lua::stored::stamp::read(&raw, c.our_stamp.as_deref(), host);
    let (spf, dkim, dmarc) = stamp.auth;
    let peer_is_private = mailrs_inbound::identity::first_received_peer_ip(&raw)
        .is_some_and(mailrs_inbound::identity::address_is_private);
    let shown_links = mailrs_inbound::body::shown_links(&parsed);
    let reader = reader_of(Path::new(c.root), file);
    let relation = mailrs_fraud_lua::stored::relation::relation(
        c.hist.as_mut(),
        &reader,
        &from,
        &mailrs_inbound::identity::reply_to_address(&raw),
        mailrs_fraud_lua::stored::relation::Counted::After,
    );
    let arrived = mailrs_inbound::dates::received_epoch(&raw).unwrap_or(0);
    let novelty = mailrs_core_sidestate::families::domain_first::novelty(
        c.hist.as_mut(),
        host,
        &mailrs_inbound::scan_input::link_hosts(&parsed),
        arrived,
    );
    let read = mailrs_parse_worker::attachments_inside(&raw, mailrs_parse_worker::Limit::Batch);
    let inside = read.inside;
    let f = Facts {
        from: &from,
        domain: host,
        registrable: &reg,
        subject: &subject,
        to_display: &to,
        x_mailer: x.as_deref(),
        domain_seen: c
            .domain_counts
            .get(&reg)
            .copied()
            .unwrap_or(1)
            .saturating_sub(1),
        reply_rotation: reply
            .as_ref()
            .and_then(|r| c.replies.get(r))
            .map_or(0, |s| s.len() as u32),
        has_zero_width: deception.unjustified_zero_width,
        has_zero_width_in_name: name_deception.unjustified_zero_width,
        has_bidi_override: deception.bidi_override,
        has_zero_width_inside_a_word: deception.zero_width_inside_a_word,
        offers_the_reader_a_sum: mailrs_inbound::body::offers_the_reader_a_sum(&parsed),
        asks_for_the_readers_line: mailrs_inbound::body::asks_for_the_readers_line(&parsed),
        shown_links: &shown_links,
        link_has_kit_placeholder: mailrs_inbound::body::link_has_kit_placeholder(&parsed),
        is_bulk: mailrs_inbound::body::is_bulk(&raw),
        has_executable_attachment: inside.carries_code,
        has_encrypted_document_archive: inside.encrypted_document_archive,
        parser_failed_on_message: read.parser_failed,
        has_unreadable_archive: inside.unreadable_archive,
        reply_marker_without_thread: mailrs_inbound::thread_claim::reply_marker_without_thread(
            &raw,
        ),
        date_ahead: mailrs_inbound::dates::date_ahead_of(
            &raw,
            mailrs_inbound::dates::received_epoch(&raw),
        ),
        spf: &spf,
        dkim: &dkim,
        dmarc: &dmarc,
        unauthenticated: stamp.unauthenticated,
        peer_is_private,
        relation,
        novelty,
        payment: mailrs_inbound::body::payment_ids(&parsed, ""),
        html: mailrs_inbound::body::html_attachments(&parsed, host),
        reply_to_outside: mailrs_fraud::reply_rotation::is_off_domain(
            host,
            &mailrs_inbound::identity::reply_to_address(&raw),
        ),
        reply_to_other_org: mailrs_fraud::org_domain::reply_elsewhere(
            host,
            &mailrs_inbound::identity::reply_to_address(&raw),
        ),
        dkim_signed_by_from_org: mailrs_fraud::org_domain::signed_only_by(
            host,
            &mailrs_inbound::dkim_signers::of(&raw),
        ),
        dkim_from_org_passed: stamp.dkim_from_org_passed,
        has_links: !mailrs_inbound::scan_input::link_hosts(&parsed).is_empty(),
        ..Facts::default()
    };
    let (old, old_shadow) = match before {
        Some(r) => {
            let v = r.classify(&f, &c.policy);
            if !v.errors.is_empty() {
                return Err(format!("baseline: {:?}", v.errors));
            }
            (v.findings, v.shadow)
        }
        None => (mailrs_fraud::scan(&f, &c.policy), Findings::new()),
    };
    let new = after.classify(&f, &c.policy);
    if !new.errors.is_empty() {
        return Err(format!("candidate: {:?}", new.errors));
    }
    let whitelisted = c.whitelisted(&reader, &raw);
    Ok(Judged {
        month: month_of(&raw),
        from,
        domain: reg,
        whitelisted,
        subject,
        old,
        old_shadow,
        new: new.findings,
        new_shadow: new.shadow,
    })
}

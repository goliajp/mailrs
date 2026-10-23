//! What the pipeline and the fraud rules are told about one inbound
//! message: the receive context, the facts read from the message and
//! the sending domain's history, and the organisation's policy.

use std::net::SocketAddr;

use mailrs_inbound::ReceiveContext;
use mailrs_smtp_proto::session::State;

use super::super::super::super::ConnectionContext;

pub(super) async fn receive_context(
    session_state: &State,
    addr: SocketAddr,
    reverse_path: &str,
    first_rcpt: &str,
    full_message: Vec<u8>,
    ctx: &ConnectionContext,
) -> ReceiveContext {
    let ehlo_domain = match session_state {
        State::Greeted { domain } => domain.as_str(),
        State::Authenticated { domain, .. } => domain.as_str(),
        _ => "unknown",
    };
    // Move `full_message` into ReceiveContext (the pipeline owns it
    // during run()) and reclaim it with `mem::take` after — saves a
    // full-body clone on every inbound message. The pipeline does
    // not retain the body past `run().await` so the take is safe.
    let mut receive_ctx = mailrs_inbound::ReceiveContext::new(
        addr.ip(),
        ehlo_domain,
        reverse_path,
        first_rcpt,
        full_message,
        &ctx.hostname,
    );

    // v2.4.1 Phase 3 (RFC-B §3.3) — populate the per-user
    // whitelist / blacklist snapshot for the primary rcpt. Empty on
    // absent client, kevy error, or missing rcpt — all fail-open
    // and the pipeline falls back to the score-based path
    // (identical to pre-Phase-3 behavior). Also lowercase the
    // envelope MAIL FROM into `from_addr` so the lookup match is
    // case-insensitive.
    receive_ctx.from_addr = reverse_path.to_lowercase();
    if !first_rcpt.is_empty() {
        let (wl, bl) = crate::spam_lists::load_recipient_lists_async(
            ctx.spam_lists_client.clone(),
            first_rcpt,
        )
        .await;
        receive_ctx.recipient_whitelist = wl;
        receive_ctx.recipient_blacklist = bl;
    }
    // A colleague's message is not junk. Mail that really comes from
    // one of our own domains — proven by SPF or DKIM, never by the
    // `From:` header alone — takes the same Accept as a whitelisted
    // sender. Mail our own users *send* never reaches here: this whole
    // function runs only for unauthenticated sessions.
    receive_ctx.local_domains = ctx.runtime.local_domains.as_set();
    receive_ctx
}

/// The readings [`mailrs_fraud::Facts`] borrows from, owned here so
/// the alignment pass can reuse them after the pipeline has run.
pub(super) struct Evidence {
    decoded_from: String,
    sender_host: String,
    registrable: String,
    domain_seen: u64,
    x_mailer: Option<String>,
    has_zero_width_in_name: bool,
    attachment_names: Vec<String>,
    offers_the_reader_a_sum: bool,
    asks_for_the_readers_line: bool,
    subject: String,
    to_display: String,
    reply_rotation: u32,
}

impl Evidence {
    // The half of `mailrs_fraud` that needs to know what this
    // organisation is called. The other half — the mailer fingerprint —
    // is read from the message by `ReceiveContext::new`, because it
    // needs no configuration.
    //
    // Read from the **decoded** `From:`: the names arrive base64'd
    // inside `=?UTF-8?B?…?=` in every sample, and a check on the raw
    // header sees only ASCII.
    // The facts, assembled once, then one scan. This was three
    // separate assignments into three separate fields, each with its
    // own extraction — and the two that read the `From` read it
    // differently for a while. One `Facts`, one `scan`, and a new
    // rule needs neither a field nor a line here.
    pub(super) async fn gather(receive_ctx: &ReceiveContext, ctx: &ConnectionContext) -> Self {
        let decoded_from = mailrs_inbound::identity::from_header(&receive_ctx.message);
        let sender_host = decoded_from
            .rfind('@')
            .map(|at| {
                decoded_from[at + 1..]
                    .trim_end_matches('>')
                    .trim()
                    .to_string()
            })
            .unwrap_or_default();
        let registrable = mailrs_fraud::brand::registrable(&sender_host);
        let domain_seen =
            crate::spam_lists::domain_seen_async(ctx.spam_lists_client.clone(), &sender_host).await;
        let x_mailer = mailrs_inbound::identity::x_mailer_header(&receive_ctx.message);
        let name_deception = mailrs_inbound::deception_in_display_name(&receive_ctx.message);
        let parsed = mailrs_mime::parse(&receive_ctx.message);
        let attachment_names: Vec<String> = parsed
            .attachments()
            .filter_map(|p| p.attachment_filename())
            .map(|f| f.to_string())
            .collect();
        let offers_the_reader_a_sum = mailrs_inbound::body::offers_the_reader_a_sum(&parsed);
        let asks_for_the_readers_line = mailrs_inbound::body::asks_for_the_readers_line(&parsed);
        let subject = mailrs_inbound::subject_header(&receive_ctx.message);
        let to_display = mailrs_inbound::identity::to_display_name(&receive_ctx.message);
        let reply_to = mailrs_inbound::identity::reply_to_address(&receive_ctx.message);
        // Records this sending domain against the reply address, then
        // counts — so the arrival that reaches the threshold convicts
        // itself and not only the ones after it.
        let reply_rotation = crate::spam_lists::reply_rotation_async(
            ctx.spam_lists_client.clone(),
            &sender_host,
            &reply_to,
        )
        .await;
        Self {
            decoded_from,
            sender_host,
            registrable,
            domain_seen,
            x_mailer,
            has_zero_width_in_name: name_deception.unjustified_zero_width,
            attachment_names,
            offers_the_reader_a_sum,
            asks_for_the_readers_line,
            subject,
            to_display,
            reply_rotation,
        }
    }

    pub(super) fn facts<'a>(
        &'a self,
        receive_ctx: &ReceiveContext,
        addr: SocketAddr,
    ) -> mailrs_fraud::Facts<'a> {
        mailrs_fraud::Facts {
            from: &self.decoded_from,
            subject: &self.subject,
            domain: &self.sender_host,
            registrable: &self.registrable,
            domain_seen: self.domain_seen,
            x_mailer: self.x_mailer.as_deref(),
            has_zero_width: receive_ctx.deception.unjustified_zero_width,
            has_bidi_override: receive_ctx.deception.bidi_override,
            // Narrower than the reading above, and deliberately: that one
            // folds in the subject, where a zero-width space is
            // occasionally legitimate. In a display name it never was —
            // forty in the corpus, forty phishing.
            has_zero_width_in_name: self.has_zero_width_in_name,
            has_executable_attachment: mailrs_fraud::attachment::any_executable(
                self.attachment_names.iter().map(String::as_str),
            ),
            has_zero_width_inside_a_word: receive_ctx.deception.zero_width_inside_a_word,
            offers_the_reader_a_sum: self.offers_the_reader_a_sum,
            asks_for_the_readers_line: self.asks_for_the_readers_line,
            is_bulk: mailrs_inbound::body::is_bulk(&receive_ctx.message),
            to_display: &self.to_display,
            reply_rotation: self.reply_rotation,
            date_ahead: mailrs_inbound::dates::date_ahead_of(
                &receive_ctx.message,
                Some(chrono::Utc::now().timestamp()),
            ),
            // `run_antispam` is called only when the session did not
            // authenticate (`!is_authenticated` in `events/data/mod.rs`).
            // Stated as a fact rather than left to the rules to assume
            // from which code path they are running on.
            unauthenticated: true,
            // This deployment's own services submit to the MX without
            // SMTP AUTH, from the container bridge.
            peer_is_private: mailrs_inbound::identity::address_is_private(addr.ip()),
            // SPF, DKIM and DMARC are not known yet — the stage that
            // checks them runs below, in the pipeline. The rules that ask
            // decline here and get their answer from the rescan further
            // down.
            ..mailrs_fraud::Facts::default()
        }
    }
}

pub(super) async fn policy(ctx: &ConnectionContext) -> mailrs_fraud::Policy {
    mailrs_fraud::Policy {
        // From the shared store, published by fastcore at boot — not
        // from this process's environment, which is where the org
        // names live and is why half this check was off for a day.
        account_names: crate::spam_lists::account_names_async(ctx.spam_lists_client.clone()).await,
        org_names: ctx.org_names.clone(),
        our_domains: ctx.runtime.local_domains.current().to_vec(),
        allowed_domains: ctx.org_name_allowed_domains.clone(),
    }
}

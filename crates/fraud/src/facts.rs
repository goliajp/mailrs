//! What the host extracts from a message for the rules to read.

/// Upper bound on [`Facts::attachment_text`], in bytes. The rules engine
/// refuses a string fact longer than this.
pub const ATTACHMENT_TEXT_CAP: usize = 64 * 1024;

/// Everything one message offers the rules, extracted once.
///
/// Rules do not parse. The host reads the message and fills this in,
/// because extraction has already gone wrong three separate ways
/// when three code paths each did their own: the receiver, the sweep
/// and the ingest read the `From` differently, and a folded
/// `Message-ID` was invisible to one of them for a day.
///
/// Borrowed throughout — this is built per message on a path that
/// handles every message.
#[derive(Debug, Clone, Default)]
pub struct Facts<'a> {
    /// The decoded `From:` — display name and address together, as
    /// `mailrs_inbound::identity` produces it. Undecoded input is the
    /// way to make every name check answer false: the names arrive
    /// base64'd inside `=?UTF-8?B?…?=` in every real sample.
    pub from: &'a str,
    /// The sending host, lowercased: `mail02.marriottanji.com`.
    pub domain: &'a str,
    /// …and its registrable domain: `marriottanji.com`.
    pub registrable: &'a str,
    /// Messages this deployment has ever had from `registrable`,
    /// **not counting this one**. Zero for a domain nothing has ever
    /// arrived from — which is what makes a brand claim from it
    /// suspicious.
    pub domain_seen: u64,
    /// `X-Mailer`, unfolded, when the message carries one.
    pub x_mailer: Option<&'a str>,
    /// The decoded subject.
    pub subject: &'a str,
    /// RFC 8601 tokens as the receiver recorded them: `pass`,
    /// `fail`, `none`, … Empty when nothing was checked, which is a
    /// different fact from `none`.
    pub spf: &'a str,
    /// DKIM's verdict, aggregated across signatures.
    pub dkim: &'a str,
    /// DMARC's verdict — the one that decides alignment.
    pub dmarc: &'a str,
    /// Zero-width characters in the identifying text that nothing
    /// justifies — `mailrs_textguard`'s reading.
    pub has_zero_width: bool,
    /// A zero-width character inside the **display name**.
    ///
    /// Narrower than `has_zero_width`, which also covers the subject
    /// — and the difference is the whole rule. See [`crate::scan`].
    pub has_zero_width_in_name: bool,
    /// An attachment whose extension the operating system would
    /// execute: `.exe`, `.cab`, `.js`, `.lnk`, `.docm`, and the rest
    /// of that family.
    pub has_executable_attachment: bool,
    /// Whether the peer that delivered it was on a loopback or private
    /// address — this deployment's own host or LAN.
    ///
    /// Its own systems submit to the MX without SMTP AUTH: measured on
    /// production, `devops@golia.jp` and friends arrive from
    /// `172.18.0.1` (the container bridge) with `spf=softfail
    /// dkim=none dmarc=fail`, indistinguishable by authentication from
    /// a spoof. What distinguishes them is where they came from, which
    /// is not something a forger can borrow.
    pub peer_is_private: bool,
    /// Whether the message arrived on an SMTP session that did not
    /// authenticate.
    ///
    /// The receiver runs the fraud scan only for unauthenticated
    /// sessions (`!is_authenticated && antispam_enabled`), so it sets
    /// this; the historical sweep cannot know it and leaves it false.
    /// That difference is the point — "nobody proved they were us" is
    /// a fact about a connection, and a sweep over stored mail has no
    /// connection to look at.  See the `claims-our-domain` rule.
    pub unauthenticated: bool,
    /// The `To:` header's display name, decoded.
    ///
    /// Compared against a name the subject greets. Deliberately the
    /// message's *own* claim about who it is for rather than a list
    /// of the reader's names: a list can never be complete, and an
    /// incomplete one convicts every sender who greets the reader by
    /// a name it happens to be missing. Production's account row says
    /// `LI HAO`, the reader is also 李好, and LinkedIn greets 李好 by
    /// name 189 times.
    pub to_display: &'a str,
    /// How many distinct registrable domains have sent mail to this
    /// message's off-domain `Reply-To`, this one included.
    ///
    /// Zero when there is no off-domain reply address, or when the
    /// deployment has no history to count — which reads as *not
    /// rotating*, so a fresh install delivers rather than holds.
    pub reply_rotation: u32,
    /// A zero-width character wedged between two alphanumerics —
    /// `J\u{200d}CB`, `S\u{200b}A\u{200c}I\u{200d}S\u{feff}O\u{200b}N`.
    pub has_zero_width_inside_a_word: bool,
    /// A bidi override or isolate in the identifying text.
    ///
    /// `mailrs_textguard`'s reading again, and its own note on the
    /// field is the whole argument: *"no legitimate use in a
    /// sender's name."*
    pub has_bidi_override: bool,
    /// The text offers its reader part of a sum in the millions —
    /// [`crate::advance_fee::offers_the_reader_a_sum`].
    pub offers_the_reader_a_sum: bool,
    /// The text asks its reader to send their own LINE contact —
    /// [`crate::line_handoff::asks_for_the_readers_line`].
    pub asks_for_the_readers_line: bool,
    /// The message carries `List-Unsubscribe` or `List-Id`.
    ///
    /// A newsletter reporting somebody's revenue reads like an offer
    /// more often than a letter does. Absence says nothing: plenty of
    /// bulk mail carries neither header, so this can only excuse.
    pub is_bulk: bool,
    /// Seconds the `Date:` header is ahead of the message's arrival:
    /// the clock on the live path, this deployment's own `Received:`
    /// stamp in a sweep. Zero when it is not ahead or either time is
    /// missing.
    pub date_ahead: i64,
    /// Text extracted from the message's PDF attachments, joined and cut
    /// at [`ATTACHMENT_TEXT_CAP`]. Empty when nothing was extracted —
    /// always on the live path, which judges before extraction runs.
    pub attachment_text: &'a str,
    /// [`Self::offers_the_reader_a_sum`]'s reading of [`Self::attachment_text`].
    pub attachment_offers_the_reader_a_sum: bool,
    /// Links whose text shows a web address with a path on another host
    /// than the one they go to, one per line as `shown target`, at most
    /// [`crate::links::MAX_SHOWN_LINKS`] — [`crate::links::shown_links`].
    pub shown_links: &'a str,
    /// A link target still carries a kit's `[[name(...)]]` template call
    /// — [`crate::links::has_kit_placeholder`].
    pub link_has_kit_placeholder: bool,
}

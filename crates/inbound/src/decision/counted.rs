//! What each signal adds to the Junk total: the fixed scores of the
//! sender and padding signals, and what the content scan and the
//! classifier count, less on authenticated list mail.

use super::PipelineInput;

/// Score contributed to the spam total when receive-time authentication
/// folds to [`SenderTrust::Suspicious`](crate::SenderTrust::Suspicious) (DMARC alignment failed, or a hard
/// SPF/DKIM fail with no DMARC record — i.e. the From domain is being
/// spoofed or the sender's setup is broken).
///
/// Deliberately a **signal, not a hard rule**: at 3.0 against the default
/// 5.0 threshold, a suspicious sender alone still reaches the inbox (where
/// the UI's sender-trust badge flags it), but suspicious **plus** any
/// moderate content signal (>= 2.0) crosses the threshold into Junk. This
/// avoids junking a legit small domain with a transient DMARC fail under
/// `p=none`, while still catching spoofed phishing that also looks spammy.
pub const SUSPICIOUS_SENDER_SCORE: f64 = 3.0;

/// Score contributed when a From display name or Subject carries
/// zero-width characters with no typographic job — `U+200B`, `U+2060`
/// and the invisible math operators (`mailrs_textguard`).
///
/// A **score and not a verdict**, and the number comes from the
/// measurement. Over 33,602 production messages, 40 carried them and 39
/// were phishing; the one that was not is a Quora digest with a
/// zero-width space inside a long Japanese subject. One in forty is too
/// often to junk a newsletter over, and far too rare to ignore.
///
/// At 2.5 against the default 5.0 threshold it cannot junk on its own —
/// a padded subject with no other signal still reaches the inbox — but
/// it crosses with any moderate content score, and it crosses on its
/// own beside a suspicious sender (3.0 + 2.5 = 5.5). Which is the shape
/// the phishing in that sample has: padded brand name, spoofed-looking
/// setup, and spam-like content all at once.
pub const UNJUSTIFIED_ZERO_WIDTH_SCORE: f64 = 2.5;

/// The most of the spam threshold the classifier may contribute on
/// authenticated list mail: half, so the classifier alone can never
/// send it to Junk and needs at least as much again from signals that
/// are not about looking like bulk mail.
///
/// The classifier learns what the mail it was trained as spam looks
/// like, and until 2026-11 that was mostly newsletters the score had put
/// in Junk, so it scored newsletters as spam: of lihao's 982 Junk
/// conversations over 90 days, about 886 were newsletters the reader had
/// subscribed to.
pub const LIST_MAIL_CLASSIFIER_SHARE: f64 = 0.5;

/// Authenticated list mail: DMARC passed, so the `From:` domain sent it,
/// and it carries an unsubscribe link or a list id. A newsletter or a
/// notification, which belongs in Promotions or Notifications and not
/// in Junk.
pub fn authenticated_list_mail(input: &PipelineInput) -> bool {
    input.list_mail && input.auth.dmarc.eq_ignore_ascii_case("pass")
}

/// What the content scan and the classifier count toward the Junk total.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Counted {
    /// The content rules' score.
    pub content: f64,
    /// The classifier's score.
    pub classifier: f64,
}

/// The content and classifier scores as they count toward the total.
///
/// Both as scored, except on [`authenticated_list_mail`]: there the
/// format rules count nothing, and the classifier at most
/// [`LIST_MAIL_CLASSIFIER_SHARE`] of the threshold. Everything else still
/// counts in full: the content rules about what a message says (money
/// with urgency, a shortened link, a missing header), the fraud rules,
/// the sender's PTR and the zero-width padding.
pub fn counted(input: &PipelineInput) -> Counted {
    if !authenticated_list_mail(input) {
        return Counted {
            content: input.content_score,
            classifier: input.ai_score,
        };
    }
    Counted {
        content: input.content_score - input.format_score,
        classifier: input
            .ai_score
            .min(input.spam_threshold * LIST_MAIL_CLASSIFIER_SHARE),
    }
}

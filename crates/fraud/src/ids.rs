//! What each rule is called, and what it weighs.
//!
//! Split from the scan by theme: this is the wire contract, that is
//! the reading. A rule id is named by a stored verdict, by a release
//! record, and by the per-rule release rate — the only honest
//! false-positive measure there is — so renaming one is renaming a
//! column, and they are easier to keep still on their own.

/// A display name that is a company's, from a domain that is not.
/// See [`brand::brand_is_the_display_name`].
pub const RULE_BRAND_IS_THE_NAME: &str = "brand-is-the-display-name";

/// Score for a display name that is only a company's name.
///
/// Held. 96 of 36,318 production messages, and not one is real mail
/// somebody wanted.
pub const BRAND_IS_THE_NAME_SCORE: f64 = 6.0;

/// A subject that is this organisation's name and almost nothing
/// else. See [`crate::subject_is_our_name`].
pub const RULE_SUBJECT_IS_OUR_NAME: &str = "subject-is-our-name";

/// Score for a subject that is only the organisation's name.
///
/// Held. 24 of 36,976 production messages, every one of them the
/// same BEC campaign; the 66 messages that merely *name* the company
/// are untouched, and the nearest of them has six characters beside
/// the name where the threshold is four.
pub const SUBJECT_IS_OUR_NAME_SCORE: f64 = 6.0;

/// Somebody wearing the name of one of this deployment's own people.
/// See [`crate::impersonation::impersonates_one_of_us`].
pub const RULE_IMPERSONATES_ONE_OF_US: &str = "impersonates-one-of-us";

/// Score for a display name that is one of our own account holders.
///
/// Held. 9 of 36,717 production messages, 8 of them fraud; the ninth
/// is a Jira notification from a domain that belongs in the
/// allow-list.
pub const IMPERSONATES_ONE_OF_US_SCORE: f64 = 6.0;

/// Somebody claiming to be this organisation.
///
/// A rule id is a wire contract: a stored verdict names it, a
/// release records it, and the per-rule release rate — the only
/// honest false-positive measure there is — counts it. Renaming one
/// is renaming a column.
pub const RULE_CLAIMS_OUR_NAME: &str = "claims-our-name";
/// An `X-Mailer` no mail client writes. See [`RULE_CLAIMS_OUR_NAME`].
pub const RULE_GENERATED_MAILER: &str = "x-mailer-generated";
/// Somebody claiming to be a company the reader has an account with.
/// See [`RULE_CLAIMS_OUR_NAME`].
pub const RULE_IMPERSONATES_BRAND: &str = "impersonates-brand";
/// A subject claiming a company the reader has an account with.
/// See [`RULE_CLAIMS_OUR_NAME`].
pub const RULE_SUBJECT_CLAIMS_BRAND: &str = "subject-claims-brand";
/// Mail that opens by greeting somebody who is not the reader.
/// See [`RULE_CLAIMS_OUR_NAME`].
pub const RULE_GREETS_A_STRANGER: &str = "greets-a-stranger";

/// Score for a subject greeting a name the `To:` header does not
/// carry.
///
/// **Suspicion, not a hold.** 146 in the corpus, of which the great
/// majority are the fortune-telling campaign — but a dozen are
/// LinkedIn subjects opening with a Chinese phrase that is not a
/// name at all, and a handful are ordinary senders who greet in the
/// subject and leave the `To:` display name empty. Enough to push
/// toward Junk; not enough to hide mail.
pub const GREETS_A_STRANGER_SCORE: f64 = 2.0;

/// Invisible characters wedged inside a word. See
/// [`mailrs_textguard::Deception::zero_width_inside_a_word`].
pub const RULE_ZERO_WIDTH_IN_WORD: &str = "zero-width-inside-a-word";

/// Score for a word split by invisible characters.
///
/// Held. **51 of 36,318 production messages, and the 52nd — the only
/// legitimate one that carries an insertion at all — is spared by
/// the count rather than by an exception.** IKEA Japan's newsletter
/// has one, on the seam where a template put the reader's name in
/// front of an honorific; every phishing message has one inside a
/// Latin word, or two anywhere. See
/// [`mailrs_textguard::Deception::zero_width_inside_a_word`].
pub const ZERO_WIDTH_IN_WORD_SCORE: f64 = 6.0;

/// An address whose local part and registered domain both read as
/// machine-minted. See [`minted_address`].
pub const RULE_MINTED_ADDRESS: &str = "minted-address";

/// Score for an address generated on both sides of the `@`.
///
/// Held. 11 of 35,575 production messages, and all eleven are a BEC
/// campaign impersonating this company or Japanese brand phishing.
pub const MINTED_ADDRESS_SCORE: f64 = 6.0;

/// One reply address that many disposable sending domains funnel
/// into. See [`reply_rotation`].
pub const RULE_REPLY_DOMAIN_ROTATION: &str = "reply-domain-rotation";

/// Score for reply-address domain rotation.
///
/// Held on its own. Measured at 258 messages across three campaigns
/// with nothing legitimate above the threshold.
pub const REPLY_ROTATION_SCORE: f64 = 6.0;

/// A hostname naming a company that does not own it.
/// See [`RULE_CLAIMS_OUR_NAME`].
pub const RULE_HOSTNAME_CLAIMS_COMPANY: &str = "hostname-claims-company";

/// Score for a hostname that borrows a company's name.
///
/// The heaviest of the name signals. The others weigh what a message
/// says about itself, where wording can coincide; this weighs a
/// label somebody registered, where it cannot.
pub const HOSTNAME_CLAIM_SCORE: f64 = 5.0;

/// A relay host whose leading label was minted rather than chosen.
/// See [`RULE_CLAIMS_OUR_NAME`].
pub const RULE_MINTED_SENDING_HOST: &str = "minted-sending-host";

/// Score for a sending host whose name was generated.
///
/// Below the Junk threshold on its own. 34 of 36 is a prior worth
/// acting on beside anything else and not worth acting on alone —
/// which is exactly what a score buys.
pub const MINTED_SENDING_HOST_SCORE: f64 = 3.0;

/// A display name that renders as something other than what it says.
/// See [`RULE_CLAIMS_OUR_NAME`].
pub const RULE_BIDI_DISPLAY_NAME: &str = "bidi-display-name";

/// A display name with invisible characters spliced into it.
/// See [`RULE_CLAIMS_OUR_NAME`].
pub const RULE_ZERO_WIDTH_NAME: &str = "zero-width-name";
/// An attachment the operating system would run.
/// See [`RULE_CLAIMS_OUR_NAME`].
pub const RULE_EXECUTABLE_ATTACHMENT: &str = "executable-attachment";

/// Score for invisible characters inside a display name.
///
/// Forty in the corpus, forty phishing. Weighted like the other name
/// signals rather than higher: the measurement is of one mailbox's
/// mail, and a name is a weaker thing to convict on than a file the
/// machine would run.
pub const ZERO_WIDTH_NAME_SCORE: f64 = 4.5;

/// Score for an attachment the operating system would run.
///
/// The highest of them. One in 35,962 messages carried one and it
/// was malware; the cost of holding a legitimate one is a click, and
/// the cost of delivering a real one is the machine.
pub const EXECUTABLE_ATTACHMENT_SCORE: f64 = 6.0;

/// Score for a display name carrying a bidi override.
///
/// As high as the two name checks, and for a stronger reason than
/// either: those weigh a claim, and a claim can be innocent. This
/// weighs a mechanism, and `mailrs_textguard`'s own note on the field
/// is that there is *"no legitimate use in a sender's name."* A name
/// that has to be reordered to read correctly was built to be
/// misread.
pub const BIDI_DISPLAY_NAME_SCORE: f64 = 4.5;

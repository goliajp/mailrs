//! Keys for text extracted from attachments, and the queue that feeds it.

/// One attachment's extracted text — hash with [`ATTACHMENT_TEXT_FIELD`].
///
/// `n` counts the message's PDF attachments in MIME order, from 0.
///
/// No thread id on the row. The message is shared by every recipient and
/// each recipient's copy can sit in a different thread, so a search hit
/// resolves the thread through the caller's own message row instead.
pub fn attachment_text(message_id: &str, n: usize) -> String {
    format!("mailrs:atttext:{message_id}:{n}")
}

/// Key prefix the attachment-text index is declared over.
pub const ATTTEXT_PREFIX: &[u8] = b"mailrs:atttext:";

/// Name of the full-text index over attachment text.
pub const IDX_ATTACHMENT_TEXT: &[u8] = b"mailrs_attachment_text";

/// Indexed field on a `mailrs:atttext:*` row.
pub const ATTACHMENT_TEXT_FIELD: &[u8] = b"text";

/// Upper bound on stored text per attachment, in bytes. Longer text is cut
/// on a char boundary and the cut is recorded on the message's
/// [`attachment_text_state`].
pub const ATTACHMENT_TEXT_CAP: usize = 64 * 1024;

/// What extraction made of one message — hash. Its presence means the
/// message has been processed and must not be queued again.
///
/// Not under [`ATTTEXT_PREFIX`], which would feed it to the text index.
pub fn attachment_text_state(message_id: &str) -> String {
    format!("mailrs:attstate:{message_id}")
}

/// The web addresses the QR codes in a message's images and scanned pages
/// carry — hash with [`ATTACHMENT_QR_FIELD`], one address per line.
/// Written with the message's [`attachment_text_state`] and removed with it.
///
/// Not under [`ATTTEXT_PREFIX`]: an address is not text to search.
pub fn attachment_qr(message_id: &str) -> String {
    format!("mailrs:attqr:{message_id}")
}

/// Field on an [`attachment_qr`] hash.
pub const ATTACHMENT_QR_FIELD: &[u8] = b"urls";

/// Addresses kept per message: four codes on each of ten images.
pub const ATTACHMENT_QR_MAX: usize = 40;

/// Messages waiting for extraction — set of `{user}\t{message_id}`.
///
/// A queue, so it is kept by hand: delivery adds, the worker removes after
/// one attempt whatever the attempt produced.
pub const ATTACHMENT_TEXT_PENDING: &[u8] = b"mailrs:attpending";

/// Queue members whose extraction has been started and not finished:
/// member → attempts. A count that survives a restart is a process that
/// died reading that message.
pub const ATTACHMENT_TEXT_ATTEMPTS: &[u8] = b"mailrs:attattempts";

/// Queue members given up on because the process died reading them every
/// time it tried: set of `{user}\t{message_id}`.
pub const ATTACHMENT_TEXT_SET_ASIDE: &[u8] = b"mailrs:attsetaside";

/// Where the run over mail read by an older extractor stands — hash with
/// [`ATTACHMENT_BACKLOG_FIELD`], one JSON document written whole. Its
/// cursor lets the run stop and resume, across restarts.
pub const ATTACHMENT_BACKLOG: &[u8] = b"mailrs:attbacklog";

/// Field on [`ATTACHMENT_BACKLOG`].
pub const ATTACHMENT_BACKLOG_FIELD: &[u8] = b"progress";

/// Truncate `text` to at most `cap` bytes without splitting a char.
pub fn cap_text(text: &str, cap: usize) -> &str {
    if text.len() <= cap {
        return text;
    }
    let mut end = cap;
    while end > 0 && !text.is_char_boundary(end) {
        end -= 1;
    }
    &text[..end]
}

/// Remote images waiting to be fetched: members `user \t message id \t url`.
/// fastcore's prefetch worker takes and removes them.
pub const IMAGE_PREFETCH_PENDING: &[u8] = b"mailrs:imgpending";
/// The same, for images a reader with the message open asked for; the
/// worker serves these first.
pub const IMAGE_PREFETCH_ASKED: &[u8] = b"mailrs:imgasked";

//! Where a core keeps attachment text, and what every store must do alike.

use std::future::Future;
use std::io;

use mailrs_mailbox::AttachmentTextState;

/// Upper bound on stored text per attachment, in bytes: the cap the fraud
/// rules read text under.
pub const TEXT_CAP: usize = mailrs_fraud::ATTACHMENT_TEXT_CAP;

/// QR addresses kept per message: four codes on each of ten images.
pub const QR_MAX: usize = 40;

/// A core's queue of copies to read and the outcome stored for each
/// message. A message is named by its `Message-ID`; what is stored for it
/// is the same for every recipient, and goes when the last recipient's copy
/// goes. Check an implementation with [`check_attachment_text_store`].
pub trait AttachmentTextStore: Send + Sync + 'static {
    /// Queue `user`'s copy of the message for reading.
    fn queue(&self, user: &str, mid: &str) -> impl Future<Output = io::Result<()>> + Send;
    /// Up to `limit` queued `(user, message id)` pairs, left in the queue.
    fn pending(
        &self,
        limit: usize,
    ) -> impl Future<Output = io::Result<Vec<(String, String)>>> + Send;
    /// Count one more attempt at a queued copy, durably before it returns,
    /// so a process that dies reading the message leaves the count behind.
    /// Returns the attempts so far, this one included.
    fn begin(&self, user: &str, mid: &str) -> impl Future<Output = io::Result<u32>> + Send;
    /// Take the copy off the queue, and forget its attempts.
    fn dequeue(&self, user: &str, mid: &str) -> impl Future<Output = io::Result<()>> + Send;
    /// Give up on the copy: off the queue, into the set-aside set.
    fn set_aside(&self, user: &str, mid: &str) -> impl Future<Output = io::Result<()>> + Send;
    /// Whether the copy is queued.
    fn queued(&self, user: &str, mid: &str) -> impl Future<Output = io::Result<bool>> + Send;
    /// How many copies were given up on.
    fn set_aside_count(&self) -> impl Future<Output = io::Result<usize>> + Send;
    /// The file of `user`'s copy, or `None` when the copy or the file is gone.
    fn raw(&self, user: &str, mid: &str) -> impl Future<Output = Option<Vec<u8>>> + Send;
    /// The outcome stored for the message, or `None` if it was never read.
    fn state(
        &self,
        mid: &str,
    ) -> impl Future<Output = io::Result<Option<AttachmentTextState>>> + Send;
    /// Make what is stored for the message part of `user`'s copy, for a
    /// store that keeps it per copy; a store that keeps one per message
    /// has nothing to do.
    fn share(&self, user: &str, mid: &str) -> impl Future<Output = io::Result<()>> + Send;
    /// The stored text of each attachment, in attachment order.
    fn texts(&self, mid: &str) -> impl Future<Output = io::Result<Vec<String>>> + Send;
    /// Store what reading the message came to. `texts` pairs a position
    /// among the readable parts with its text, kept trimmed and cut at
    /// [`TEXT_CAP`]; an empty one is not kept. Replaces everything stored
    /// for the message before, its QR addresses included.
    fn store(
        &self,
        mid: &str,
        texts: &[(usize, String)],
        state: &AttachmentTextState,
    ) -> impl Future<Output = io::Result<()>> + Send;
    /// Store the web addresses the message's QR codes carry, replacing any
    /// stored before; trimmed, without empty or multi-line ones, at most
    /// [`QR_MAX`].
    fn store_qr(&self, mid: &str, urls: &[String]) -> impl Future<Output = io::Result<()>> + Send;
    /// Replace only the QR addresses of a message whose text is current,
    /// and record that `version` read them.
    fn refresh_qr(
        &self,
        mid: &str,
        urls: &[String],
        version: u32,
    ) -> impl Future<Output = io::Result<()>> + Send;
    /// The stored QR addresses, in the order they were read.
    fn qr_urls(&self, mid: &str) -> impl Future<Output = io::Result<Vec<String>>> + Send;
    /// A connection to the network kevy the number histories live in.
    /// Blocking; called off the async runtime.
    fn net_conn(&self) -> Option<kevy_client::Connection>;
}

/// The QR addresses a store keeps of `urls`: trimmed, without empty or
/// multi-line ones, at most [`QR_MAX`].
pub fn kept_qr_urls(urls: &[String]) -> Vec<&str> {
    urls.iter()
        .map(|u| u.trim())
        .filter(|u| !u.is_empty() && !u.contains('\n'))
        .take(QR_MAX)
        .collect()
}

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

/// Panic unless `s` keeps attachment text as [`AttachmentTextStore`] says.
///
/// `user` must have a copy of `mid` delivered and `other` must have one
/// too; nothing may be stored or queued for `mid` yet.
pub async fn check_attachment_text_store<S: AttachmentTextStore>(
    s: &S,
    user: &str,
    other: &str,
    mid: &str,
) {
    // the queue: a pair once, until it is taken off; attempts start over
    assert!(!s.queued(user, mid).await.unwrap());
    s.queue(user, mid).await.unwrap();
    s.queue(user, mid).await.unwrap();
    s.queue(other, mid).await.unwrap();
    let mut got = s.pending(10).await.unwrap();
    got.sort();
    let mut want = vec![
        (user.to_string(), mid.to_string()),
        (other.to_string(), mid.to_string()),
    ];
    want.sort();
    assert_eq!(got, want, "pending");
    assert_eq!(s.pending(1).await.unwrap().len(), 1, "pending is bounded");
    assert_eq!(s.begin(user, mid).await.unwrap(), 1);
    assert_eq!(s.begin(user, mid).await.unwrap(), 2);
    s.dequeue(user, mid).await.unwrap();
    assert!(!s.queued(user, mid).await.unwrap());
    s.queue(user, mid).await.unwrap();
    assert_eq!(s.begin(user, mid).await.unwrap(), 1, "attempts start over");
    let before = s.set_aside_count().await.unwrap();
    s.set_aside(user, mid).await.unwrap();
    assert!(!s.queued(user, mid).await.unwrap());
    assert_eq!(s.set_aside_count().await.unwrap(), before + 1);
    s.dequeue(other, mid).await.unwrap();
    assert!(s.pending(10).await.unwrap().is_empty());

    // the outcome: nothing until stored, then replaced whole
    assert!(s.raw(user, mid).await.is_some(), "the copy's file");
    assert_eq!(s.state(mid).await.unwrap(), None);
    let state = AttachmentTextState {
        version: 5,
        parts: 3,
        pdfs: 2,
        images: 1,
        stored: 2,
        truncated: 1,
        ..Default::default()
    };
    let long = "日".repeat(TEXT_CAP);
    let texts = [
        (0, "  first  ".to_string()),
        (1, "   ".to_string()),
        (2, long),
    ];
    s.store(mid, &texts, &state).await.unwrap();
    assert_eq!(s.state(mid).await.unwrap(), Some(state.clone()));
    let back = s.texts(mid).await.unwrap();
    assert_eq!(back.len(), 2, "an empty text is not kept");
    assert_eq!(back[0], "first");
    assert!(back[1].len() <= TEXT_CAP && back[1].len() > TEXT_CAP - 3);
    s.share(other, mid).await.unwrap();
    assert_eq!(s.texts(mid).await.unwrap(), back, "sharing changes nothing");

    // QR addresses: kept clean and bounded, replaced, gone with a new read
    assert!(s.qr_urls(mid).await.unwrap().is_empty());
    let mut urls: Vec<String> = (0..QR_MAX + 5)
        .map(|n| format!(" https://h{n}.example/ "))
        .collect();
    urls.insert(0, "two\nlines".to_string());
    urls.insert(0, String::new());
    s.store_qr(mid, &urls).await.unwrap();
    let kept = s.qr_urls(mid).await.unwrap();
    assert_eq!(kept.len(), QR_MAX);
    assert_eq!(kept[0], "https://h0.example/");
    s.refresh_qr(mid, &urls[2..3], 13).await.unwrap();
    assert_eq!(s.qr_urls(mid).await.unwrap(), ["https://h0.example/"]);
    let refreshed = s.state(mid).await.unwrap().unwrap();
    assert_eq!((refreshed.version, refreshed.stored), (13, 2));
    assert_eq!(
        s.texts(mid).await.unwrap(),
        back,
        "a refresh keeps the text"
    );
    let again = AttachmentTextState {
        version: 5,
        parts: 1,
        pdfs: 1,
        stored: 1,
        ..Default::default()
    };
    s.store(mid, &[(0, "again".to_string())], &again)
        .await
        .unwrap();
    assert_eq!(s.texts(mid).await.unwrap(), ["again"]);
    assert!(s.qr_urls(mid).await.unwrap().is_empty());
    assert_eq!(s.state(mid).await.unwrap(), Some(again));
}

//! A letter that asks its reader to send over their own LINE contact.
//!
//! ```text
//! メールを受け取った後、今後の業務連絡をより効率的に行うため、
//! ご自身の個人LINEのQRコードまでこのメールアドレス宛に返信してください。
//! ```
//!
//! The business-email compromise aimed at this company never asks for
//! money in the letter. It asks for the reader's LINE QR code, and the
//! money is asked for later, in a chat no mail filter sees. The letter
//! changes everything else from wave to wave — the persona (`齋藤 真`,
//! `富川 貴司`, `西口 征郎`), the company it signs as, the sending
//! domain, freemail or minted or our own — and keeps this one request.
//!
//! # What counts
//!
//! Within one sentence, either
//!
//! - **a word for the reader, then LINE, then a QR code**:
//!   `ご自身の個人LINEのQRコード`, `您的LINE二维码`; or
//! - **LINE, then a QR code, then "please send" or "please reply"**:
//!   `友だち追加用のQRコード（または招待リンク）を返信で送ってください`.
//!
//! LINE and a QR code together are ordinary: shops print `LINE限定クーポン
//! ＼友達追加／ QRコード` in every newsletter, and card issuers list
//! `ライン決済` beside `QR`. What none of them does is ask the reader to
//! hand theirs over. A sender offering their own code — `私のLINEのQRコードを
//! 送ります` — names no reader and asks for nothing, so it is not convicted.
//!
//! # Measured
//!
//! Over 43,838 production messages on 2026-10-23: 263 matched, every
//! one this campaign, across all its personas; nothing else matched.
//! Japanese and Chinese only, because every sample is; a letter in any
//! other language reads as *no request*, which is the direction to be
//! wrong in.

use crate::advance_fee::{Body, reading};

const LINE: &[&str] = &["line", "ｌｉｎｅ", "ライン"];
const QR: &[&str] = &["qr", "ｑｒ", "二维码", "二維碼"];
/// Words that point at the person reading. `自分` and `個人` are left
/// out: `自分のLINE` is as often the writer's own.
const READER: &[&str] = &[
    "ご自身",
    "ご自分",
    "ご本人",
    "あなた",
    "貴方",
    "お客様",
    "您",
    "你的",
];
const SEND: &[&str] = &["返信", "送って", "送付", "お送り"];
const PLEASE: &[&str] = &["ください", "下さい"];

/// How far apart, in characters, the parts of the request may sit.
const NEAR: usize = 30;
const QR_TO_SEND: usize = 40;
const SEND_TO_PLEASE: usize = 15;
/// The longest word in [`READER`], in characters.
const LONGEST: usize = 3;

/// Whether the text asks its reader to send their own LINE contact.
#[must_use]
pub fn asks_for_the_readers_line<'a>(bodies: impl IntoIterator<Item = Body<'a>>) -> bool {
    let text = reading(bodies);
    text.split('。').any(|sentence| {
        let s: Vec<char> = sentence.chars().collect();
        let line = starts(&s, LINE);
        if line.is_empty() {
            return false;
        }
        let at = Positions {
            reader: starts(&s, READER),
            qr: starts(&s, QR),
            send: starts(&s, SEND),
            please: starts(&s, PLEASE),
        };
        line.into_iter()
            .any(|(start, end)| at.names_the_reader(start, end) || at.asks_to_send(end))
    })
}

/// Where each kind of word occurs in one sentence, found once: a
/// stylesheet left in the text says `line-height` hundreds of times.
struct Positions {
    reader: Vec<(usize, usize)>,
    qr: Vec<(usize, usize)>,
    send: Vec<(usize, usize)>,
    please: Vec<(usize, usize)>,
}

impl Positions {
    /// A reader word ending at most [`NEAR`] before LINE, and a QR code
    /// starting at most [`NEAR`] after it.
    fn names_the_reader(&self, line_at: usize, line_end: usize) -> bool {
        let reader_before = window(
            &self.reader,
            line_at.saturating_sub(NEAR + LONGEST),
            line_at,
        )
        .iter()
        .any(|&(_, end)| end <= line_at && line_at - end <= NEAR);
        reader_before && within(&self.qr, line_end, NEAR).is_some()
    }

    /// A QR code after LINE, then a sending verb, then `ください`.
    fn asks_to_send(&self, line_end: usize) -> bool {
        let Some(qr_end) = within(&self.qr, line_end, QR_TO_SEND) else {
            return false;
        };
        window(&self.send, qr_end, qr_end + QR_TO_SEND)
            .iter()
            .any(|&(_, send_end)| within(&self.please, send_end, SEND_TO_PLEASE).is_some())
    }
}

/// The end of the earliest occurrence starting within `reach`
/// characters after `from`.
fn within(found: &[(usize, usize)], from: usize, reach: usize) -> Option<usize> {
    window(found, from, from + reach)
        .iter()
        .map(|&(_, end)| end)
        .min()
}

/// The occurrences starting in `from..=to`. `found` is sorted by start,
/// so a text that repeats `line qr` for 256 KiB still costs a binary
/// search and a few dozen comparisons per occurrence, not one pass over
/// all of them.
fn window(found: &[(usize, usize)], from: usize, to: usize) -> &[(usize, usize)] {
    let lo = found.partition_point(|&(at, _)| at < from);
    let hi = found.partition_point(|&(at, _)| at <= to);
    &found[lo..hi.max(lo)]
}

/// Every `(start, end)` at which one of `words` occurs, in characters,
/// sorted by start.
fn starts(s: &[char], words: &[&str]) -> Vec<(usize, usize)> {
    let mut out = Vec::new();
    for word in words {
        let w: Vec<char> = word.chars().collect();
        if w.len() > s.len() {
            continue;
        }
        for at in 0..=s.len() - w.len() {
            if s[at..at + w.len()] == w[..] {
                out.push((at, at + w.len()));
            }
        }
    }
    out.sort_unstable();
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn asks(text: &str) -> bool {
        asks_for_the_readers_line([Body::Plain(text)])
    }

    /// The letter that reached an inbox on 2026-10-23, from `nogz.cn`.
    #[test]
    fn the_campaigns_request_is_caught() {
        assert!(asks(
            "お疲れ様です。\nメールを受け取った後、今後の業務連絡をより効率的に行うため、\n\
             ご自身の個人LINEのQRコードまでこのメールアドレス宛に返信してください。"
        ));
    }

    #[test]
    fn the_wording_without_a_reader_word_is_caught() {
        assert!(asks(
            "新しいLINEアカウントから改めて連絡しますので、お手数ですが、\
             友だち追加用のQRコード（または招待リンク）を返信で送ってください。"
        ));
        assert!(asks("LINEのQRコードを本メール宛にご返信ください。"));
        assert!(asks(
            "LINE グループの QR コード を 本メール宛にご送付ください。"
        ));
    }

    #[test]
    fn full_width_letters_and_html_are_read() {
        assert!(asks("ご自身のＬＩＮＥのＱＲコードを返信してください。"));
        assert!(asks_for_the_readers_line([Body::Html(
            "<p>ご自身の個人<b>LINE</b>の<span>QR</span>コードを返信してください</p>"
        )]));
    }

    #[test]
    fn a_chinese_request_is_caught() {
        assert!(asks("请把您的LINE二维码发给我。"));
    }

    /// The newsletters and card statements in the corpus.
    #[test]
    fn shops_and_card_issuers_are_not_convicted() {
        assert!(!asks(
            "LINE限定のお得なクーポンも定期配信中！ ＼タップして友達追加／ ＼QRコードはこちら／"
        ));
        assert!(!asks(
            "楽天ペイ（ライン決済）を含む各種バーコード・QRコード決済が対象です。"
        ));
        assert!(!asks("LINEのQRコードを読み取ってください。"));
    }

    /// Not in the corpus: somebody offering their own code. It names no
    /// reader and asks for nothing back.
    #[test]
    fn offering_your_own_code_is_not_convicted() {
        assert!(!asks(
            "私のLINEのQRコードを送りますので、追加をお願いします。"
        ));
        assert!(!asks("自分のLINEのQRコードを添付しました。"));
    }

    /// The parts of the request have to be in one sentence.
    #[test]
    fn a_sentence_break_separates_the_parts() {
        assert!(!asks(
            "ご自身の予定をご確認ください。LINEのQRコードは後日お知らせします。"
        ));
    }

    #[test]
    fn english_reads_as_no_request() {
        assert!(!asks("Please reply with your LINE QR code."));
    }
}

//! Running the parsers on one request: what the worker process does with
//! each frame, and what a process without a worker does in place.
//!
//! `step` is called with each parser before it runs; the worker turns
//! that into a progress frame.

use crate::proto::{
    Answer, AnswerHeader, CleanedHtml, HtmlFacts, MessageReading, Op, Parser, Tokenizer, Want,
};

/// The answer to `op` on `input`.
pub fn answer(op: &Op, input: &[u8], step: &mut dyn FnMut(Parser)) -> Answer {
    match op {
        Op::Message(want) => Answer {
            header: AnswerHeader::Message(message(*want, input, step)),
            blobs: Vec::new(),
        },
        Op::ReportPayloads => {
            step(Parser::ReportDecompress);
            Answer {
                header: AnswerHeader::ReportPayloads,
                blobs: mailrs_dmarc::ingest::extract_report_payloads(input),
            }
        }
        Op::Office { kind } => {
            step(Parser::Office);
            let lines = mailrs_attachment_extract::office::office_text((*kind).into(), input)
                .map(|sections| sections.into_iter().flat_map(|s| s.lines).collect());
            Answer {
                header: AnswerHeader::Office { lines },
                blobs: Vec::new(),
            }
        }
        Op::CleanHtml => {
            step(Parser::CleanHtml);
            let cleaned = mailrs_clean::clean_email_html(&String::from_utf8_lossy(input));
            Answer {
                header: AnswerHeader::CleanHtml(CleanedHtml {
                    facts: HtmlFacts {
                        has_tracking_pixel: cleaned.has_tracking_pixel,
                        is_template_heavy: cleaned.is_template_heavy,
                        link_count: cleaned.link_count,
                    },
                    clean_text: cleaned.clean_text,
                }),
                blobs: Vec::new(),
            }
        }
        Op::Pdf => {
            step(Parser::Pdf);
            Answer {
                header: AnswerHeader::Pdf {
                    pages: mailrs_attachment_extract::extract_pdf_pages(input),
                },
                blobs: Vec::new(),
            }
        }
    }
}

fn message(want: Want, raw: &[u8], step: &mut dyn FnMut(Parser)) -> MessageReading {
    let mut out = MessageReading::default();
    if want.body_text || want.inside || want.html_facts {
        step(Parser::Mime);
        let parsed = mailrs_mime::parse(raw);
        if want.body_text {
            step(Parser::Html2text);
            out.body_text = mailrs_clean::body_text_for_search(&parsed);
        }
        if want.inside {
            let parts: Vec<(&str, &[u8])> = parsed
                .attachments()
                .map(|p| (p.attachment_filename().unwrap_or(""), &p.body[..]))
                .collect();
            // each RAR is read on its own first, so a worker that dies in
            // the RAR walker is counted under `rar`; the listing is
            // bounded by the archive's size, so reading it twice is cheap
            for part in parts
                .iter()
                .filter(|(_, b)| mailrs_fraud::attachment_inside::is_rar(b))
            {
                step(Parser::Rar);
                let _ = mailrs_fraud::attachment_inside::read([*part]);
            }
            step(Parser::AttachmentInside);
            out.inside = mailrs_fraud::attachment_inside::read(parts).into();
        }
        if want.html_facts {
            step(Parser::CleanHtml);
            out.html_facts = html_facts(&parsed);
        }
    }
    if let Some(tokenizer) = want.tokens {
        step(Parser::BayesTokenizer);
        out.tokens = match tokenizer {
            Tokenizer::Raw => mailrs_bayes::tokenize_raw(raw),
            Tokenizer::Decoded => mailrs_bayes::tokenize(raw),
        };
    }
    out
}

/// The first `text/html` part's markup, cleaned: whether it carries a
/// tracking pixel, is mostly template, and how many links it has.
fn html_facts(parsed: &mailrs_mime::Part<'_>) -> Option<HtmlFacts> {
    let html = parsed
        .walk()
        .find(|p| p.content_type.mime_type().as_str() == "text/html")
        .and_then(|p| p.body_text())?;
    let cleaned = mailrs_clean::clean_email_html(&html);
    Some(HtmlFacts {
        has_tracking_pixel: cleaned.has_tracking_pixel,
        is_template_heavy: cleaned.is_template_heavy,
        link_count: cleaned.link_count,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    const MSG: &[u8] = b"From: a@b.example\r\n\
Subject: hello\r\n\
Content-Type: multipart/mixed; boundary=b\r\n\
\r\n\
--b\r\n\
Content-Type: text/html\r\n\
\r\n\
<p>Hello <a href=\"https://x.example/\">there</a></p>\r\n\
--b\r\n\
Content-Type: application/octet-stream; name=run.exe\r\n\
Content-Disposition: attachment; filename=run.exe\r\n\
\r\n\
MZ\r\n\
--b--\r\n";

    #[test]
    fn each_reading_is_the_one_its_parser_gives_and_names_its_steps() {
        let mut steps = Vec::new();
        let want = Want {
            body_text: true,
            inside: true,
            tokens: Some(Tokenizer::Raw),
            html_facts: true,
        };
        let a = answer(&Op::Message(want), MSG, &mut |p| steps.push(p));
        let AnswerHeader::Message(r) = a.header else {
            panic!("a message reading");
        };
        let parsed = mailrs_mime::parse(MSG);
        assert_eq!(r.body_text, mailrs_clean::body_text_for_search(&parsed));
        assert!(r.inside.carries_code);
        assert_eq!(r.tokens, mailrs_bayes::tokenize_raw(MSG));
        assert_eq!(r.html_facts.map(|h| h.link_count), Some(1));
        assert_eq!(
            steps,
            [
                Parser::Mime,
                Parser::Html2text,
                Parser::AttachmentInside,
                Parser::CleanHtml,
                Parser::BayesTokenizer
            ]
        );
    }

    /// A RAR is read under its own progress step, before the whole
    /// reading, so a death in the RAR walker is counted as `rar`; a RAR
    /// whose headers break off reads as an archive that will not say what
    /// it holds.
    #[test]
    fn a_rar_attachment_is_read_under_its_own_step() {
        let msg = b"From: a@b.example\r\n\
Content-Type: multipart/mixed; boundary=b\r\n\
\r\n\
--b\r\n\
Content-Type: application/octet-stream\r\n\
Content-Disposition: attachment; filename=order.dat\r\n\
\r\n\
Rar!\x1a\x07\x01\x00\r\n\
--b--\r\n";
        let mut steps = Vec::new();
        let want = Want {
            inside: true,
            ..Want::default()
        };
        let a = answer(&Op::Message(want), msg, &mut |p| steps.push(p));
        let AnswerHeader::Message(r) = a.header else {
            panic!("a message reading");
        };
        assert_eq!(steps, [Parser::Mime, Parser::Rar, Parser::AttachmentInside]);
        assert!(r.inside.unreadable_archive, "{:?}", r.inside);
        assert!(!r.inside.carries_code);
    }

    #[test]
    fn cleaned_html_is_what_the_cleaner_gives() {
        let html = "<p>Hello <a href=\"https://x.example/\">there</a></p>";
        let a = answer(&Op::CleanHtml, html.as_bytes(), &mut |_| {});
        let AnswerHeader::CleanHtml(c) = a.header else {
            panic!("a cleaned document");
        };
        let want = mailrs_clean::clean_email_html(html);
        assert_eq!(c.clean_text, want.clean_text);
        assert_eq!(c.facts.link_count, want.link_count);
    }

    #[test]
    fn nothing_asked_for_runs_nothing() {
        let mut steps = Vec::new();
        let a = answer(&Op::Message(Want::default()), MSG, &mut |p| steps.push(p));
        assert_eq!(a.header, AnswerHeader::Message(MessageReading::default()));
        assert!(steps.is_empty());
    }

    #[test]
    fn a_file_that_is_not_office_says_why() {
        let a = answer(
            &Op::Office {
                kind: crate::proto::OfficeKind::Word,
            },
            b"not a zip",
            &mut |_| {},
        );
        assert!(matches!(a.header, AnswerHeader::Office { lines: Err(_) }));
    }
}

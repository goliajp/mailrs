//! The parsed view of a captured message.

use mailrs_rfc5322::{Message, split_list};
use serde::{Deserialize, Serialize};

use crate::extract;

/// What a test reads first about a message. Small enough to keep beside
/// the message's id; the bodies are parsed again from the stored file.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Summary {
    pub from: String,
    pub to: Vec<String>,
    pub cc: Vec<String>,
    pub subject: String,
    pub message_id: Option<String>,
    pub codes: Vec<String>,
    pub links: Vec<Link>,
    pub attachments: Vec<Attachment>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Link {
    pub url: String,
    pub text: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Attachment {
    pub filename: String,
    pub size: usize,
    pub content_type: String,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ParsedMessage {
    #[serde(flatten)]
    pub summary: Summary,
    pub text: String,
    pub html: Option<String>,
}

/// Headers, bodies, attachments, codes and links of the raw message.
/// Any input gives a result: missing parts come back empty.
pub fn parse_message(raw: &[u8]) -> ParsedMessage {
    let msg = Message::new(raw);
    let field = |name: &str| msg.header(name).map(unfold).unwrap_or_default();
    let decoded = |value: &str| mailrs_rfc2047::decode(value.as_bytes()).trim().to_string();
    // split before decoding: a decoded display name may hold a bare comma
    let list = |name: &str| -> Vec<String> {
        split_list(&field(name))
            .filter(|item| !item.ends_with(';') || item.contains('@'))
            .map(decoded)
            .collect()
    };
    let subject = decoded(&field("Subject"));
    let message_id = Some(field("Message-ID")).filter(|id| !id.is_empty());

    let root = mailrs_mime::parse(raw);
    let body = |mime: &str| {
        root.walk()
            .find(|p| !p.is_attachment() && p.content_type.mime_type() == mime)
            .and_then(|p| p.body_text())
    };
    let html = body("text/html");
    let text = body("text/plain")
        .or_else(|| html.as_deref().map(mailrs_clean::html_to_clean_text))
        .unwrap_or_default();
    let attachments = root
        .attachments()
        .map(|p| Attachment {
            filename: p.attachment_filename().map(decoded).unwrap_or_default(),
            size: p.body.len(),
            content_type: p.content_type.mime_type(),
        })
        .collect();

    let searched = format!("{subject}\n{text}");
    ParsedMessage {
        summary: Summary {
            from: decoded(&field("From")),
            to: list("To"),
            cc: list("Cc"),
            codes: extract::codes(&searched),
            links: extract::links(html.as_deref(), &searched),
            subject,
            message_id,
            attachments,
        },
        text,
        html,
    }
}

// a folded header is one logical line
fn unfold(value: &[u8]) -> String {
    let text = String::from_utf8_lossy(value);
    let mut out = String::with_capacity(text.len());
    for line in text.split('\n') {
        let line = line.trim_end_matches('\r');
        if out.is_empty() {
            out.push_str(line.trim());
        } else {
            let line = line.trim_start();
            if !line.is_empty() {
                out.push(' ');
                out.push_str(line);
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    const ALTERNATIVE: &[u8] = b"From: =?UTF-8?B?R2l0SHVi?= <noreply@github.test>\r\n\
To: \"Doe, Jane\" <insight.signup@mock.example.test>,\r\n bob@x.test\r\n\
Cc: undisclosed-recipients:;\r\n\
Subject: =?UTF-8?Q?Please_verify_your_account?=\r\n\
Message-ID:  <abc@github.test> \r\n\
MIME-Version: 1.0\r\n\
Content-Type: multipart/alternative; boundary=\"xx\"\r\n\
\r\n\
--xx\r\n\
Content-Type: text/plain; charset=utf-8\r\n\
\r\n\
Your verification code is 482913\r\n\
Docs: https://docs.github.test/start\r\n\
--xx\r\n\
Content-Type: text/html; charset=utf-8\r\n\
\r\n\
<p><a href=\"https://github.test/settings\">Settings</a></p>\r\n\
<p><a href=\"https://github.test/verify?token=abc&amp;u=1\">Verify email</a></p>\r\n\
--xx--\r\n";

    #[test]
    fn multipart_alternative_with_verify_link_first() {
        let m = parse_message(ALTERNATIVE);
        let s = &m.summary;
        assert_eq!(s.from, "GitHub <noreply@github.test>");
        assert_eq!(
            s.to,
            [
                "\"Doe, Jane\" <insight.signup@mock.example.test>",
                "bob@x.test"
            ]
        );
        assert!(s.cc.is_empty());
        assert_eq!(s.subject, "Please verify your account");
        assert_eq!(s.message_id.as_deref(), Some("<abc@github.test>"));
        assert_eq!(s.codes, ["482913"]);
        assert_eq!(
            s.links,
            [
                Link {
                    url: "https://github.test/verify?token=abc&u=1".into(),
                    text: "Verify email".into(),
                },
                Link {
                    url: "https://github.test/settings".into(),
                    text: "Settings".into(),
                },
                Link {
                    url: "https://docs.github.test/start".into(),
                    text: String::new(),
                },
            ]
        );
        assert!(m.text.starts_with("Your verification code is 482913"));
        assert!(
            m.html
                .as_deref()
                .is_some_and(|h| h.contains("Verify email"))
        );
        assert!(s.attachments.is_empty());
    }

    #[test]
    fn html_only_message_gets_text_from_html() {
        let raw = "Subject: ログイン\r\nContent-Type: text/html; charset=utf-8\r\n\r\n\
<p>認証コード：<b>１２３４５６</b></p><p>有効期限は10分です</p>";
        let m = parse_message(raw.as_bytes());
        assert!(m.text.contains("認証コード"));
        assert_eq!(m.summary.codes, ["123456"]);
        assert!(m.html.is_some());
    }

    #[test]
    fn attachments_with_decoded_size() {
        let raw = b"Subject: report\r\n\
Content-Type: multipart/mixed; boundary=\"b\"\r\n\
\r\n\
--b\r\n\
Content-Type: text/plain\r\n\
\r\n\
see attached\r\n\
--b\r\n\
Content-Type: application/pdf; name=\"r.pdf\"\r\n\
Content-Disposition: attachment; filename=\"=?UTF-8?B?5aCx5ZGK?=.pdf\"\r\n\
Content-Transfer-Encoding: base64\r\n\
\r\n\
aGVsbG8=\r\n\
--b--\r\n";
        let m = parse_message(raw);
        assert_eq!(m.text.trim(), "see attached");
        assert_eq!(
            m.summary.attachments,
            [Attachment {
                filename: "報告.pdf".into(),
                size: 5,
                content_type: "application/pdf".into(),
            }]
        );
    }

    #[test]
    fn missing_everything() {
        let m = parse_message(b"");
        assert_eq!(m.summary, Summary::default());
        assert!(m.text.is_empty());
        assert!(m.html.is_none());
    }

    #[test]
    fn broken_inputs_do_not_panic() {
        let samples: &[&[u8]] = &[
            b"\xff\xfe\x00\x80",
            b"Content-Type: multipart/mixed; boundary=\"\"\r\n\r\n--\r\n--",
            b"Content-Type: multipart/mixed; boundary=b\r\n\r\n--b\r\nContent-Transfer-Encoding: base64\r\n\r\n!!!\r\n",
            b"Subject: =?UTF-8?B?\xff?=\r\nTo: ,,,<,>\"\r\n\r\n",
            b"Content-Type: text/html\r\n\r\n<a href=\"https://x.test/&#",
        ];
        for raw in samples {
            let _ = parse_message(raw);
        }
    }

    proptest::proptest! {
        #[test]
        fn arbitrary_bytes_do_not_panic(raw in proptest::collection::vec(proptest::num::u8::ANY, 0..2048)) {
            let _ = parse_message(&raw);
        }

        #[test]
        fn arbitrary_text_does_not_panic(text in "\\PC{0,400}") {
            let _ = extract::codes(&text);
            let _ = extract::links(Some(&text), &text);
        }
    }
}

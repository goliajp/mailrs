//! How much the replay actually tested: what kinds of attachment the
//! sample carried, and for each rule how many messages gave its detector
//! something to read. Zero hits over thirty PDFs says little; over three
//! thousand it says the rule stays quiet on real mail.
//!
//! A message counts once per kind however many parts of that kind it
//! carries. The kind is read from the bytes first (`%PDF`, a zip, rar or
//! 7z signature, an OLE compound file, `{\rtf`) and from the name and
//! declared type only where the bytes cannot tell (OOXML is a zip with a
//! content-types part; HTML and SVG are text). A file named as a program
//! is `executable` whatever it holds.

use std::collections::{BTreeMap, BTreeSet};

/// The kinds the report counts.
pub(super) const KINDS: [&str; 8] = [
    "pdf",
    "zip",
    "rar_7z",
    "ooxml",
    "ole",
    "rtf",
    "html_svg",
    "executable",
];

/// What one attachment is, for coverage. `None` for anything else.
pub(super) fn kind_of(
    name: &str,
    declared: &str,
    bytes: &[u8],
    executable: bool,
) -> Option<&'static str> {
    if executable {
        return Some("executable");
    }
    let lower = name.to_ascii_lowercase();
    let ext = lower.rsplit_once('.').map(|(_, e)| e).unwrap_or("");
    let declared = declared.to_ascii_lowercase();
    if bytes.starts_with(b"%PDF") {
        return Some("pdf");
    }
    if bytes.starts_with(b"PK\x03\x04") {
        let head = &bytes[..bytes.len().min(64 * 1024)];
        let ooxml = head.windows(19).any(|w| w == b"[Content_Types].xml");
        return match ooxml {
            true => Some("ooxml"),
            false => Some("zip"),
        };
    }
    if bytes.starts_with(b"Rar!") || bytes.starts_with(b"7z\xbc\xaf\x27\x1c") {
        return Some("rar_7z");
    }
    if bytes.starts_with(&[0xd0, 0xcf, 0x11, 0xe0]) {
        return Some("ole");
    }
    if bytes.starts_with(b"{\\rtf") {
        return Some("rtf");
    }
    let html_name = matches!(ext, "htm" | "html" | "shtml" | "xhtml" | "svg");
    let html_type = declared == "text/html" || declared == "image/svg+xml";
    match html_name || html_type {
        true => Some("html_svg"),
        false => None,
    }
}

/// The inputs a message gave the detectors.
#[derive(Default)]
pub(super) struct Seen {
    /// The attachment kinds it carried.
    pub kinds: BTreeSet<&'static str>,
    pub any_attachment: bool,
    pub client_ip: bool,
    pub sender_domain: bool,
    pub link_hosts: bool,
}

/// Which messages a rule's detector reads: the kinds of attachment it
/// looks inside, or the reputation input it looks up. `None` for a rule
/// this replay does not know, which is left out of the coverage rather
/// than guessed.
fn rule_saw(rule: &str, s: &Seen) -> Option<bool> {
    let kinds = |ks: &[&str]| ks.iter().any(|k| s.kinds.contains(k));
    let saw = match rule {
        "exe-name" | "double-extension" | "type-mismatch" | "own-hash-listed" => s.any_attachment,
        "archive-executable" | "encrypted-archive" => kinds(&["zip"]),
        "unlistable-archive" => kinds(&["rar_7z"]),
        "office-macros" | "office-object" => kinds(&["ooxml", "ole"]),
        "rtf-object" => kinds(&["rtf"]),
        "pdf-script" | "pdf-embedded" => kinds(&["pdf"]),
        "html-form" | "svg-script" | "html-smuggling" => kinds(&["html_svg"]),
        "ip-incident-manual" | "ip-abuse" | "ip-login-guard" => s.client_ip,
        "sender-incident-manual" | "sender-user-junk" | "sender-complaint" | "auth-fail-listed" => {
            s.sender_domain
        }
        "link-incident-manual" | "link-user-junk" => s.link_hosts,
        _ => return None,
    };
    Some(saw)
}

/// The rules [`rule_saw`] knows, so the report lists every one even when
/// no message reached it.
pub(super) const KNOWN_RULES: [&str; 24] = [
    "exe-name",
    "double-extension",
    "type-mismatch",
    "archive-executable",
    "encrypted-archive",
    "unlistable-archive",
    "office-macros",
    "office-object",
    "rtf-object",
    "pdf-script",
    "pdf-embedded",
    "html-form",
    "svg-script",
    "html-smuggling",
    "own-hash-listed",
    "ip-incident-manual",
    "ip-abuse",
    "ip-login-guard",
    "sender-incident-manual",
    "link-incident-manual",
    "sender-user-junk",
    "link-user-junk",
    "sender-complaint",
    "auth-fail-listed",
];

/// One sample's coverage, and its hits that only observations recorded
/// after the message arrived explain.
#[derive(Default, serde::Serialize)]
pub(super) struct Coverage {
    pub messages_with_attachments: u64,
    pub attachments: BTreeMap<&'static str, u64>,
    pub detector_saw: BTreeMap<&'static str, u64>,
    pub after_the_fact: u64,
    pub after_the_fact_by_rule: BTreeMap<String, u64>,
}

impl Coverage {
    pub(super) fn new() -> Self {
        let mut c = Coverage::default();
        for k in KINDS {
            c.attachments.insert(k, 0);
        }
        for r in KNOWN_RULES {
            c.detector_saw.insert(r, 0);
        }
        c
    }

    pub(super) fn add(&mut self, s: &Seen) {
        if s.any_attachment {
            self.messages_with_attachments += 1;
        }
        for k in &s.kinds {
            *self.attachments.entry(k).or_default() += 1;
        }
        for r in KNOWN_RULES {
            if rule_saw(r, s) == Some(true) {
                *self.detector_saw.entry(r).or_default() += 1;
            }
        }
    }
}

/// What the detectors were given by `raw`, read the way the replay's
/// judging reads it; whether a file is named as a program is the
/// inspector's own answer.
pub(super) fn seen(raw: &[u8], inspector: &mailrs_attachment_scan::Inspector) -> Seen {
    let parsed = mailrs_mime::parse(raw);
    let parts = mailrs_inbound::scan_input::attachments(&parsed);
    let atts: Vec<mailrs_attachment_scan::Attachment> = parts
        .iter()
        .map(|p| mailrs_attachment_scan::Attachment {
            name: &p.name,
            declared_type: &p.declared_type,
            bytes: p.bytes,
        })
        .collect();
    let facts = inspector.inspect(&atts);
    let mut s = Seen {
        any_attachment: !parts.is_empty(),
        client_ip: mailrs_inbound::identity::first_received_peer_ip(raw).is_some(),
        sender_domain: mailrs_inbound::identity::from_address(raw).contains('@'),
        link_hosts: !mailrs_inbound::scan_input::link_hosts(&parsed).is_empty(),
        ..Seen::default()
    };
    for (i, p) in parts.iter().enumerate() {
        let executable = facts
            .iter()
            .any(|f| f.part == i && f.executable_name.is_some());
        if let Some(k) = kind_of(&p.name, &p.declared_type, p.bytes, executable) {
            s.kinds.insert(k);
        }
    }
    s
}

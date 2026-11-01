//! The mbox file format, both ways: mboxrd (a body line matching `>*From `
//! gains one `>` on the way out and loses it on the way in), with the folder
//! and read state carried in headers so an export imports back as it was.

/// Where a message is filed, as an export records it and an import reads it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Folder {
    Inbox,
    Sent,
    Archive,
    /// Junk or the Trash, in an export from elsewhere: not imported.
    Discarded,
}

impl Folder {
    fn name(self) -> &'static str {
        match self {
            Folder::Inbox => "INBOX",
            Folder::Sent => "Sent",
            Folder::Archive => "Archive",
            Folder::Discarded => "Trash",
        }
    }
}

/// The header an export writes and an import reads first.
pub const FOLDER_HEADER: &str = "X-Mailrs-Folder";

/// One message as it goes into an mbox: separator, two headers, the
/// message with `From ` lines quoted, and a blank line after it.
pub fn write_message(out: &mut Vec<u8>, raw: &[u8], at: i64, folder: Folder, seen: bool) {
    let when = chrono::DateTime::from_timestamp(at.max(0), 0).unwrap_or_default();
    out.extend_from_slice(b"From MAILER-DAEMON ");
    out.extend_from_slice(when.format("%a %b %e %H:%M:%S %Y").to_string().as_bytes());
    out.extend_from_slice(b"\n");
    out.extend_from_slice(format!("{FOLDER_HEADER}: {}\n", folder.name()).as_bytes());
    if seen {
        out.extend_from_slice(b"Status: RO\n");
    }
    for line in raw.split_inclusive(|b| *b == b'\n') {
        if is_from_line(unquoted(line)) {
            out.push(b'>');
        }
        out.extend_from_slice(line);
    }
    if !raw.ends_with(b"\n") {
        out.push(b'\n');
    }
    out.push(b'\n');
}

/// `line` without the `>` quoting in front of it.
fn unquoted(line: &[u8]) -> &[u8] {
    let n = line.iter().take_while(|b| **b == b'>').count();
    &line[n..]
}

fn is_from_line(line: &[u8]) -> bool {
    line.starts_with(b"From ")
}

/// One message read out of an mbox.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Parsed {
    /// The message, `From ` quoting undone and the export's own headers
    /// (`X-Mailrs-Folder`, `Status`, `X-Status`) removed.
    pub raw: Vec<u8>,
    pub folder: Folder,
    pub seen: bool,
}

/// Feed an mbox in chunks of any size; whole messages come out as soon as
/// the separator after them has been seen.
#[derive(Default)]
pub struct Splitter {
    pending: Vec<u8>,
    current: Option<Vec<u8>>,
}

impl Splitter {
    pub fn push(&mut self, chunk: &[u8]) -> Vec<Parsed> {
        self.pending.extend_from_slice(chunk);
        let mut out = Vec::new();
        while let Some(nl) = self.pending.iter().position(|b| *b == b'\n') {
            let line: Vec<u8> = self.pending.drain(..=nl).collect();
            self.line(line, &mut out);
        }
        out
    }

    /// The last message, and a final line with no newline after it.
    pub fn finish(mut self) -> Vec<Parsed> {
        let mut out = Vec::new();
        if !self.pending.is_empty() {
            let rest = std::mem::take(&mut self.pending);
            self.line(rest, &mut out);
        }
        if let Some(m) = self.current.take() {
            out.push(parse(m));
        }
        out
    }

    fn line(&mut self, line: Vec<u8>, out: &mut Vec<Parsed>) {
        if is_from_line(&line) {
            if let Some(m) = self.current.replace(Vec::new()) {
                out.push(parse(m));
            }
            return;
        }
        // text before the first separator is not a message
        let Some(m) = self.current.as_mut() else {
            return;
        };
        match unquoted(&line) {
            rest if is_from_line(rest) => m.extend_from_slice(&line[1..]),
            _ => m.extend_from_slice(&line),
        }
    }
}

/// Header block of `m`: its lines up to the first blank one.
fn header_end(m: &[u8]) -> usize {
    let mut at = 0;
    for line in m.split_inclusive(|b| *b == b'\n') {
        if line == b"\n" || line == b"\r\n" {
            return at;
        }
        at += line.len();
    }
    at
}

fn parse(mut m: Vec<u8>) -> Parsed {
    // the separator's blank line belongs to the mbox, not the message
    if m.ends_with(b"\n\n") {
        m.pop();
    }
    let end = header_end(&m);
    let mut kept = Vec::with_capacity(m.len());
    let mut folder: Option<Folder> = None;
    let mut seen = false;
    let mut labels = String::new();
    let mut skipping = false;
    for line in m[..end].split_inclusive(|b| *b == b'\n') {
        let continued = line.first().is_some_and(|b| *b == b' ' || *b == b'\t');
        if continued {
            if !skipping {
                kept.extend_from_slice(line);
            }
            continue;
        }
        let text = String::from_utf8_lossy(line);
        let (name, value) = text.split_once(':').unwrap_or((&text, ""));
        let value = value.trim();
        skipping = false;
        match name.to_ascii_lowercase().as_str() {
            "x-mailrs-folder" => {
                folder = Some(folder_named(value));
                skipping = true;
            }
            "status" | "x-status" => {
                seen |= value.contains('R');
                skipping = true;
            }
            "x-gmail-labels" => {
                labels = value.to_string();
                kept.extend_from_slice(line);
            }
            _ => kept.extend_from_slice(line),
        }
    }
    kept.extend_from_slice(&m[end..]);
    let (folder, seen) = match folder {
        Some(f) => (f, seen),
        None if labels.is_empty() => (Folder::Inbox, seen),
        None => gmail(&labels),
    };
    Parsed {
        raw: kept,
        folder,
        seen,
    }
}

fn folder_named(v: &str) -> Folder {
    match v.to_ascii_lowercase().as_str() {
        "sent" => Folder::Sent,
        "archive" => Folder::Archive,
        "trash" | "junk" => Folder::Discarded,
        _ => Folder::Inbox,
    }
}

/// Gmail Takeout: `X-Gmail-Labels: Inbox,Unread,Important`. Spam and the
/// Trash are left behind; mail in neither Inbox nor Sent was archived.
fn gmail(labels: &str) -> (Folder, bool) {
    let has = |want: &str| {
        labels
            .split(',')
            .any(|l| l.trim().trim_matches('"').eq_ignore_ascii_case(want))
    };
    let seen = !has("Unread");
    let folder = if has("Spam") || has("Trash") {
        Folder::Discarded
    } else if has("Sent") {
        Folder::Sent
    } else if has("Inbox") {
        Folder::Inbox
    } else {
        Folder::Archive
    };
    (folder, seen)
}

#[cfg(test)]
mod tests;

//! What a PDF's dictionaries name. Names are read where they stand in the
//! file; streams are not decompressed, so a name inside a compressed
//! object stream (`/ObjStm`) is not seen — a known miss.

use super::{Found, capped};
use crate::{Attachment, Limits};

fn is_pdf(b: &[u8]) -> bool {
    b.get(..1024).unwrap_or(b).windows(5).any(|w| w == b"%PDF-")
}

/// Every name (`/Name`) in the bytes, `#xx` escapes decoded.
fn names(b: &[u8]) -> impl Iterator<Item = String> + '_ {
    let delim = |c: u8| c.is_ascii_whitespace() || b"/()<>[]{}%".contains(&c);
    let mut i = 0;
    std::iter::from_fn(move || {
        while i < b.len() {
            if b[i] != b'/' {
                i += 1;
                continue;
            }
            let start = i + 1;
            let mut j = start;
            while j < b.len() && !delim(b[j]) {
                j += 1;
            }
            i = j;
            if j > start {
                return Some(decode(&b[start..j]));
            }
        }
        None
    })
}

fn decode(raw: &[u8]) -> String {
    let mut out = Vec::with_capacity(raw.len());
    let mut k = 0;
    while k < raw.len() {
        let hex = raw
            .get(k + 1..k + 3)
            .and_then(|h| std::str::from_utf8(h).ok());
        match (raw[k], hex.and_then(|h| u8::from_str_radix(h, 16).ok())) {
            (b'#', Some(v)) => {
                out.push(v);
                k += 3;
            }
            (c, _) => {
                out.push(c);
                k += 1;
            }
        }
    }
    String::from_utf8_lossy(&out).to_string()
}

/// File names given in `/F (…)` or `/UF (…)` that the machine would run.
fn executable_file_name(b: &[u8]) -> Option<String> {
    for key in [b"/UF".as_slice(), b"/F"] {
        let mut from = 0;
        while let Some(at) = b[from..].windows(key.len()).position(|w| w == key) {
            let rest = &b[from + at + key.len()..];
            let rest = rest
                .iter()
                .skip_while(|c| c.is_ascii_whitespace())
                .copied()
                .collect::<Vec<_>>();
            if rest.first() == Some(&b'(') {
                let end = rest.iter().position(|&c| c == b')').unwrap_or(rest.len());
                let name = String::from_utf8_lossy(&rest[1..end.min(rest.len())]).to_string();
                if super::names::executable(&name).is_some() {
                    return Some(name);
                }
            }
            from += at + key.len();
        }
    }
    None
}

/// A script, a launch action, or an embedded file the machine would run.
pub(crate) fn script(a: &Attachment<'_>, p: &Limits) -> Option<Found> {
    if !is_pdf(a.bytes) {
        return None;
    }
    let (b, truncated) = capped(a, p);
    let found = names(b)
        .find(|n| n == "JavaScript" || n == "JS" || n == "Launch")
        .map(|n| format!("/{n}"))
        .or_else(|| executable_file_name(b).map(|n| format!("an embedded {n}")))?;
    Some(Found {
        what: format!("{found} in {:?}", a.name),
        truncated,
    })
}

/// An embedded file or an open action: common in e-invoices on their own.
pub(crate) fn embedded(a: &Attachment<'_>, p: &Limits) -> Option<Found> {
    if !is_pdf(a.bytes) {
        return None;
    }
    let (b, truncated) = capped(a, p);
    let n = names(b).find(|n| n == "EmbeddedFile" || n == "OpenAction")?;
    Some(Found {
        what: format!("/{n} in {:?}", a.name),
        truncated,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn att(bytes: &[u8]) -> Attachment<'_> {
        Attachment {
            name: "a.pdf",
            declared_type: "application/pdf",
            bytes,
        }
    }

    #[test]
    fn scripts_and_launches_however_they_are_spelt() {
        let p = Limits::default();
        assert!(
            script(
                &att(b"%PDF-1.7\n1 0 obj<</S/JavaScript/JS(app.alert(1))>>"),
                &p
            )
            .is_some()
        );
        assert!(script(&att(b"%PDF-1.4\n<</Type/Action/S/Launch/F(cmd.exe)>>"), &p).is_some());
        assert!(script(&att(b"%PDF-1.4\n<</S/J#61vaScript>>"), &p).is_some());
        assert!(
            script(
                &att(b"%PDF-1.4\n<</Type/Filespec/F (setup.exe)/EF<</F 3 0 R>>>>"),
                &p
            )
            .is_some()
        );
        assert!(script(&att(b"%PDF-1.4\n<</Type/Page/Contents 4 0 R>>"), &p).is_none());
        assert!(script(&att(b"not a pdf /JavaScript"), &p).is_none());
    }

    #[test]
    fn an_e_invoice_is_only_noted() {
        let p = Limits::default();
        let invoice =
            b"%PDF-1.7\n<</Type/Filespec/F(factur-x.xml)/EF<</F 9 0 R>>>>\n<</Type/EmbeddedFile>>";
        assert!(script(&att(invoice), &p).is_none());
        assert!(embedded(&att(invoice), &p).is_some());
        assert!(embedded(&att(b"%PDF-1.7\n<</OpenAction 3 0 R>>"), &p).is_some());
    }
}

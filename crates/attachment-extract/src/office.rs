//! Text out of Word, Excel and PowerPoint files (Office Open XML), for the
//! attachment preview and for search.
//!
//! Only what a reader needs to tell what the file says: paragraphs of a
//! document, the rows of each sheet, the text of each slide. No layout, no
//! formulas, no images. The archive is untrusted: entries and the bytes read
//! from each are capped, so a file that inflates without end stops at the cap.

use std::io::Read;

use quick_xml::events::Event;

/// Which Office format a file is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OfficeKind {
    /// `.docx`
    Word,
    /// `.xlsx`
    Excel,
    /// `.pptx`
    PowerPoint,
}

/// One part of a file: a document's body, a sheet, a slide.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct Section {
    /// The sheet's name or the slide's number; empty for a document.
    pub title: String,
    /// Its lines. A sheet's cells are separated by tabs.
    pub lines: Vec<String>,
}

const MAX_ENTRIES: usize = 2_000;
const MAX_PART_BYTES: u64 = 16 * 1024 * 1024;
const MAX_SECTIONS: usize = 200;

/// The Office format named by a content type or, for a generic one, by the
/// file name's extension.
pub fn office_kind(content_type: &str, file_name: &str) -> Option<OfficeKind> {
    let ct = content_type.to_ascii_lowercase();
    if ct.contains("wordprocessingml.document") {
        return Some(OfficeKind::Word);
    }
    if ct.contains("spreadsheetml.sheet") {
        return Some(OfficeKind::Excel);
    }
    if ct.contains("presentationml.presentation") {
        return Some(OfficeKind::PowerPoint);
    }
    let name = file_name.to_ascii_lowercase();
    match name.rsplit_once('.').map(|(_, ext)| ext) {
        Some("docx") => Some(OfficeKind::Word),
        Some("xlsx") => Some(OfficeKind::Excel),
        Some("pptx") => Some(OfficeKind::PowerPoint),
        _ => None,
    }
}

/// The file's text, section by section.
pub fn office_text(kind: OfficeKind, bytes: &[u8]) -> Result<Vec<Section>, String> {
    let mut zip = zip::ZipArchive::new(std::io::Cursor::new(bytes))
        .map_err(|e| format!("not an Office file: {e}"))?;
    if zip.len() > MAX_ENTRIES {
        return Err(format!(
            "{} entries, more than an Office file has",
            zip.len()
        ));
    }
    match kind {
        OfficeKind::Word => {
            let xml = part(&mut zip, "word/document.xml")?.ok_or("no word/document.xml")?;
            Ok(vec![Section {
                title: String::new(),
                lines: paragraphs(&xml, "w:p", "w:t")?,
            }])
        }
        OfficeKind::PowerPoint => {
            let mut slides: Vec<(u32, String)> = numbered(&zip, "ppt/slides/slide", ".xml");
            slides.sort();
            let mut out = Vec::new();
            for (n, name) in slides.into_iter().take(MAX_SECTIONS) {
                let xml = part(&mut zip, &name)?.unwrap_or_default();
                out.push(Section {
                    title: n.to_string(),
                    lines: paragraphs(&xml, "a:p", "a:t")?,
                });
            }
            Ok(out)
        }
        OfficeKind::Excel => excel(&mut zip),
    }
}

type Archive<'a> = zip::ZipArchive<std::io::Cursor<&'a [u8]>>;

/// An entry's bytes, capped; `None` when the archive has no such entry.
fn part(zip: &mut Archive<'_>, name: &str) -> Result<Option<Vec<u8>>, String> {
    let file = match zip.by_name(name) {
        Ok(f) => f,
        Err(zip::result::ZipError::FileNotFound) => return Ok(None),
        Err(e) => return Err(format!("{name}: {e}")),
    };
    let mut out = Vec::new();
    file.take(MAX_PART_BYTES)
        .read_to_end(&mut out)
        .map_err(|e| format!("{name}: {e}"))?;
    Ok(Some(out))
}

/// Entries named `<prefix><n><suffix>`, with their numbers.
fn numbered(zip: &Archive<'_>, prefix: &str, suffix: &str) -> Vec<(u32, String)> {
    zip.file_names()
        .filter_map(|name| {
            let name = name.ok()?;
            let n = name
                .strip_prefix(prefix)?
                .strip_suffix(suffix)?
                .parse()
                .ok()?;
            Some((n, name.to_string()))
        })
        .collect()
}

/// The text runs (`run`) of each paragraph (`para`), one line per paragraph,
/// empty paragraphs dropped.
fn paragraphs(xml: &[u8], para: &str, run: &str) -> Result<Vec<String>, String> {
    let mut reader = quick_xml::Reader::from_reader(xml);
    let mut lines = Vec::new();
    let mut line = String::new();
    let mut in_run = false;
    let mut buf = Vec::new();
    loop {
        match reader
            .read_event_into(&mut buf)
            .map_err(|e| e.to_string())?
        {
            Event::Start(e) if e.name().as_ref() == run => in_run = true,
            Event::End(e) if e.name().as_ref() == run => in_run = false,
            Event::Empty(e) if e.name().as_ref() == "w:tab" => line.push('\t'),
            Event::Empty(e) if matches!(e.name().as_ref(), "w:br" | "a:br") => line.push(' '),
            Event::Text(t) if in_run => line.push_str(AsRef::<str>::as_ref(&t)),
            Event::GeneralRef(r) if in_run => {
                if let Some(c) = r.resolve_char_ref().map_err(|e| e.to_string())? {
                    line.push(c);
                } else {
                    line.push_str(entity(AsRef::<str>::as_ref(&r)));
                }
            }
            Event::End(e) if e.name().as_ref() == para => {
                let done = std::mem::take(&mut line);
                if !done.trim().is_empty() {
                    lines.push(done.trim().to_string());
                }
            }
            Event::Eof => break,
            _ => {}
        }
        buf.clear();
    }
    Ok(lines)
}

fn entity(name: &str) -> &'static str {
    match name {
        "amp" => "&",
        "lt" => "<",
        "gt" => ">",
        "quot" => "\"",
        "apos" => "'",
        _ => "",
    }
}

fn excel(zip: &mut Archive<'_>) -> Result<Vec<Section>, String> {
    let shared = match part(zip, "xl/sharedStrings.xml")? {
        Some(xml) => shared_strings(&xml)?,
        None => Vec::new(),
    };
    let names = match part(zip, "xl/workbook.xml")? {
        Some(xml) => sheet_names(&xml)?,
        None => Vec::new(),
    };
    let mut sheets = numbered(zip, "xl/worksheets/sheet", ".xml");
    sheets.sort();
    let mut out = Vec::new();
    for (i, (n, entry)) in sheets.into_iter().take(MAX_SECTIONS).enumerate() {
        let xml = part(zip, &entry)?.unwrap_or_default();
        let title = names.get(i).cloned().unwrap_or_else(|| format!("Sheet{n}"));
        out.push(Section {
            title,
            lines: rows(&xml, &shared)?,
        });
    }
    Ok(out)
}

/// The workbook's shared strings, in index order.
fn shared_strings(xml: &[u8]) -> Result<Vec<String>, String> {
    let mut reader = quick_xml::Reader::from_reader(xml);
    let mut out = Vec::new();
    let mut cur = String::new();
    let mut in_t = false;
    let mut buf = Vec::new();
    loop {
        match reader
            .read_event_into(&mut buf)
            .map_err(|e| e.to_string())?
        {
            Event::Start(e) if e.name().as_ref() == "t" => in_t = true,
            Event::End(e) if e.name().as_ref() == "t" => in_t = false,
            Event::Text(t) if in_t => cur.push_str(AsRef::<str>::as_ref(&t)),
            Event::GeneralRef(r) if in_t => cur.push_str(entity(AsRef::<str>::as_ref(&r))),
            Event::End(e) if e.name().as_ref() == "si" => out.push(std::mem::take(&mut cur)),
            Event::Eof => break,
            _ => {}
        }
        buf.clear();
    }
    Ok(out)
}

/// Sheet names in the workbook's order.
fn sheet_names(xml: &[u8]) -> Result<Vec<String>, String> {
    let mut reader = quick_xml::Reader::from_reader(xml);
    let mut out = Vec::new();
    let mut buf = Vec::new();
    loop {
        match reader
            .read_event_into(&mut buf)
            .map_err(|e| e.to_string())?
        {
            Event::Empty(e) | Event::Start(e) if e.name().as_ref() == "sheet" => {
                if let Some(a) = e.try_get_attribute("name").map_err(|e| e.to_string())? {
                    out.push(
                        a.normalized_value(quick_xml::XmlVersion::Implicit1_0)
                            .map_err(|e| e.to_string())?
                            .into_owned(),
                    );
                }
            }
            Event::Eof => break,
            _ => {}
        }
        buf.clear();
    }
    Ok(out)
}

/// A sheet's rows, cells joined by tabs; empty rows dropped.
fn rows(xml: &[u8], shared: &[String]) -> Result<Vec<String>, String> {
    let mut reader = quick_xml::Reader::from_reader(xml);
    let mut out = Vec::new();
    let mut row: Vec<String> = Vec::new();
    let mut cell_type = String::new();
    let mut value = String::new();
    let mut in_value = false;
    let mut buf = Vec::new();
    loop {
        match reader
            .read_event_into(&mut buf)
            .map_err(|e| e.to_string())?
        {
            Event::Start(e) if e.name().as_ref() == "c" => {
                cell_type = match e.try_get_attribute("t").map_err(|e| e.to_string())? {
                    Some(a) => a.value.into_owned(),
                    None => String::new(),
                };
                value.clear();
            }
            Event::Start(e) if matches!(e.name().as_ref(), "v" | "t") => in_value = true,
            Event::End(e) if matches!(e.name().as_ref(), "v" | "t") => in_value = false,
            Event::Text(t) if in_value => value.push_str(AsRef::<str>::as_ref(&t)),
            Event::End(e) if e.name().as_ref() == "c" => {
                row.push(cell_text(&cell_type, &value, shared));
            }
            Event::End(e) if e.name().as_ref() == "row" => {
                let cells = std::mem::take(&mut row);
                if cells.iter().any(|c| !c.trim().is_empty()) {
                    out.push(cells.join("\t"));
                }
            }
            Event::Eof => break,
            _ => {}
        }
        buf.clear();
    }
    Ok(out)
}

fn cell_text(cell_type: &str, value: &str, shared: &[String]) -> String {
    match cell_type {
        "s" => value
            .trim()
            .parse::<usize>()
            .ok()
            .and_then(|i| shared.get(i))
            .cloned()
            .unwrap_or_default(),
        "b" => match value.trim() {
            "1" => "TRUE".into(),
            _ => "FALSE".into(),
        },
        _ => value.to_string(),
    }
}

#[cfg(test)]
mod tests;

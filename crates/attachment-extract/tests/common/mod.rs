//! builders for small synthetic pdfs and truetype programs

#![allow(
    dead_code,
    reason = "each test binary uses a different subset of the builders"
)]

use lopdf::{Dictionary, Document, Object, ObjectId, Stream, dictionary};

pub struct Builder {
    pub doc: Document,
    pages: ObjectId,
    kids: Vec<Object>,
}

impl Builder {
    pub fn new() -> Self {
        let mut doc = Document::with_version("1.5");
        let pages = doc.new_object_id();
        Builder {
            doc,
            pages,
            kids: Vec::new(),
        }
    }

    pub fn add(&mut self, obj: impl Into<Object>) -> ObjectId {
        self.doc.add_object(obj)
    }

    pub fn stream(&mut self, dict: Dictionary, data: &[u8]) -> ObjectId {
        self.doc.add_object(Stream::new(dict, data.to_vec()))
    }

    /// A page whose resources hold `font` as `/F1`.
    pub fn page_with_font(&mut self, font: ObjectId, content: &[u8]) -> ObjectId {
        let resources = dictionary! { "Font" => dictionary! { "F1" => font } };
        self.page(resources, content)
    }

    pub fn page(&mut self, resources: Dictionary, content: &[u8]) -> ObjectId {
        let contents = self.stream(Dictionary::new(), content);
        let page = self.add(dictionary! {
            "Type" => "Page",
            "Parent" => self.pages,
            "MediaBox" => vec![0.into(), 0.into(), 612.into(), 792.into()],
            "Resources" => resources,
            "Contents" => contents,
        });
        self.kids.push(page.into());
        page
    }

    pub fn finish(mut self) -> Vec<u8> {
        let count = self.kids.len() as i64;
        let pages =
            dictionary! { "Type" => "Pages", "Kids" => self.kids.clone(), "Count" => count };
        self.doc
            .objects
            .insert(self.pages, Object::Dictionary(pages));
        let catalog = self.add(dictionary! { "Type" => "Catalog", "Pages" => self.pages });
        self.doc.trailer.set("Root", catalog);
        let mut out = Vec::new();
        self.doc.save_to(&mut out).expect("serialise pdf");
        out
    }
}

/// A standard 14 font without widths or encoding.
pub fn helvetica(b: &mut Builder) -> ObjectId {
    b.add(dictionary! { "Type" => "Font", "Subtype" => "Type1", "BaseFont" => "Helvetica" })
}

pub fn text(pdf: &[u8]) -> String {
    mailrs_attachment_extract::extract_pdf_text(pdf)
        .expect("extract")
        .text
}

/// `(platform, encoding, [(code, glyph id)])`
pub type Subtable = (u16, u16, Vec<(u16, u16)>);

/// A truetype program with only a `cmap` table, every subtable written as format 4.
pub fn truetype(subtables: &[Subtable]) -> Vec<u8> {
    let records: Vec<(u16, u16, Vec<u8>)> = subtables
        .iter()
        .map(|(p, e, pairs)| (*p, *e, format4(pairs)))
        .collect();
    truetype_raw(&records)
}

fn format4(pairs: &[(u16, u16)]) -> Vec<u8> {
    let mut segs: Vec<(u16, u16)> = pairs.to_vec();
    segs.push((0xFFFF, 0));
    let n = segs.len() as u16;
    let mut t = Vec::new();
    for v in [4u16, 16 + 8 * n, 0, 2 * n, 0, 0, 0] {
        t.extend(v.to_be_bytes());
    }
    segs.iter().for_each(|(c, _)| t.extend(c.to_be_bytes()));
    t.extend(0u16.to_be_bytes());
    segs.iter().for_each(|(c, _)| t.extend(c.to_be_bytes()));
    segs.iter()
        .for_each(|(c, g)| t.extend(g.wrapping_sub(*c).to_be_bytes()));
    segs.iter().for_each(|_| t.extend(0u16.to_be_bytes()));
    t
}

/// A truetype program whose `cmap` holds the given raw subtables.
pub fn truetype_raw(records: &[(u16, u16, Vec<u8>)]) -> Vec<u8> {
    let mut cmap = Vec::new();
    cmap.extend(0u16.to_be_bytes());
    cmap.extend((records.len() as u16).to_be_bytes());
    let mut offset = 4 + 8 * records.len() as u32;
    for (p, e, body) in records {
        cmap.extend(p.to_be_bytes());
        cmap.extend(e.to_be_bytes());
        cmap.extend(offset.to_be_bytes());
        offset += body.len() as u32;
    }
    records.iter().for_each(|(_, _, b)| cmap.extend(b));
    let mut font = Vec::new();
    font.extend(0x0001_0000u32.to_be_bytes());
    for v in [1u16, 16, 0, 0] {
        font.extend(v.to_be_bytes());
    }
    font.extend(b"cmap");
    font.extend(0u32.to_be_bytes());
    font.extend(28u32.to_be_bytes());
    font.extend((cmap.len() as u32).to_be_bytes());
    font.extend(cmap);
    font
}

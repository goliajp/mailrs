//! hostile pdfs: none of these may panic or run away; text that is still
//! reachable comes out

mod common;

use common::{Builder, helvetica, text, truetype, truetype_raw};
use lopdf::{Object, ObjectId, dictionary};
use mailrs_attachment_extract::extract_pdf_text;

fn sample() -> Vec<u8> {
    let mut b = Builder::new();
    let font = helvetica(&mut b);
    b.page_with_font(font, b"BT /F1 12 Tf 72 700 Td (first page) Tj ET");
    b.page_with_font(font, b"BT /F1 12 Tf 72 700 Td [(second) -400 (page)] TJ ET");
    b.finish()
}

#[test]
fn self_referencing_form_and_reference_loops() {
    let mut b = Builder::new();
    let font = helvetica(&mut b);
    let form_id: ObjectId = b.doc.new_object_id();
    let looped: ObjectId = b.doc.new_object_id();
    b.doc.objects.insert(looped, Object::Reference(looped));
    let form = lopdf::Stream::new(
        dictionary! {
            "Type" => "XObject", "Subtype" => "Form",
            "Resources" => dictionary! { "XObject" => dictionary! { "Me" => form_id }, "Font" => dictionary! { "F1" => font } },
            "Matrix" => looped,
        },
        b"BT /F1 10 Tf (again) Tj ET /Me Do".to_vec(),
    );
    b.doc.objects.insert(form_id, Object::Stream(form));
    let resources = dictionary! {
        "Font" => dictionary! { "F1" => font, "F2" => looped },
        "XObject" => dictionary! { "X" => form_id },
    };
    b.page(
        resources,
        b"BT /F2 10 Tf (lost) Tj /F1 10 Tf (kept) Tj ET q 1 0 0 1 0 -100 cm /X Do Q",
    );
    assert_eq!(text(&b.finish()), "kept\nagain");
}

#[test]
fn page_tree_cycle_through_parent() {
    let mut b = Builder::new();
    let font = helvetica(&mut b);
    let page = b.page_with_font(font, b"BT /F1 12 Tf 72 700 Td (cycle) Tj ET");
    let bytes = b.finish();
    let mut doc = lopdf::Document::load_mem(&bytes).expect("reload");
    let pages = doc
        .get_dictionary(page)
        .expect("page")
        .get(b"Parent")
        .expect("parent")
        .as_reference()
        .expect("ref");
    doc.get_dictionary_mut(pages)
        .expect("pages")
        .set("Parent", page);
    let mut out = Vec::new();
    doc.save_to(&mut out).expect("save");
    let _ = extract_pdf_text(&out);
}

#[test]
fn absurd_ranges_and_counts() {
    let mut b = Builder::new();
    let cmap = b"1 begincodespacerange <00000000> <FFFFFFFF> endcodespacerange
1 beginbfrange <00000000> <FFFFFFFF> <0041> endbfrange";
    let tu = b.stream(dictionary! {}, cmap);
    // a format 12 group claiming every code point
    let group12: Vec<u8> = [
        &[0u8, 12, 0, 0, 0, 0, 0, 28, 0, 0, 0, 0, 0, 0, 0, 1][..],
        &[0, 0, 0, 0, 0, 0x10, 0xFF, 0xFF, 0, 0, 0, 1],
    ]
    .concat();
    let huge = truetype_raw(&[(3, 10, group12), (3, 0, vec![0, 4, 0xFF, 0xFF])]);
    let file = b.stream(dictionary! {}, &huge);
    let desc = b.add(dictionary! { "Type" => "FontDescriptor", "Flags" => 4, "FontFile2" => file, "MissingWidth" => 1e30 });
    let cid = b.add(dictionary! {
        "Type" => "Font", "Subtype" => "CIDFontType2", "FontDescriptor" => desc,
        "W" => vec![0.into(), 4_294_967_295i64.into(), 500.into(), (-5).into(), vec![Object::Null].into()],
    });
    let font = b.add(dictionary! {
        "Type" => "Font", "Subtype" => "Type0", "Encoding" => "Identity-H",
        "DescendantFonts" => vec![cid.into()], "ToUnicode" => tu,
    });
    let simple = b.add(dictionary! {
        "Type" => "Font", "Subtype" => "Type1",
        "Encoding" => dictionary! { "Differences" => vec![1_099_511_627_776i64.into(), Object::Name(b"A".to_vec())] },
        "FirstChar" => -7, "Widths" => vec![Object::Null],
    });
    let resources = dictionary! { "Font" => dictionary! { "F1" => font, "F2" => simple } };
    b.page(
        resources,
        b"BT /F1 12 Tf 99999 Tz 1e300 TL <00000001> Tj T* /F2 1e308 Tf (B) Tj ET",
    );
    let t = text(&b.finish());
    assert!(t.starts_with('A'), "{t:?}");
}

#[test]
fn deep_save_nesting_and_operator_floods() {
    let mut b = Builder::new();
    let font = helvetica(&mut b);
    let mut content = "q ".repeat(200_000);
    content.push_str("BT /F1 12 Tf (deep) Tj ET ");
    content.push_str(&"Q ".repeat(300_000));
    content.push_str(&"0 0 Td ".repeat(100_000));
    b.page_with_font(font, content.as_bytes());
    assert_eq!(text(&b.finish()), "deep");
}

#[test]
fn absurd_stream_length_and_missing_xref() {
    let pdf = b"%PDF-1.4
1 0 obj << /Type /Catalog /Pages 2 0 R >> endobj
2 0 obj << /Type /Pages /Kids [3 0 R] /Count 1 >> endobj
3 0 obj << /Type /Page /Parent 2 0 R /Contents 4 0 R /Resources << /Font << /F1 5 0 R >> >> >> endobj
4 0 obj << /Length 99999999999 >> stream
BT /F1 12 Tf (Hi) Tj ET
endstream endobj
5 0 obj << /Type /Font /Subtype /Type1 /BaseFont /Helvetica >> endobj
trailer << /Root 1 0 R >>
%%EOF";
    let _ = extract_pdf_text(pdf);
}

#[test]
fn every_truncation_of_a_valid_file() {
    let pdf = sample();
    for end in (0..pdf.len()).step_by(7) {
        let _ = extract_pdf_text(&pdf[..end]);
    }
}

#[test]
fn deterministic_byte_corruption() {
    let pdf = sample();
    let mut seed: u64 = 0x2545_F491_4F6C_DD1D;
    for _ in 0..400 {
        let mut bytes = pdf.clone();
        for _ in 0..8 {
            seed ^= seed << 13;
            seed ^= seed >> 7;
            seed ^= seed << 17;
            let at = (seed % bytes.len() as u64) as usize;
            bytes[at] = (seed >> 32) as u8;
        }
        let _ = extract_pdf_text(&bytes);
    }
}

#[test]
fn truncated_content_and_font_streams() {
    let mut b = Builder::new();
    let tu = b.stream(dictionary! {}, b"1 beginbfchar <01> <00");
    let program = truetype(&[(3, 0, vec![(0xF041, 5)])]);
    let file = b.stream(dictionary! {}, &program[..program.len() - 9]);
    let desc = b.add(dictionary! { "Type" => "FontDescriptor", "Flags" => 4, "FontFile2" => file });
    let font = b.add(dictionary! {
        "Type" => "Font", "Subtype" => "TrueType", "FontDescriptor" => desc, "ToUnicode" => tu,
    });
    b.page_with_font(font, b"BT /F1 12 Tf (A\\001) Tj [(unterminated");
    let _ = text(&b.finish());
}

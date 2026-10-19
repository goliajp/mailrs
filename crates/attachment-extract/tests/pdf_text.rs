//! pdf text extraction on synthetic documents, one mapping path per test

mod common;

use common::{Builder, helvetica, text, truetype};
use lopdf::{Object, dictionary};

#[test]
fn to_unicode_bfchar_bfrange_and_arrays() {
    let mut b = Builder::new();
    let cmap = b"/CIDInit /ProcSet findresource begin 12 dict begin begincmap
1 begincodespacerange <00> <FF> endcodespacerange
2 beginbfchar <01> <0048> <02> /i endbfchar
2 beginbfrange <03> <05> <0041> <06> <07> [<0058> <00590059>] endbfrange
endcmap CMapName currentdict /CMap defineresource pop end end";
    let tu = b.stream(dictionary! {}, cmap);
    let font = b.add(dictionary! {
        "Type" => "Font", "Subtype" => "Type1", "BaseFont" => "Custom", "ToUnicode" => tu,
    });
    b.page_with_font(
        font,
        b"BT /F1 12 Tf 72 700 Td (\\001\\002\\003\\004\\005\\006\\007) Tj ET",
    );
    assert_eq!(text(&b.finish()), "HiABCXYY");
}

#[test]
fn differences_through_the_glyph_list() {
    let mut b = Builder::new();
    let enc = dictionary! {
        "Type" => "Encoding",
        "BaseEncoding" => "WinAnsiEncoding",
        "Differences" => vec![
            65.into(), Object::Name(b"uni3042".to_vec()), Object::Name(b"afii10017".to_vec()),
            Object::Name(b"f_i".to_vec()), Object::Name(b"eacute".to_vec()), Object::Name(b"g17".to_vec()),
        ],
    };
    let font = b.add(dictionary! {
        "Type" => "Font", "Subtype" => "Type1", "BaseFont" => "Garbled", "Encoding" => enc,
    });
    b.page_with_font(font, b"BT /F1 12 Tf 72 700 Td (ABCDEFG\\200) Tj ET");
    // E names a glyph without meaning; F, G and 0x80 keep their WinAnsi meaning
    assert_eq!(text(&b.finish()), "あАfiéFG\u{20ac}");
}

#[test]
fn identity_h_cid_font_with_to_unicode() {
    let mut b = Builder::new();
    let cmap = b"begincmap 1 begincodespacerange <0000> <FFFF> endcodespacerange
3 beginbfchar <0001> <65E5> <0002> <672C> <0003> <D83DDE00> endbfchar
1 beginbfrange <0010> <0011> <3042> endbfrange endcmap";
    let tu = b.stream(dictionary! {}, cmap);
    let cid = b.add(dictionary! {
        "Type" => "Font", "Subtype" => "CIDFontType2", "BaseFont" => "Sub",
        "CIDSystemInfo" => dictionary! { "Registry" => Object::string_literal("Adobe"), "Ordering" => Object::string_literal("Identity"), "Supplement" => 0 },
        "DW" => 1000,
    });
    let font = b.add(dictionary! {
        "Type" => "Font", "Subtype" => "Type0", "BaseFont" => "Sub", "Encoding" => "Identity-H",
        "DescendantFonts" => vec![cid.into()], "ToUnicode" => tu,
    });
    b.page_with_font(
        font,
        b"BT /F1 12 Tf 72 700 Td <0001000200030010 0011> Tj ET",
    );
    assert_eq!(text(&b.finish()), "日本\u{1F600}あぃ");
}

#[test]
fn predefined_cjk_cmap_without_to_unicode() {
    let mut b = Builder::new();
    let cid = b
        .add(dictionary! { "Type" => "Font", "Subtype" => "CIDFontType0", "BaseFont" => "Mincho" });
    let font = b.add(dictionary! {
        "Type" => "Font", "Subtype" => "Type0", "BaseFont" => "Mincho", "Encoding" => "90ms-RKSJ-H",
        "DescendantFonts" => vec![cid.into()],
    });
    b.page_with_font(font, b"BT /F1 12 Tf 72 700 Td <82A082A2> Tj ET");
    assert_eq!(text(&b.finish()), "あい");
}

#[test]
fn japan1_cids_without_to_unicode() {
    let mut b = Builder::new();
    let cid = b.add(dictionary! {
        "Type" => "Font", "Subtype" => "CIDFontType0", "BaseFont" => "Gothic",
        "CIDSystemInfo" => dictionary! { "Registry" => Object::string_literal("Adobe"), "Ordering" => Object::string_literal("Japan1"), "Supplement" => 6 },
    });
    let font = b.add(dictionary! {
        "Type" => "Font", "Subtype" => "Type0", "BaseFont" => "Gothic", "Encoding" => "Identity-H",
        "DescendantFonts" => vec![cid.into()],
    });
    // cids 843 and 845 are hiragana a and i in adobe-japan1
    b.page_with_font(font, b"BT /F1 12 Tf 72 700 Td <034B034D> Tj ET");
    assert_eq!(text(&b.finish()), "あい");
}

fn truetype_font(b: &mut Builder) -> lopdf::ObjectId {
    // symbol cmap: codes 0xF041/0xF042 -> glyphs 5/6; unicode cmap: glyphs 5/6 are Ж/З
    let program = truetype(&[
        (3, 0, vec![(0xF041, 5), (0xF042, 6)]),
        (3, 1, vec![(0x0416, 5), (0x0417, 6)]),
    ]);
    b.stream(dictionary! { "Length1" => program.len() as i64 }, &program)
}

#[test]
fn truetype_cmap_for_a_symbolic_simple_font() {
    let mut b = Builder::new();
    let file = truetype_font(&mut b);
    let desc = b.add(dictionary! { "Type" => "FontDescriptor", "FontName" => "Sym", "Flags" => 4, "FontFile2" => file });
    let font = b.add(dictionary! {
        "Type" => "Font", "Subtype" => "TrueType", "BaseFont" => "Sym", "FontDescriptor" => desc,
        "FirstChar" => 65, "LastChar" => 66, "Widths" => vec![600.into(), 600.into()],
    });
    b.page_with_font(font, b"BT /F1 12 Tf 72 700 Td (AB) Tj ET");
    assert_eq!(text(&b.finish()), "ЖЗ");
}

#[test]
fn truetype_cmap_for_an_identity_cid_font() {
    let mut b = Builder::new();
    let file = truetype_font(&mut b);
    let desc = b.add(dictionary! { "Type" => "FontDescriptor", "FontName" => "Sub", "Flags" => 4, "FontFile2" => file });
    let cid = b.add(dictionary! {
        "Type" => "Font", "Subtype" => "CIDFontType2", "BaseFont" => "Sub", "FontDescriptor" => desc,
        "CIDToGIDMap" => "Identity",
    });
    let font = b.add(dictionary! {
        "Type" => "Font", "Subtype" => "Type0", "BaseFont" => "Sub", "Encoding" => "Identity-H",
        "DescendantFonts" => vec![cid.into()],
    });
    b.page_with_font(font, b"BT /F1 12 Tf 72 700 Td <00050006> Tj ET");
    assert_eq!(text(&b.finish()), "ЖЗ");
}

#[test]
fn spaces_from_tj_offsets_and_lines_from_baselines() {
    let mut b = Builder::new();
    let font = helvetica(&mut b);
    let content = b"BT /F1 10 Tf 72 700 Td [(Hel) -20 (lo) -300 (World)] TJ 0 -14 Td (Next) Tj \
        T* 20 TL (leading) ' ET";
    b.page_with_font(font, content);
    assert_eq!(text(&b.finish()), "Hello World\nNext\nleading");
}

#[test]
fn word_and_char_spacing_operators() {
    let mut b = Builder::new();
    let font = helvetica(&mut b);
    b.page_with_font(
        font,
        b"BT /F1 10 Tf 72 700 Td 2 Tc (ab) Tj 0 Tc 30 Tw (c d) Tj ET",
    );
    assert_eq!(text(&b.finish()), "a b c d");
}

#[test]
fn form_xobjects_are_followed() {
    let mut b = Builder::new();
    let font = helvetica(&mut b);
    let form = b.stream(
        dictionary! { "Type" => "XObject", "Subtype" => "Form", "BBox" => vec![0.into(), 0.into(), 100.into(), 100.into()] },
        b"BT /F1 10 Tf 0 0 Td (inside) Tj ET",
    );
    let resources = dictionary! { "Font" => dictionary! { "F1" => font }, "XObject" => dictionary! { "X1" => form } };
    b.page(resources, b"q 1 0 0 1 72 600 cm /X1 Do Q");
    assert_eq!(text(&b.finish()), "inside");
}

#[test]
fn pages_are_separated_by_form_feeds() {
    let mut b = Builder::new();
    let font = helvetica(&mut b);
    for t in ["one", "two", "three"] {
        b.page_with_font(
            font,
            format!("BT /F1 12 Tf 72 700 Td ({t}) Tj ET").as_bytes(),
        );
    }
    let r = mailrs_attachment_extract::extract_pdf_text(&b.finish()).expect("extract");
    assert_eq!(r.text, "one\u{000C}two\u{000C}three");
    assert_eq!(r.page_count, Some(3));
}

/// A page without text keeps its place, even at the start where the
/// joined text would trim it away.
#[test]
fn per_page_text_keeps_empty_pages_in_place() {
    let mut b = Builder::new();
    let font = helvetica(&mut b);
    b.page_with_font(font, b"");
    b.page_with_font(font, b"BT /F1 12 Tf 72 700 Td (two) Tj ET");
    b.page_with_font(font, b"");
    let pages = mailrs_attachment_extract::extract_pdf_pages(&b.finish()).expect("extract");
    assert_eq!(pages, vec!["", "two", ""]);
}

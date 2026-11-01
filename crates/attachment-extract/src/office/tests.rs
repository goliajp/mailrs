use std::io::Write;

use super::*;

fn archive(entries: &[(&str, &str)]) -> Vec<u8> {
    let mut out = std::io::Cursor::new(Vec::new());
    let mut zip = zip::ZipWriter::new(&mut out);
    let opts =
        zip::write::SimpleFileOptions::default().compression_method(zip::CompressionMethod::Stored);
    for (name, body) in entries {
        zip.start_file(*name, opts).unwrap();
        zip.write_all(body.as_bytes()).unwrap();
    }
    zip.finish().unwrap();
    out.into_inner()
}

#[test]
fn a_document_reads_as_its_paragraphs() {
    let doc = archive(&[(
        "word/document.xml",
        r#"<w:document><w:body>
<w:p><w:r><w:t>Invoice </w:t></w:r><w:r><w:t>#42</w:t></w:r></w:p>
<w:p></w:p>
<w:p><w:r><w:t>Total</w:t></w:r><w:r><w:tab/><w:t>1,200 &amp; tax</w:t></w:r></w:p>
</w:body></w:document>"#,
    )]);
    let s = office_text(OfficeKind::Word, &doc).unwrap();
    assert_eq!(s.len(), 1);
    assert_eq!(s[0].lines, ["Invoice #42", "Total\t1,200 & tax"]);
}

#[test]
fn a_workbook_reads_as_named_sheets_of_rows() {
    let book = archive(&[
        (
            "xl/workbook.xml",
            r#"<workbook><sheets><sheet name="Q1" sheetId="1"/><sheet name="Q2" sheetId="2"/></sheets></workbook>"#,
        ),
        (
            "xl/sharedStrings.xml",
            r#"<sst><si><t>Item</t></si><si><t>Paid</t></si></sst>"#,
        ),
        (
            "xl/worksheets/sheet1.xml",
            r#"<worksheet><sheetData>
<row><c t="s"><v>0</v></c><c t="s"><v>1</v></c></row>
<row><c></c></row>
<row><c t="inlineStr"><is><t>Rent</t></is></c><c t="b"><v>1</v></c><c><v>950.5</v></c></row>
</sheetData></worksheet>"#,
        ),
        (
            "xl/worksheets/sheet2.xml",
            r#"<worksheet><sheetData/></worksheet>"#,
        ),
    ]);
    let s = office_text(OfficeKind::Excel, &book).unwrap();
    assert_eq!(s.len(), 2);
    assert_eq!(s[0].title, "Q1");
    assert_eq!(s[0].lines, ["Item\tPaid", "Rent\tTRUE\t950.5"]);
    assert_eq!(s[1].title, "Q2");
    assert!(s[1].lines.is_empty());
}

#[test]
fn slides_come_in_number_order() {
    let slide = |text: &str| format!(r#"<p:sld><a:p><a:r><a:t>{text}</a:t></a:r></a:p></p:sld>"#);
    let (two, ten) = (slide("Two"), slide("Ten"));
    let deck = archive(&[
        ("ppt/slides/slide10.xml", &ten),
        ("ppt/slides/slide2.xml", &two),
    ]);
    let s = office_text(OfficeKind::PowerPoint, &deck).unwrap();
    let titles: Vec<&str> = s.iter().map(|x| x.title.as_str()).collect();
    assert_eq!(titles, ["2", "10"]);
    assert_eq!(s[0].lines, ["Two"]);
}

#[test]
fn what_is_not_an_office_file_says_so() {
    assert!(office_text(OfficeKind::Word, b"%PDF-1.7 not a zip").is_err());
    let empty = archive(&[("readme.txt", "hi")]);
    assert!(office_text(OfficeKind::Word, &empty).is_err());
    // a workbook with no sheets is empty, not an error
    assert!(office_text(OfficeKind::Excel, &empty).unwrap().is_empty());
}

#[test]
fn an_archive_with_too_many_entries_is_refused() {
    let names: Vec<String> = (0..=MAX_ENTRIES).map(|i| format!("e{i}")).collect();
    let entries: Vec<(&str, &str)> = names.iter().map(|n| (n.as_str(), "")).collect();
    assert!(office_text(OfficeKind::Word, &archive(&entries)).is_err());
}

#[test]
fn the_kind_comes_from_the_content_type_or_else_the_name() {
    let docx = "application/vnd.openxmlformats-officedocument.wordprocessingml.document";
    assert_eq!(office_kind(docx, ""), Some(OfficeKind::Word));
    assert_eq!(
        office_kind("application/octet-stream", "Report.XLSX"),
        Some(OfficeKind::Excel)
    );
    assert_eq!(
        office_kind("application/octet-stream", "deck.pptx"),
        Some(OfficeKind::PowerPoint)
    );
    assert_eq!(office_kind("application/pdf", "x.pdf"), None);
    assert_eq!(office_kind("application/msword", "old.doc"), None);
}

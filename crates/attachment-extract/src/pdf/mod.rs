//! pdf text extraction on top of lopdf's object, stream and content parsing
//!
//! attachments are untrusted, so every structure walk is bounded: stream
//! sizes, form nesting, operators per document and output length.

mod cjk;
mod cmap;
mod cmap_lex;
mod encodings;
mod font;
mod font_cid;
mod font_simple;
mod glyphname;
mod glyphs;
mod interp;
mod japan1;
mod layout;
mod macnames;
mod matrix;
mod objs;
mod truetype;
mod type1;

use lopdf::{Document, LoadOptions};

/// Text of every page, pages separated by `'\u{000C}'`.
pub(crate) fn extract_text(data: &[u8]) -> Result<String, String> {
    let options = LoadOptions {
        max_decompressed_size: Some(objs::MAX_STREAM),
        ..LoadOptions::default()
    };
    let doc = Document::load_mem_with_options(data, options).map_err(|e| e.to_string())?;
    if doc.trailer.get(b"Encrypt").is_ok() && !doc.was_encrypted() {
        return Err("encrypted with a password".to_string());
    }
    let mut interp = interp::Interp::new(&doc);
    for (i, page) in doc.page_iter().enumerate() {
        if i > 0 {
            interp.sink.page_break();
        }
        interp.run_page(page);
    }
    Ok(interp.sink.out)
}

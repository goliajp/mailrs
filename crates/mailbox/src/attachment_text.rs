/// What one extraction pass made of a message's attachments.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct AttachmentTextState {
    /// Extractor generation that wrote this. Rows from before the field
    /// existed read as 1: PDF text only, no OCR.
    pub version: u32,
    /// Attachments considered (PDFs and images, in MIME order). Rows are
    /// numbered by position among these.
    pub parts: u32,
    /// Of those, PDFs.
    pub pdfs: u32,
    /// Of those, images.
    pub images: u32,
    /// Attachments whose text was stored.
    pub stored: u32,
    /// Of the stored, how many were read by OCR.
    pub ocr: u32,
    /// Reading failed: unreadable file, a panic, or OCR gave up.
    pub failed: u32,
    /// Skipped without reading, over the size limit.
    pub too_large: u32,
    /// Images too small to carry text worth reading.
    pub too_small: u32,
    /// Stored, but cut at the per-attachment cap.
    pub truncated: u32,
}

impl AttachmentTextState {
    /// Every field by name, in one fixed order: what a store writes.
    pub fn fields(&self) -> [(&'static str, u32); 10] {
        [
            ("version", self.version),
            ("parts", self.parts),
            ("pdfs", self.pdfs),
            ("images", self.images),
            ("stored", self.stored),
            ("ocr", self.ocr),
            ("failed", self.failed),
            ("too_large", self.too_large),
            ("too_small", self.too_small),
            ("truncated", self.truncated),
        ]
    }

    /// Set the field `name` names; an unknown name changes nothing.
    pub fn set(&mut self, name: &str, v: u32) {
        let slot = match name {
            "version" => &mut self.version,
            "parts" => &mut self.parts,
            "pdfs" => &mut self.pdfs,
            "images" => &mut self.images,
            "stored" => &mut self.stored,
            "ocr" => &mut self.ocr,
            "failed" => &mut self.failed,
            "too_large" => &mut self.too_large,
            "too_small" => &mut self.too_small,
            "truncated" => &mut self.truncated,
            _ => return,
        };
        *slot = v;
    }
}

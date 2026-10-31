//! Which parts of a message are files: attachments, and the inline
//! resources a body refers to by Content-ID.

use crate::Part;

impl<'a> Part<'a> {
    /// Iterate over leaf parts marked as attachments. A part is an
    /// "attachment" when its `Content-Disposition: attachment` OR
    /// it has a `filename` parameter in either Content-Type or
    /// Content-Disposition. (Some mailers omit the disposition
    /// header but still set a filename.)
    pub fn attachments(&self) -> impl Iterator<Item = &Part<'a>> {
        self.walk().filter(|p| p.is_attachment())
    }

    /// How many attachments a reader is shown: [`attachments`](Self::attachments)
    /// less the images the HTML body draws in place by `cid:`, which the
    /// web hides from the attachment list for the same reason — they are
    /// already on screen, often a signature's logo.
    pub fn shown_attachment_count(&self) -> usize {
        let html: String = self
            .walk()
            .filter(|p| p.content_type.type_ == "text" && p.content_type.subtype == "html")
            .filter_map(|p| p.body_text())
            .collect::<Vec<_>>()
            .join("\n")
            .to_lowercase();
        self.attachments()
            .filter(|p| match &p.content_id {
                Some(id) => {
                    let id = id.trim().trim_start_matches('<').trim_end_matches('>');
                    id.is_empty() || !html.contains(&format!("cid:{}", id.to_lowercase()))
                }
                None => true,
            })
            .count()
    }

    /// Every leaf part a reader can open by index: the attachments, then
    /// the inline resources a body points at only by Content-ID (an image
    /// in `multipart/related` with no filename). Attachments come first so
    /// their indexes are the same as in [`attachments`](Self::attachments).
    pub fn files(&self) -> impl Iterator<Item = &Part<'a>> {
        self.attachments()
            .chain(self.walk().filter(|p| p.is_inline_resource()))
    }

    /// A leaf part that is not an attachment but is referenced by
    /// Content-ID, and is not one of the text bodies.
    pub fn is_inline_resource(&self) -> bool {
        self.children.is_empty()
            && !self.is_attachment()
            && self.content_id.is_some()
            && self.content_type.type_ != "text"
            && self.content_type.type_ != "multipart"
    }

    /// Returns the attachment's filename if available — checks
    /// Content-Disposition `filename=` first, falls back to
    /// Content-Type `name=`.
    pub fn attachment_filename(&self) -> Option<&str> {
        if let Some(d) = &self.disposition
            && let Some(f) = d.filename()
        {
            return Some(f);
        }
        self.content_type.name()
    }

    /// Whether this part qualifies as an attachment (per
    /// [`attachments`](Self::attachments)).
    pub fn is_attachment(&self) -> bool {
        if let Some(d) = &self.disposition
            && d.is_attachment()
        {
            return true;
        }
        if self.content_type.name().is_some()
            || self
                .disposition
                .as_ref()
                .and_then(|d| d.filename())
                .is_some()
        {
            // Has a filename → treat as attachment even without
            // explicit Content-Disposition.
            return true;
        }
        false
    }
}

#[cfg(test)]
mod tests {
    use crate::parse;

    /// A signature logo the HTML draws by `cid:` is not an attachment to
    /// the reader; the PDF is, and so is an image nothing draws.
    #[test]
    fn inline_images_drawn_by_the_body_are_not_counted() {
        let raw = b"Content-Type: multipart/mixed; boundary=M\r\n\r\n\
--M\r\nContent-Type: multipart/related; boundary=R\r\n\r\n\
--R\r\nContent-Type: text/html\r\n\r\n<p>hi</p><img src=\"cid:Logo@x\">\r\n\
--R\r\nContent-Type: image/png; name=logo.png\r\nContent-ID: <logo@x>\r\n\
Content-Disposition: inline; filename=logo.png\r\n\r\nPNG\r\n\
--R\r\nContent-Type: image/png; name=stray.png\r\nContent-ID: <stray@x>\r\n\r\nPNG\r\n\
--R--\r\n\
--M\r\nContent-Type: application/pdf; name=a.pdf\r\n\
Content-Disposition: attachment; filename=a.pdf\r\n\r\n%PDF\r\n\
--M--\r\n";
        let root = parse(raw);
        assert_eq!(root.attachments().count(), 3);
        assert_eq!(root.shown_attachment_count(), 2);
    }

    #[test]
    fn a_letter_has_none() {
        let root = parse(b"Content-Type: text/plain\r\n\r\nhello\r\n");
        assert_eq!(root.shown_attachment_count(), 0);
    }
}

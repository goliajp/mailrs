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

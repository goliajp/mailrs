//! Message inspection context — wraps the raw RFC 5322 bytes and
//! exposes the header / body queries that test evaluation needs.
//! Extracted from `eval/mod.rs` so the main evaluator stays under
//! the file-size limit.

pub(super) struct MessageContext<'m> {
    raw: &'m [u8],
}

impl<'m> MessageContext<'m> {
    pub(super) fn new(raw: &'m [u8]) -> Self {
        Self { raw }
    }

    /// All values of header `name` (case-insensitive), with RFC
    /// 5322 §2.2.3 folding (`\r\n ` / `\r\n\t`) collapsed back to
    /// single-line form for `:contains` / `:matches` to see the
    /// logical value.
    pub(super) fn header_values(&self, name: &str) -> Vec<String> {
        let parsed = mailrs_rfc5322::Message::new(self.raw);
        parsed
            .header_all(name)
            .filter_map(|h| {
                h.value_str()
                    .map(|s| s.replace("\r\n ", " ").replace("\r\n\t", " "))
            })
            .collect()
    }

    /// Total message size in bytes — what `size :over` / `:under`
    /// compares against.
    pub(super) fn body_size(&self) -> u64 {
        self.raw.len() as u64
    }
}

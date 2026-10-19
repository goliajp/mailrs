//! The id a message goes by when it carries no `Message-ID` header.

/// `header_id` when the message has one, otherwise an id derived from its
/// maildir file name: `<epoch>.<unique>@mailrs.local`.
///
/// Every path that reads the file (delivery, the self-heal sweep, the file
/// index) must arrive at the same id, so it comes from the name and not
/// from the clock, and the `:2,` flags are left off: they change when the
/// message is read, the id must not. The form is the one the monolith
/// minted at delivery, so mail it indexed is found again, not indexed a
/// second time.
pub(crate) fn message_id_or_file_id(header_id: String, blob_ref: &str) -> String {
    if !header_id.is_empty() {
        return header_id;
    }
    let name = blob_ref.rsplit('/').next().unwrap_or(blob_ref);
    let unique = name.split_once(":2,").map_or(name, |(u, _)| u);
    match crate::maildir_filename_epoch(unique) {
        Some(epoch) => format!("{epoch}.{unique}@mailrs.local"),
        None => format!("{unique}@mailrs.local"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_header_id_is_kept() {
        assert_eq!(
            message_id_or_file_id("a@b".into(), "1790000000.M1P1.h"),
            "a@b"
        );
    }

    /// The monolith's form for the same file, whatever folder or flags the
    /// file has since gained.
    #[test]
    fn a_missing_id_comes_from_the_file_name_and_survives_reading() {
        let want = "1781066005.1781066005.M390270P1Q0.8f4314585e63@mailrs.local";
        for name in [
            "1781066005.M390270P1Q0.8f4314585e63",
            "1781066005.M390270P1Q0.8f4314585e63:2,S",
            ".Archive/1781066005.M390270P1Q0.8f4314585e63:2,RS",
        ] {
            assert_eq!(message_id_or_file_id(String::new(), name), want, "{name}");
        }
        assert_eq!(
            message_id_or_file_id(String::new(), "odd-name"),
            "odd-name@mailrs.local"
        );
    }
}

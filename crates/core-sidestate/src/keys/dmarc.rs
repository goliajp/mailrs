//! Received DMARC aggregate reports: intake stores them, the webapi lists
//! them.

/// Prefix of the per-report metadata hash.
pub const REPORT_PREFIX: &str = "mailrs:dmarcrpt:report:";

/// Prefix of the per-report row zset.
pub const ROWS_PREFIX: &str = "mailrs:dmarcrpt:rows:";

/// Time-ordered index of every stored report.
pub const INDEX_KEY: &[u8] = b"mailrs:dmarcrpt:index";

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dmarc_report_keys_keep_their_prod_spelling() {
        assert_eq!(REPORT_PREFIX, "mailrs:dmarcrpt:report:");
        assert_eq!(ROWS_PREFIX, "mailrs:dmarcrpt:rows:");
        assert_eq!(INDEX_KEY, b"mailrs:dmarcrpt:index");
    }
}

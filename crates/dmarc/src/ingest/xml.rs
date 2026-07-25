//! `<feedback>` XML document → [`AggregateReport`].
//!
//! This is a trust boundary: the input arrives by mail from anyone who
//! chooses to send it. Every failure mode returns an error; nothing
//! panics, and nothing is silently truncated.

use super::model::AggregateReport;

/// Largest report payload accepted, after decompression.
///
/// Real aggregate reports from large receivers run to a few hundred
/// kilobytes. 16 MiB is well clear of legitimate traffic while bounding
/// what a hostile sender can make us allocate.
pub const MAX_PAYLOAD_BYTES: usize = 16 * 1024 * 1024;

/// Largest number of `<record>` rows accepted in one report.
///
/// Deliberately a hard error rather than a silent truncation — a report
/// this size means either a domain far larger than anything we host, or
/// an attempt to exhaust storage. Either way the operator should see it
/// rather than get a quietly partial row set.
pub const MAX_RECORDS: usize = 50_000;

/// Why an aggregate report failed to parse.
#[derive(Debug, thiserror::Error)]
pub enum IngestError {
    /// Payload exceeded [`MAX_PAYLOAD_BYTES`].
    #[error("report payload too large: {bytes} bytes (max {MAX_PAYLOAD_BYTES})")]
    TooLarge {
        /// Actual payload size.
        bytes: usize,
    },
    /// Row count exceeded [`MAX_RECORDS`].
    #[error("report has {records} records (max {MAX_RECORDS})")]
    TooManyRecords {
        /// Actual row count.
        records: usize,
    },
    /// Payload was not valid UTF-8.
    #[error("report payload is not valid UTF-8")]
    NotUtf8,
    /// XML was malformed, or did not match the RFC 7489 schema.
    #[error("malformed aggregate report XML: {0}")]
    Xml(String),
    /// Decompression failed.
    #[error("could not decompress report attachment: {0}")]
    Decompress(String),
}

/// Parse one `<feedback>` document.
///
/// Accepts an optional UTF-8 BOM and an XML declaration, both of which
/// appear in the wild.
pub fn parse_aggregate_report(xml: &[u8]) -> Result<AggregateReport, IngestError> {
    if xml.len() > MAX_PAYLOAD_BYTES {
        return Err(IngestError::TooLarge { bytes: xml.len() });
    }
    let text = std::str::from_utf8(strip_bom(xml)).map_err(|_| IngestError::NotUtf8)?;
    let report: AggregateReport =
        quick_xml::de::from_str(text).map_err(|e| IngestError::Xml(e.to_string()))?;
    if report.records.len() > MAX_RECORDS {
        return Err(IngestError::TooManyRecords {
            records: report.records.len(),
        });
    }
    Ok(report)
}

/// Drop a leading UTF-8 byte-order mark if present.
fn strip_bom(bytes: &[u8]) -> &[u8] {
    bytes.strip_prefix(&[0xEF, 0xBB, 0xBF]).unwrap_or(bytes)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Shape emitted by Google, trimmed to two records.
    const GOOGLE_REPORT: &str = r#"<?xml version="1.0" encoding="UTF-8" ?>
<feedback>
  <report_metadata>
    <org_name>google.com</org_name>
    <email>noreply-dmarc-support@google.com</email>
    <extra_contact_info>https://support.google.com/a/answer/2466580</extra_contact_info>
    <report_id>2385904712340987654</report_id>
    <date_range>
      <begin>1784937600</begin>
      <end>1785023999</end>
    </date_range>
  </report_metadata>
  <policy_published>
    <domain>golia.jp</domain>
    <adkim>r</adkim>
    <aspf>r</aspf>
    <p>quarantine</p>
    <sp>quarantine</sp>
    <pct>100</pct>
  </policy_published>
  <record>
    <row>
      <source_ip>203.0.113.10</source_ip>
      <count>5</count>
      <policy_evaluated>
        <disposition>none</disposition>
        <dkim>pass</dkim>
        <spf>pass</spf>
      </policy_evaluated>
    </row>
    <identifiers>
      <header_from>golia.jp</header_from>
    </identifiers>
    <auth_results>
      <dkim>
        <domain>golia.jp</domain>
        <result>pass</result>
        <selector>s1</selector>
      </dkim>
      <spf>
        <domain>golia.jp</domain>
        <result>pass</result>
      </spf>
    </auth_results>
  </record>
  <record>
    <row>
      <source_ip>198.51.100.7</source_ip>
      <count>2</count>
      <policy_evaluated>
        <disposition>quarantine</disposition>
        <dkim>fail</dkim>
        <spf>fail</spf>
      </policy_evaluated>
    </row>
    <identifiers>
      <envelope_from>golia.jp</envelope_from>
      <header_from>golia.jp</header_from>
    </identifiers>
    <auth_results>
      <spf>
        <domain>golia.jp</domain>
        <scope>mfrom</scope>
        <result>softfail</result>
      </spf>
    </auth_results>
  </record>
</feedback>"#;

    #[test]
    fn parses_a_two_record_report() {
        let r = parse_aggregate_report(GOOGLE_REPORT.as_bytes()).expect("parses");

        assert_eq!(r.report_metadata.org_name, "google.com");
        assert_eq!(r.report_metadata.date_range.begin, 1784937600);
        assert_eq!(r.policy_published.domain, "golia.jp");
        assert_eq!(r.policy_published.p, "quarantine");
        assert_eq!(r.policy_published.pct, Some(100));
        assert_eq!(r.records.len(), 2);
    }

    #[test]
    fn sums_counts_and_pass_rate() {
        let r = parse_aggregate_report(GOOGLE_REPORT.as_bytes()).expect("parses");

        assert_eq!(r.total_count(), 7);
        assert_eq!(r.passing_count(), 5);
    }

    #[test]
    fn reads_auth_results_and_optional_fields() {
        let r = parse_aggregate_report(GOOGLE_REPORT.as_bytes()).expect("parses");

        let first = &r.records[0];
        assert_eq!(first.auth_results.dkim.len(), 1);
        assert_eq!(first.auth_results.dkim[0].selector.as_deref(), Some("s1"));
        assert_eq!(first.identifiers.envelope_from, None);

        let second = &r.records[1];
        assert!(second.auth_results.dkim.is_empty());
        assert_eq!(second.auth_results.spf[0].scope.as_deref(), Some("mfrom"));
        assert_eq!(
            second.identifiers.envelope_from.as_deref(),
            Some("golia.jp")
        );
    }

    #[test]
    fn passed_reads_policy_evaluated_not_auth_results() {
        let r = parse_aggregate_report(GOOGLE_REPORT.as_bytes()).expect("parses");

        assert!(r.records[0].passed());
        // softfail SPF in auth_results, but policy_evaluated says fail
        assert!(!r.records[1].passed());
    }

    #[test]
    fn accepts_a_utf8_bom() {
        let mut with_bom = vec![0xEF, 0xBB, 0xBF];
        with_bom.extend_from_slice(GOOGLE_REPORT.as_bytes());

        assert!(parse_aggregate_report(&with_bom).is_ok());
    }

    #[test]
    fn accepts_a_report_with_no_records() {
        let xml = r#"<feedback>
  <report_metadata>
    <org_name>quiet.example</org_name>
    <email>dmarc@quiet.example</email>
    <report_id>empty-1</report_id>
    <date_range><begin>1</begin><end>2</end></date_range>
  </report_metadata>
  <policy_published>
    <domain>golia.jp</domain>
    <p>none</p>
  </policy_published>
</feedback>"#;

        let r = parse_aggregate_report(xml.as_bytes()).expect("parses");
        assert!(r.records.is_empty());
        assert_eq!(r.total_count(), 0);
    }

    #[test]
    fn rejects_malformed_xml_without_panicking() {
        assert!(matches!(
            parse_aggregate_report(b"<feedback><unclosed>"),
            Err(IngestError::Xml(_))
        ));
    }

    #[test]
    fn rejects_a_document_that_is_not_a_report() {
        assert!(parse_aggregate_report(b"<html><body>hi</body></html>").is_err());
    }

    #[test]
    fn rejects_non_utf8() {
        assert!(matches!(
            parse_aggregate_report(&[0xFF, 0xFE, 0x00, 0x41]),
            Err(IngestError::NotUtf8)
        ));
    }

    #[test]
    fn rejects_oversized_payload() {
        let big = vec![b'x'; MAX_PAYLOAD_BYTES + 1];
        assert!(matches!(
            parse_aggregate_report(&big),
            Err(IngestError::TooLarge { .. })
        ));
    }

    #[test]
    fn empty_input_is_an_error_not_a_panic() {
        assert!(parse_aggregate_report(b"").is_err());
    }
}

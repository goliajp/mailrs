//! Pull aggregate-report XML out of a report mail.
//!
//! RFC 7489 §7.2.1 says reports arrive as a message with the report as
//! an attachment, compressed with gzip. In practice receivers vary:
//! Google and Microsoft send `application/gzip`, several send
//! `application/zip`, and a long tail sends `application/octet-stream`
//! and relies on the filename extension. Some send bare XML.
//!
//! So identification runs in three steps, most authoritative first:
//! Content-Type, then filename extension, then magic bytes. A part that
//! matches none of them is skipped.
//!
//! Decompression is bounded — a report that expands past
//! [`MAX_PAYLOAD_BYTES`] is dropped rather than allowed to consume
//! memory, since the input is attacker-reachable.

use std::io::Read;

use super::xml::{IngestError, MAX_PAYLOAD_BYTES};

/// How a candidate part is encoded.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Encoding {
    Gzip,
    Zip,
    Plain,
}

/// Extract every aggregate-report XML payload from a raw RFC 5322
/// message.
///
/// Returns one entry per decodable candidate attachment. A message
/// carrying no report yields an empty vec — that is the normal outcome
/// for ordinary mail and is not an error. Individual attachments that
/// fail to decompress are skipped so one bad part cannot hide a good
/// one in the same message.
pub fn extract_report_payloads(raw: &[u8]) -> Vec<Vec<u8>> {
    let root = mailrs_mime::parse(raw);
    let mut out = Vec::new();
    for part in root.walk() {
        let Some(encoding) = classify(part) else {
            continue;
        };
        if let Ok(payload) = decode(&part.body, encoding) {
            out.push(payload);
        }
    }
    out
}

/// Decide how a part is encoded, or `None` if it is not a plausible
/// report attachment.
fn classify(part: &mailrs_mime::Part<'_>) -> Option<Encoding> {
    let ct_type = part.content_type.type_.as_str();
    let ct_subtype = part.content_type.subtype.as_str();

    match (ct_type, ct_subtype) {
        ("application", "gzip" | "x-gzip" | "x-gzip-compressed") => return Some(Encoding::Gzip),
        ("application", "zip" | "x-zip" | "x-zip-compressed") => return Some(Encoding::Zip),
        ("application" | "text", "xml" | "json") => return Some(Encoding::Plain),
        _ => {}
    }

    // RFC 6839 structured-syntax suffixes. TLS-RPT reports arrive as
    // `application/tlsrpt+gzip`, which is a gzip payload even though
    // the subtype is not literally "gzip".
    if ct_type == "application" {
        if ct_subtype.ends_with("+gzip") {
            return Some(Encoding::Gzip);
        }
        if ct_subtype.ends_with("+zip") {
            return Some(Encoding::Zip);
        }
        if ct_subtype.ends_with("+json") || ct_subtype.ends_with("+xml") {
            return Some(Encoding::Plain);
        }
    }

    // Content-Type was unhelpful (octet-stream is common). Try the
    // filename the reporter attached.
    let name = part.attachment_filename().unwrap_or_default();
    let lower = name.to_ascii_lowercase();
    if lower.ends_with(".gz") {
        return Some(Encoding::Gzip);
    }
    if lower.ends_with(".zip") {
        return Some(Encoding::Zip);
    }
    if lower.ends_with(".xml") {
        return Some(Encoding::Plain);
    }

    // Last resort: sniff. Only applied to parts that look like
    // attachments, so ordinary text bodies are never fed to a
    // decompressor.
    if !part.is_attachment() {
        return None;
    }
    match part.body.as_ref() {
        [0x1F, 0x8B, ..] => Some(Encoding::Gzip),
        [b'P', b'K', 0x03, 0x04, ..] => Some(Encoding::Zip),
        _ => None,
    }
}

/// Decode one candidate part into XML bytes.
fn decode(body: &[u8], encoding: Encoding) -> Result<Vec<u8>, IngestError> {
    match encoding {
        Encoding::Plain => {
            if body.len() > MAX_PAYLOAD_BYTES {
                return Err(IngestError::TooLarge { bytes: body.len() });
            }
            Ok(body.to_vec())
        }
        Encoding::Gzip => read_bounded(flate2::read::GzDecoder::new(body)),
        Encoding::Zip => {
            let mut archive = zip::ZipArchive::new(std::io::Cursor::new(body))
                .map_err(|e| IngestError::Decompress(e.to_string()))?;
            // Aggregate reports are single-entry archives. Take the
            // first entry that decodes; ignore any others.
            for i in 0..archive.len() {
                let entry = archive
                    .by_index(i)
                    .map_err(|e| IngestError::Decompress(e.to_string()))?;
                if entry.is_dir() {
                    continue;
                }
                return read_bounded(entry);
            }
            Err(IngestError::Decompress("zip archive has no files".into()))
        }
    }
}

/// Read a decompressing stream, refusing anything that expands past
/// [`MAX_PAYLOAD_BYTES`]. Reading one byte past the cap is what makes
/// the overflow detectable rather than silently truncated.
fn read_bounded<R: Read>(reader: R) -> Result<Vec<u8>, IngestError> {
    let mut buf = Vec::new();
    let mut limited = reader.take(MAX_PAYLOAD_BYTES as u64 + 1);
    limited
        .read_to_end(&mut buf)
        .map_err(|e| IngestError::Decompress(e.to_string()))?;
    if buf.len() > MAX_PAYLOAD_BYTES {
        return Err(IngestError::TooLarge { bytes: buf.len() });
    }
    Ok(buf)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    const XML: &[u8] = b"<feedback><report_metadata/></feedback>";

    fn gzipped(payload: &[u8]) -> Vec<u8> {
        let mut e = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::default());
        e.write_all(payload).expect("gzip write");
        e.finish().expect("gzip finish")
    }

    fn zipped(name: &str, payload: &[u8]) -> Vec<u8> {
        let mut w = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
        w.start_file(name, zip::write::SimpleFileOptions::default())
            .expect("zip entry");
        w.write_all(payload).expect("zip write");
        w.finish().expect("zip finish").into_inner()
    }

    fn b64(bytes: &[u8]) -> String {
        use base64::Engine;
        base64::engine::general_purpose::STANDARD.encode(bytes)
    }

    fn message_with(content_type: &str, filename: &str, encoded: &str) -> Vec<u8> {
        format!(
            "From: reporter@example.com\r\n\
             To: dmarc@golia.jp\r\n\
             Subject: Report domain: golia.jp\r\n\
             MIME-Version: 1.0\r\n\
             Content-Type: multipart/mixed; boundary=\"B\"\r\n\
             \r\n\
             --B\r\n\
             Content-Type: text/plain\r\n\
             \r\n\
             This is a DMARC aggregate report.\r\n\
             --B\r\n\
             Content-Type: {content_type}; name=\"{filename}\"\r\n\
             Content-Disposition: attachment; filename=\"{filename}\"\r\n\
             Content-Transfer-Encoding: base64\r\n\
             \r\n\
             {encoded}\r\n\
             --B--\r\n"
        )
        .into_bytes()
    }

    #[test]
    fn extracts_a_gzip_attachment() {
        let msg = message_with(
            "application/gzip",
            "golia.jp!1784937600.xml.gz",
            &b64(&gzipped(XML)),
        );

        let payloads = extract_report_payloads(&msg);
        assert_eq!(payloads.len(), 1);
        assert_eq!(payloads[0], XML);
    }

    #[test]
    fn extracts_a_zip_attachment() {
        let msg = message_with(
            "application/zip",
            "golia.jp!1784937600.xml.zip",
            &b64(&zipped("report.xml", XML)),
        );

        let payloads = extract_report_payloads(&msg);
        assert_eq!(payloads.len(), 1);
        assert_eq!(payloads[0], XML);
    }

    #[test]
    fn extracts_a_tlsrpt_structured_suffix_attachment() {
        // TLS-RPT (RFC 8460) uses application/tlsrpt+gzip. The payload
        // is gzip; only the subtype spelling differs.
        let json = br#"{"organization-name":"example"}"#;
        let msg = message_with(
            "application/tlsrpt+gzip",
            "example.com!golia.jp!1.json.gz",
            &b64(&gzipped(json)),
        );

        let payloads = extract_report_payloads(&msg);
        assert_eq!(payloads.len(), 1);
        assert_eq!(payloads[0], json);
    }

    #[test]
    fn extracts_a_bare_json_attachment() {
        let json = br#"{"organization-name":"example"}"#;
        let msg = message_with("application/json", "report.json", &b64(json));

        let payloads = extract_report_payloads(&msg);
        assert_eq!(payloads.len(), 1);
        assert_eq!(payloads[0], json);
    }

    #[test]
    fn extracts_a_bare_xml_attachment() {
        let msg = message_with("application/xml", "report.xml", &b64(XML));

        let payloads = extract_report_payloads(&msg);
        assert_eq!(payloads.len(), 1);
        assert_eq!(payloads[0], XML);
    }

    #[test]
    fn falls_back_to_filename_when_content_type_is_octet_stream() {
        let msg = message_with(
            "application/octet-stream",
            "golia.jp!1784937600.xml.gz",
            &b64(&gzipped(XML)),
        );

        let payloads = extract_report_payloads(&msg);
        assert_eq!(payloads.len(), 1, "filename extension must be honoured");
        assert_eq!(payloads[0], XML);
    }

    #[test]
    fn falls_back_to_magic_bytes_when_type_and_name_are_useless() {
        let msg = message_with(
            "application/octet-stream",
            "report.bin",
            &b64(&gzipped(XML)),
        );

        let payloads = extract_report_payloads(&msg);
        assert_eq!(payloads.len(), 1, "gzip magic must be honoured");
        assert_eq!(payloads[0], XML);
    }

    #[test]
    fn ordinary_mail_yields_nothing() {
        let msg = b"From: a@b.c\r\n\
                    To: dmarc@golia.jp\r\n\
                    Subject: hello\r\n\
                    \r\n\
                    just a normal message, not a report\r\n";

        assert!(extract_report_payloads(msg).is_empty());
    }

    #[test]
    fn a_plain_text_body_is_never_sniffed() {
        // Body text that happens to start with gzip magic must not be
        // treated as an attachment — is_attachment() gates the sniff.
        let msg = b"From: a@b.c\r\n\
                    Content-Type: text/plain\r\n\
                    \r\n\
                    \x1f\x8b not really gzip\r\n";

        assert!(extract_report_payloads(msg).is_empty());
    }

    #[test]
    fn a_corrupt_attachment_is_skipped_not_fatal() {
        let msg = message_with(
            "application/gzip",
            "broken.xml.gz",
            &b64(b"not gzip at all"),
        );

        assert!(extract_report_payloads(&msg).is_empty());
    }

    #[test]
    fn one_bad_attachment_does_not_hide_a_good_one() {
        let good = b64(&gzipped(XML));
        let bad = b64(b"garbage");
        let msg = format!(
            "From: reporter@example.com\r\n\
             MIME-Version: 1.0\r\n\
             Content-Type: multipart/mixed; boundary=\"B\"\r\n\
             \r\n\
             --B\r\n\
             Content-Type: application/gzip; name=\"bad.gz\"\r\n\
             Content-Disposition: attachment; filename=\"bad.gz\"\r\n\
             Content-Transfer-Encoding: base64\r\n\
             \r\n\
             {bad}\r\n\
             --B\r\n\
             Content-Type: application/gzip; name=\"good.xml.gz\"\r\n\
             Content-Disposition: attachment; filename=\"good.xml.gz\"\r\n\
             Content-Transfer-Encoding: base64\r\n\
             \r\n\
             {good}\r\n\
             --B--\r\n"
        )
        .into_bytes();

        let payloads = extract_report_payloads(&msg);
        assert_eq!(payloads.len(), 1);
        assert_eq!(payloads[0], XML);
    }

    #[test]
    fn a_decompression_bomb_is_refused() {
        // ~64 MiB of zeros compresses to a few KiB; the cap must stop it.
        let bomb = gzipped(&vec![0u8; MAX_PAYLOAD_BYTES + 1024]);
        assert!(bomb.len() < 200_000, "sanity: bomb is small on the wire");

        let msg = message_with("application/gzip", "bomb.xml.gz", &b64(&bomb));
        assert!(extract_report_payloads(&msg).is_empty());
    }
}

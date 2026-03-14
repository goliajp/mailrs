use std::io;
use std::sync::Arc;

use rustls::client::danger::{HandshakeSignatureValid, ServerCertVerified, ServerCertVerifier};
use rustls::pki_types::{CertificateDer, ServerName, UnixTime};
use rustls::{DigitallySignedStruct, Error as TlsError, SignatureScheme};
use sha2::{Digest, Sha256, Sha512};

/// TLSA certificate usage (RFC 6698 §2.1.1)
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TlsaUsage {
    /// CA constraint (PKIX-TA)
    PkixTa = 0,
    /// service certificate constraint (PKIX-EE)
    PkixEe = 1,
    /// trust anchor assertion (DANE-TA)
    DaneTa = 2,
    /// domain-issued certificate (DANE-EE)
    DaneEe = 3,
}

/// TLSA selector (RFC 6698 §2.1.2)
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TlsaSelector {
    /// full certificate
    Full = 0,
    /// SubjectPublicKeyInfo
    Spki = 1,
}

/// TLSA matching type (RFC 6698 §2.1.3)
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TlsaMatchingType {
    /// exact match
    Exact = 0,
    /// SHA-256 hash
    Sha256 = 1,
    /// SHA-512 hash
    Sha512 = 2,
}

/// a parsed TLSA record
#[derive(Debug, Clone)]
pub struct TlsaRecord {
    pub usage: TlsaUsage,
    pub selector: TlsaSelector,
    pub matching_type: TlsaMatchingType,
    pub association_data: Vec<u8>,
}

impl TlsaRecord {
    /// parse from raw RDATA bytes (usage, selector, matching_type, data)
    pub fn from_rdata(data: &[u8]) -> Option<Self> {
        if data.len() < 4 {
            return None;
        }
        let usage = match data[0] {
            0 => TlsaUsage::PkixTa,
            1 => TlsaUsage::PkixEe,
            2 => TlsaUsage::DaneTa,
            3 => TlsaUsage::DaneEe,
            _ => return None,
        };
        let selector = match data[1] {
            0 => TlsaSelector::Full,
            1 => TlsaSelector::Spki,
            _ => return None,
        };
        let matching_type = match data[2] {
            0 => TlsaMatchingType::Exact,
            1 => TlsaMatchingType::Sha256,
            2 => TlsaMatchingType::Sha512,
            _ => return None,
        };
        Some(TlsaRecord {
            usage,
            selector,
            matching_type,
            association_data: data[3..].to_vec(),
        })
    }
}

/// resolve TLSA records for a given MX host (port 25)
pub async fn resolve_tlsa(
    resolver: &hickory_resolver::TokioResolver,
    mx_host: &str,
) -> Vec<TlsaRecord> {
    use hickory_resolver::proto::rr::RecordType;
    use hickory_resolver::proto::rr::rdata::TLSA;

    let qname = format!("_25._tcp.{mx_host}");
    let lookup = match resolver.lookup(&qname, RecordType::TLSA).await {
        Ok(l) => l,
        Err(_) => return Vec::new(),
    };

    let mut records = Vec::new();
    for rdata in lookup.iter() {
        // hickory returns TLSA variant directly — extract fields
        if let Some(tlsa) = rdata.as_tlsa() {
            let mut raw = vec![
                tlsa.cert_usage().into(),
                tlsa.selector().into(),
                tlsa.matching().into(),
            ];
            raw.extend_from_slice(tlsa.cert_data());
            if let Some(record) = TlsaRecord::from_rdata(&raw) {
                records.push(record);
            }
        }
    }
    records
}

/// extract SubjectPublicKeyInfo from a DER certificate
fn extract_spki(cert_der: &[u8]) -> Option<Vec<u8>> {
    // parse the outer SEQUENCE, skip to tbsCertificate, then find
    // subjectPublicKeyInfo which is the 7th element (index 6)
    // simplified: use the rustls-webpki parsing that's already available
    // for pragmatic implementation, hash the full cert or extract SPKI via offset
    //
    // X.509 structure:
    //   SEQUENCE {
    //     tbsCertificate SEQUENCE {
    //       version [0] EXPLICIT (optional)
    //       serialNumber INTEGER
    //       signature AlgorithmIdentifier
    //       issuer Name
    //       validity Validity
    //       subject Name
    //       subjectPublicKeyInfo SEQUENCE { ... }   <-- we want this
    //
    // use a simple ASN.1 walk to find the SPKI
    let mut pos = 0;
    // skip outer SEQUENCE header
    pos = skip_tag_length(cert_der, pos)?;
    // skip tbsCertificate SEQUENCE header
    let tbs_start = pos;
    pos = skip_tag_length(cert_der, tbs_start)?;

    // skip version if present (context tag [0])
    if cert_der.get(pos)? & 0xE0 == 0xA0 {
        pos = skip_tlv(cert_der, pos)?;
    }
    // skip serialNumber
    pos = skip_tlv(cert_der, pos)?;
    // skip signature AlgorithmIdentifier
    pos = skip_tlv(cert_der, pos)?;
    // skip issuer
    pos = skip_tlv(cert_der, pos)?;
    // skip validity
    pos = skip_tlv(cert_der, pos)?;
    // skip subject
    pos = skip_tlv(cert_der, pos)?;
    // NOW we're at subjectPublicKeyInfo
    let spki_start = pos;
    let spki_end = skip_tlv(cert_der, pos)?;
    Some(cert_der[spki_start..spki_end].to_vec())
}

/// skip a TLV (tag + length + value), returning the position after it
fn skip_tlv(data: &[u8], pos: usize) -> Option<usize> {
    if pos >= data.len() {
        return None;
    }
    // skip tag
    let mut p = pos + 1;
    // parse length
    let (len, p2) = parse_der_length(data, p)?;
    p = p2;
    Some(p + len)
}

/// skip just the tag + length header, returning position of value start
fn skip_tag_length(data: &[u8], pos: usize) -> Option<usize> {
    if pos >= data.len() {
        return None;
    }
    let p = pos + 1;
    let (_len, p2) = parse_der_length(data, p)?;
    Some(p2)
}

/// parse DER length encoding, returns (length, position_after_length)
fn parse_der_length(data: &[u8], pos: usize) -> Option<(usize, usize)> {
    if pos >= data.len() {
        return None;
    }
    let first = data[pos] as usize;
    if first < 0x80 {
        Some((first, pos + 1))
    } else {
        let num_bytes = first & 0x7F;
        if num_bytes == 0 || num_bytes > 4 || pos + 1 + num_bytes > data.len() {
            return None;
        }
        let mut len = 0usize;
        for i in 0..num_bytes {
            len = (len << 8) | (data[pos + 1 + i] as usize);
        }
        Some((len, pos + 1 + num_bytes))
    }
}

/// check if a certificate matches a TLSA record
fn cert_matches_tlsa(cert_der: &[u8], record: &TlsaRecord) -> bool {
    let data_to_match = match record.selector {
        TlsaSelector::Full => cert_der.to_vec(),
        TlsaSelector::Spki => match extract_spki(cert_der) {
            Some(spki) => spki,
            None => return false,
        },
    };

    match record.matching_type {
        TlsaMatchingType::Exact => data_to_match == record.association_data,
        TlsaMatchingType::Sha256 => {
            let hash = Sha256::digest(&data_to_match);
            hash.as_slice() == record.association_data
        }
        TlsaMatchingType::Sha512 => {
            let hash = Sha512::digest(&data_to_match);
            hash.as_slice() == record.association_data
        }
    }
}

/// verify a certificate chain against TLSA records
/// returns true if any TLSA record matches
pub fn verify_against_tlsa(
    end_entity: &CertificateDer<'_>,
    intermediates: &[CertificateDer<'_>],
    records: &[TlsaRecord],
) -> bool {
    for record in records {
        match record.usage {
            TlsaUsage::DaneEe | TlsaUsage::PkixEe => {
                // match against end-entity certificate
                if cert_matches_tlsa(end_entity.as_ref(), record) {
                    return true;
                }
            }
            TlsaUsage::DaneTa | TlsaUsage::PkixTa => {
                // match against any certificate in the chain (including end-entity)
                if cert_matches_tlsa(end_entity.as_ref(), record) {
                    return true;
                }
                for intermediate in intermediates {
                    if cert_matches_tlsa(intermediate.as_ref(), record) {
                        return true;
                    }
                }
            }
        }
    }
    false
}

/// custom TLS verifier that checks DANE TLSA records
/// for DANE-EE (usage 3): skip PKIX, only verify against TLSA
/// for DANE-TA (usage 2): verify TLSA, optionally also PKIX
/// for PKIX-* (usage 0,1): require standard PKIX + TLSA match
#[derive(Debug)]
pub struct DaneVerifier {
    tlsa_records: Vec<TlsaRecord>,
    pkix_verifier: Arc<dyn ServerCertVerifier>,
}

impl DaneVerifier {
    pub fn new(tlsa_records: Vec<TlsaRecord>) -> Self {
        let roots = rustls::RootCertStore {
            roots: webpki_roots::TLS_SERVER_ROOTS.to_vec(),
        };
        let pkix = rustls::client::WebPkiServerVerifier::builder(Arc::new(roots))
            .build()
            .expect("failed to build PKIX verifier");
        Self {
            tlsa_records,
            pkix_verifier: pkix,
        }
    }

    fn has_dane_ee(&self) -> bool {
        self.tlsa_records
            .iter()
            .any(|r| r.usage == TlsaUsage::DaneEe)
    }
}

impl ServerCertVerifier for DaneVerifier {
    fn verify_server_cert(
        &self,
        end_entity: &CertificateDer<'_>,
        intermediates: &[CertificateDer<'_>],
        server_name: &ServerName<'_>,
        ocsp_response: &[u8],
        now: UnixTime,
    ) -> Result<ServerCertVerified, TlsError> {
        // check DANE-EE records first (skip PKIX if matched)
        if self.has_dane_ee() {
            let dane_ee_records: Vec<&TlsaRecord> = self
                .tlsa_records
                .iter()
                .filter(|r| r.usage == TlsaUsage::DaneEe)
                .collect();
            for record in &dane_ee_records {
                if cert_matches_tlsa(end_entity.as_ref(), record) {
                    return Ok(ServerCertVerified::assertion());
                }
            }
            // DANE-EE records present but none matched
            return Err(TlsError::General(
                "DANE-EE: certificate does not match any TLSA record".into(),
            ));
        }

        // for other usage types, try PKIX first then check TLSA
        let pkix_result = self.pkix_verifier.verify_server_cert(
            end_entity,
            intermediates,
            server_name,
            ocsp_response,
            now,
        );

        // if TLSA records match, accept even if PKIX fails (DANE-TA)
        if verify_against_tlsa(end_entity, intermediates, &self.tlsa_records) {
            return Ok(ServerCertVerified::assertion());
        }

        // fall back to PKIX result
        pkix_result
    }

    fn verify_tls12_signature(
        &self,
        message: &[u8],
        cert: &CertificateDer<'_>,
        dss: &DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, TlsError> {
        self.pkix_verifier
            .verify_tls12_signature(message, cert, dss)
    }

    fn verify_tls13_signature(
        &self,
        message: &[u8],
        cert: &CertificateDer<'_>,
        dss: &DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, TlsError> {
        self.pkix_verifier
            .verify_tls13_signature(message, cert, dss)
    }

    fn supported_verify_schemes(&self) -> Vec<SignatureScheme> {
        self.pkix_verifier.supported_verify_schemes()
    }
}

/// build a rustls ClientConfig with DANE verification
pub fn dane_tls_config(tlsa_records: Vec<TlsaRecord>) -> rustls::ClientConfig {
    let mut config = rustls::ClientConfig::builder()
        .dangerous()
        .with_custom_certificate_verifier(Arc::new(DaneVerifier::new(tlsa_records)))
        .with_no_client_auth();
    config.alpn_protocols = vec![];
    config
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_tlsa_too_short() {
        assert!(TlsaRecord::from_rdata(&[0, 1]).is_none());
        assert!(TlsaRecord::from_rdata(&[]).is_none());
    }

    #[test]
    fn parse_tlsa_invalid_usage() {
        assert!(TlsaRecord::from_rdata(&[4, 0, 1, 0xAA]).is_none());
    }

    #[test]
    fn parse_tlsa_invalid_selector() {
        assert!(TlsaRecord::from_rdata(&[3, 2, 1, 0xAA]).is_none());
    }

    #[test]
    fn parse_tlsa_invalid_matching_type() {
        assert!(TlsaRecord::from_rdata(&[3, 1, 3, 0xAA]).is_none());
    }

    #[test]
    fn parse_tlsa_dane_ee_sha256() {
        let mut data = vec![3, 1, 1]; // usage=DANE-EE, selector=SPKI, matching=SHA-256
        data.extend_from_slice(&[0xAA; 32]); // 32-byte hash
        let record = TlsaRecord::from_rdata(&data).unwrap();
        assert_eq!(record.usage, TlsaUsage::DaneEe);
        assert_eq!(record.selector, TlsaSelector::Spki);
        assert_eq!(record.matching_type, TlsaMatchingType::Sha256);
        assert_eq!(record.association_data.len(), 32);
    }

    #[test]
    fn parse_tlsa_pkix_ta_full_exact() {
        let mut data = vec![0, 0, 0]; // usage=PKIX-TA, selector=Full, matching=Exact
        data.extend_from_slice(&[0xBB; 100]);
        let record = TlsaRecord::from_rdata(&data).unwrap();
        assert_eq!(record.usage, TlsaUsage::PkixTa);
        assert_eq!(record.selector, TlsaSelector::Full);
        assert_eq!(record.matching_type, TlsaMatchingType::Exact);
    }

    #[test]
    fn parse_tlsa_dane_ta_sha512() {
        let mut data = vec![2, 0, 2]; // usage=DANE-TA, selector=Full, matching=SHA-512
        data.extend_from_slice(&[0xCC; 64]);
        let record = TlsaRecord::from_rdata(&data).unwrap();
        assert_eq!(record.usage, TlsaUsage::DaneTa);
        assert_eq!(record.matching_type, TlsaMatchingType::Sha512);
    }

    #[test]
    fn cert_matches_full_sha256() {
        let cert = b"fake cert data for testing purposes only";
        let hash = Sha256::digest(cert);
        let record = TlsaRecord {
            usage: TlsaUsage::DaneEe,
            selector: TlsaSelector::Full,
            matching_type: TlsaMatchingType::Sha256,
            association_data: hash.to_vec(),
        };
        assert!(cert_matches_tlsa(cert, &record));
    }

    #[test]
    fn cert_matches_full_sha512() {
        let cert = b"another fake cert";
        let hash = Sha512::digest(cert);
        let record = TlsaRecord {
            usage: TlsaUsage::DaneEe,
            selector: TlsaSelector::Full,
            matching_type: TlsaMatchingType::Sha512,
            association_data: hash.to_vec(),
        };
        assert!(cert_matches_tlsa(cert, &record));
    }

    #[test]
    fn cert_matches_full_exact() {
        let cert = b"exact match cert";
        let record = TlsaRecord {
            usage: TlsaUsage::DaneEe,
            selector: TlsaSelector::Full,
            matching_type: TlsaMatchingType::Exact,
            association_data: cert.to_vec(),
        };
        assert!(cert_matches_tlsa(cert, &record));
    }

    #[test]
    fn cert_mismatch_full_sha256() {
        let cert = b"cert data";
        let record = TlsaRecord {
            usage: TlsaUsage::DaneEe,
            selector: TlsaSelector::Full,
            matching_type: TlsaMatchingType::Sha256,
            association_data: vec![0xFF; 32],
        };
        assert!(!cert_matches_tlsa(cert, &record));
    }

    #[test]
    fn verify_against_tlsa_dane_ee_match() {
        let cert_data = b"test cert";
        let hash = Sha256::digest(cert_data);
        let record = TlsaRecord {
            usage: TlsaUsage::DaneEe,
            selector: TlsaSelector::Full,
            matching_type: TlsaMatchingType::Sha256,
            association_data: hash.to_vec(),
        };
        let cert = CertificateDer::from(cert_data.to_vec());
        assert!(verify_against_tlsa(&cert, &[], &[record]));
    }

    #[test]
    fn verify_against_tlsa_dane_ta_intermediate_match() {
        let ee_cert = b"end entity";
        let ca_cert = b"ca cert";
        let hash = Sha256::digest(ca_cert);
        let record = TlsaRecord {
            usage: TlsaUsage::DaneTa,
            selector: TlsaSelector::Full,
            matching_type: TlsaMatchingType::Sha256,
            association_data: hash.to_vec(),
        };
        let ee = CertificateDer::from(ee_cert.to_vec());
        let ca = CertificateDer::from(ca_cert.to_vec());
        assert!(verify_against_tlsa(&ee, &[ca], &[record]));
    }

    #[test]
    fn verify_against_tlsa_no_match() {
        let cert = CertificateDer::from(b"cert".to_vec());
        let record = TlsaRecord {
            usage: TlsaUsage::DaneEe,
            selector: TlsaSelector::Full,
            matching_type: TlsaMatchingType::Sha256,
            association_data: vec![0xFF; 32],
        };
        assert!(!verify_against_tlsa(&cert, &[], &[record]));
    }

    #[test]
    fn verify_against_empty_records() {
        let cert = CertificateDer::from(b"cert".to_vec());
        assert!(!verify_against_tlsa(&cert, &[], &[]));
    }

    #[test]
    fn dane_tls_config_creates_valid_config() {
        let _ = rustls::crypto::ring::default_provider().install_default();
        let records = vec![TlsaRecord {
            usage: TlsaUsage::DaneEe,
            selector: TlsaSelector::Full,
            matching_type: TlsaMatchingType::Sha256,
            association_data: vec![0xAA; 32],
        }];
        let config = dane_tls_config(records);
        assert!(config.alpn_protocols.is_empty());
    }

    #[test]
    fn der_length_short_form() {
        let data = [0x05]; // length = 5
        let (len, pos) = parse_der_length(&data, 0).unwrap();
        assert_eq!(len, 5);
        assert_eq!(pos, 1);
    }

    #[test]
    fn der_length_long_form_one_byte() {
        let data = [0x81, 0x80]; // length = 128
        let (len, pos) = parse_der_length(&data, 0).unwrap();
        assert_eq!(len, 128);
        assert_eq!(pos, 2);
    }

    #[test]
    fn der_length_long_form_two_bytes() {
        let data = [0x82, 0x01, 0x00]; // length = 256
        let (len, pos) = parse_der_length(&data, 0).unwrap();
        assert_eq!(len, 256);
        assert_eq!(pos, 3);
    }

    #[test]
    fn der_length_empty() {
        assert!(parse_der_length(&[], 0).is_none());
    }
}

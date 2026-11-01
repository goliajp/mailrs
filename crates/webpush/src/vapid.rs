//! RFC 8292: who is sending, proven to the push service with a short-lived
//! token signed by this server's key.

use aws_lc_rs::rand::SystemRandom;
use aws_lc_rs::signature::{ECDSA_P256_SHA256_FIXED_SIGNING, EcdsaKeyPair, KeyPair};
use base64::Engine as _;

use crate::b64url;

/// How long a token is good for; RFC 8292 allows at most 24 hours.
const LIFETIME: i64 = 12 * 3600;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Error {
    /// Not a PEM PKCS#8 P-256 private key.
    BadKey,
    Sign,
}

/// This server's signing key and contact.
pub struct Vapid {
    key: EcdsaKeyPair,
    /// `mailto:` or `https:`: who the push service can reach.
    subject: String,
}

/// The contact the push services see: `MAILRS_VAPID_SUBJECT` when set,
/// otherwise the postmaster of this server's hostname.
pub fn subject(configured: Option<&str>, hostname: Option<&str>) -> Result<String, String> {
    let configured = configured.map(str::trim).filter(|s| !s.is_empty());
    let hostname = hostname.map(str::trim).filter(|s| !s.is_empty());
    match (configured, hostname) {
        (Some(s), _) if s.starts_with("mailto:") || s.starts_with("https:") => Ok(s.to_string()),
        (Some(_), _) => Err("MAILRS_VAPID_SUBJECT must be a mailto: or https: URI".into()),
        (None, Some(host)) => Ok(format!("mailto:postmaster@{host}")),
        (None, None) => Err("neither MAILRS_VAPID_SUBJECT nor MAILRS_HOSTNAME is set".into()),
    }
}

impl Vapid {
    /// From `MAILRS_VAPID_PRIVATE_KEY_PATH` and `MAILRS_VAPID_SUBJECT`.
    /// `Ok(None)` when the path is unset or no file is there: Web Push is
    /// off. Production names a fixed path and turns Web Push on by putting
    /// the key file there, because a blank secret variable stops the
    /// process at start.
    pub fn from_env() -> Result<Option<Vapid>, String> {
        let Ok(path) = std::env::var("MAILRS_VAPID_PRIVATE_KEY_PATH") else {
            return Ok(None);
        };
        let pem = match std::fs::read_to_string(&path) {
            Ok(pem) => pem,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(e) => return Err(format!("cannot read {path}: {e}")),
        };
        let subject = subject(
            std::env::var("MAILRS_VAPID_SUBJECT").ok().as_deref(),
            std::env::var("MAILRS_HOSTNAME").ok().as_deref(),
        )?;
        Vapid::from_pem(&pem, &subject)
            .map(Some)
            .map_err(|e| format!("VAPID key rejected: {e:?}"))
    }

    /// From a PEM `PRIVATE KEY` (PKCS#8) and the contact.
    pub fn from_pem(pem: &str, subject: &str) -> Result<Vapid, Error> {
        let body: String = pem
            .lines()
            .filter(|l| !l.starts_with("-----"))
            .collect::<Vec<_>>()
            .concat();
        let der = base64::engine::general_purpose::STANDARD
            .decode(body.trim())
            .map_err(|_| Error::BadKey)?;
        let key = EcdsaKeyPair::from_pkcs8(&ECDSA_P256_SHA256_FIXED_SIGNING, &der)
            .map_err(|_| Error::BadKey)?;
        Ok(Vapid {
            key,
            subject: subject.to_string(),
        })
    }

    /// The public key a browser subscribes with (`applicationServerKey`).
    pub fn public_key(&self) -> String {
        b64url(self.key.public_key().as_ref())
    }

    /// The `Authorization` header for a push to `endpoint` at `now`.
    pub fn authorization(&self, endpoint: &url::Url, now: i64) -> Result<String, Error> {
        let audience = endpoint.origin().ascii_serialization();
        let header = b64url(br#"{"typ":"JWT","alg":"ES256"}"#);
        let claims = serde_json::json!({
            "aud": audience,
            "exp": now + LIFETIME,
            "sub": self.subject,
        });
        let claims = b64url(claims.to_string().as_bytes());
        let signing_input = format!("{header}.{claims}");
        let sig = self
            .key
            .sign(&SystemRandom::new(), signing_input.as_bytes())
            .map_err(|_| Error::Sign)?;
        Ok(format!(
            "vapid t={signing_input}.{}, k={}",
            b64url(sig.as_ref()),
            self.public_key()
        ))
    }
}

#[cfg(test)]
mod tests {
    use aws_lc_rs::signature::{ECDSA_P256_SHA256_FIXED, UnparsedPublicKey};

    use super::*;

    #[test]
    fn the_contact_is_configured_or_the_hosts_postmaster() {
        assert_eq!(
            subject(Some("mailto:ops@x.test"), Some("mail.x.test")).unwrap(),
            "mailto:ops@x.test"
        );
        assert_eq!(
            subject(Some("  "), Some("mail.x.test")).unwrap(),
            "mailto:postmaster@mail.x.test"
        );
        assert_eq!(
            subject(None, Some("mail.x.test")).unwrap(),
            "mailto:postmaster@mail.x.test"
        );
        assert!(subject(Some("ops@x.test"), Some("mail.x.test")).is_err());
        assert!(subject(None, None).is_err());
    }
    use crate::from_b64url;

    fn pem() -> String {
        let doc =
            EcdsaKeyPair::generate_pkcs8(&ECDSA_P256_SHA256_FIXED_SIGNING, &SystemRandom::new())
                .unwrap();
        let b64 = base64::engine::general_purpose::STANDARD.encode(doc.as_ref());
        format!("-----BEGIN PRIVATE KEY-----\n{b64}\n-----END PRIVATE KEY-----\n")
    }

    /// The token verifies under the public key it names, and says who it is
    /// for, until when, and from whom.
    #[test]
    fn the_token_verifies_and_carries_the_claims() {
        let v = Vapid::from_pem(&pem(), "mailto:ops@example.test").unwrap();
        let endpoint = url::Url::parse("https://fcm.googleapis.com/fcm/send/abc").unwrap();
        let header = v.authorization(&endpoint, 1_000).unwrap();
        let (t, k) = header
            .strip_prefix("vapid t=")
            .unwrap()
            .split_once(", k=")
            .unwrap();
        assert_eq!(k, v.public_key());
        let parts: Vec<&str> = t.split('.').collect();
        assert_eq!(parts.len(), 3);
        let public = from_b64url(k).unwrap();
        UnparsedPublicKey::new(&ECDSA_P256_SHA256_FIXED, &public)
            .verify(
                format!("{}.{}", parts[0], parts[1]).as_bytes(),
                &from_b64url(parts[2]).unwrap(),
            )
            .expect("the signature verifies");
        let claims: serde_json::Value =
            serde_json::from_slice(&from_b64url(parts[1]).unwrap()).unwrap();
        assert_eq!(claims["aud"], "https://fcm.googleapis.com");
        assert_eq!(claims["exp"], 1_000 + LIFETIME);
        assert_eq!(claims["sub"], "mailto:ops@example.test");
        assert_eq!(public.len(), 65);
    }

    #[test]
    fn a_key_that_is_not_pkcs8_p256_is_refused() {
        assert_eq!(
            Vapid::from_pem(
                "-----BEGIN PRIVATE KEY-----\nAAAA\n-----END PRIVATE KEY-----",
                "x"
            )
            .err(),
            Some(Error::BadKey)
        );
    }
}

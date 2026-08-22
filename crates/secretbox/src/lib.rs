#![deny(missing_docs)]
#![doc = include_str!("../README.md")]

use base64::Engine as _;
use chacha20poly1305::aead::{Aead, KeyInit, OsRng, rand_core::RngCore};
use chacha20poly1305::{XChaCha20Poly1305, XNonce};
use sha2::{Digest, Sha256};

/// What went wrong reading a sealed value.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Error {
    /// Not a sealed value at all — wrong shape, wrong version, or not
    /// base64. A store row that says this was written by something
    /// else.
    Malformed,
    /// Sealed under a key this deployment does not have.
    ///
    /// Named rather than lumped in with corruption because the two need
    /// different answers: a rotated key is recoverable by putting the
    /// old one back, and corruption is not.
    WrongKey {
        /// The fingerprint the value was sealed under.
        sealed_with: String,
        /// The fingerprint of the key that was offered.
        offered: String,
    },
    /// The right key, but the bytes do not authenticate — truncated,
    /// altered, or corrupt.
    Tampered,
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Malformed => write!(f, "not a sealed value"),
            Self::WrongKey {
                sealed_with,
                offered,
            } => write!(
                f,
                "sealed under key {sealed_with}, this deployment has {offered}"
            ),
            Self::Tampered => write!(f, "sealed value does not authenticate"),
        }
    }
}

impl std::error::Error for Error {}

/// The deployment's sealing key.
///
/// Held in memory and never written anywhere by this crate — where it
/// comes from is the caller's problem, and the answer in mailrs is the
/// `MAILRS_ACCOUNT_KEY` environment variable.
#[derive(Clone)]
pub struct Key {
    bytes: [u8; 32],
    fingerprint: String,
}

impl std::fmt::Debug for Key {
    /// Deliberately not the key. A `Debug` that prints key material
    /// ends up in a log line eventually.
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Key({})", self.fingerprint)
    }
}

impl Key {
    /// Derive a key from whatever the deployment configured.
    ///
    /// SHA-256 of the passphrase, which is a *hash* and not a password
    /// KDF: this input is a machine-generated deployment secret held in
    /// the environment, not something a person chose, so there is no
    /// dictionary to stretch against. A passphrase a person typed
    /// should be run through argon2 first — by the caller, which knows
    /// which of the two it has.
    pub fn from_passphrase(passphrase: &str) -> Self {
        let mut bytes = [0u8; 32];
        bytes.copy_from_slice(&Sha256::digest(passphrase.as_bytes()));
        Self::from_bytes(bytes)
    }

    /// A key from raw bytes.
    pub fn from_bytes(bytes: [u8; 32]) -> Self {
        // Fingerprint the *key*, not the passphrase, so two ways of
        // arriving at the same key agree.
        let digest = Sha256::digest(bytes);
        let fingerprint = digest[..8].iter().map(|b| format!("{b:02x}")).collect();
        Self { bytes, fingerprint }
    }

    /// Sixteen hex characters naming this key, safe to store and log.
    pub fn fingerprint(&self) -> &str {
        &self.fingerprint
    }
}

const VERSION: &str = "v1";
const NONCE_LEN: usize = 24;

/// Seal a secret. The result is safe to store and useless without the key.
pub fn seal(key: &Key, secret: &[u8]) -> Result<String, Error> {
    let cipher = XChaCha20Poly1305::new((&key.bytes).into());
    let mut nonce = [0u8; NONCE_LEN];
    OsRng.fill_bytes(&mut nonce);
    let ciphertext = cipher
        .encrypt(XNonce::from_slice(&nonce), secret)
        .map_err(|_| Error::Tampered)?;
    let mut body = nonce.to_vec();
    body.extend_from_slice(&ciphertext);
    let b64 = base64::engine::general_purpose::STANDARD_NO_PAD.encode(body);
    Ok(format!("{VERSION}.{}.{b64}", key.fingerprint))
}

/// Open a sealed value.
pub fn open(key: &Key, sealed: &str) -> Result<Vec<u8>, Error> {
    let mut parts = sealed.splitn(3, '.');
    let (Some(version), Some(fingerprint), Some(b64)) = (parts.next(), parts.next(), parts.next())
    else {
        return Err(Error::Malformed);
    };
    if version != VERSION || fingerprint.len() != 16 {
        return Err(Error::Malformed);
    }
    // Asked before decoding: a rotated key is a different answer from a
    // corrupt row, and the caller can only tell them apart if this does.
    if fingerprint != key.fingerprint {
        return Err(Error::WrongKey {
            sealed_with: fingerprint.to_string(),
            offered: key.fingerprint.clone(),
        });
    }
    let body = base64::engine::general_purpose::STANDARD_NO_PAD
        .decode(b64)
        .map_err(|_| Error::Malformed)?;
    if body.len() < NONCE_LEN {
        return Err(Error::Malformed);
    }
    let (nonce, ciphertext) = body.split_at(NONCE_LEN);
    XChaCha20Poly1305::new((&key.bytes).into())
        .decrypt(XNonce::from_slice(nonce), ciphertext)
        .map_err(|_| Error::Tampered)
}

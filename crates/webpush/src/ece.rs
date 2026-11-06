//! RFC 8291: a push message encrypted so only the subscribing browser can
//! read it, in the `aes128gcm` content coding of RFC 8188.

use aws_lc_rs::aead::{AES_128_GCM, Aad, LessSafeKey, Nonce, UnboundKey};
use aws_lc_rs::agreement::{self, ECDH_P256, PrivateKey, UnparsedPublicKey};
use aws_lc_rs::rand::{SecureRandom, SystemRandom};
use hkdf::Hkdf;
use sha2::Sha256;

/// The record size written into the header; one record is enough for a
/// notification.
const RECORD_SIZE: u32 = 4096;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Error {
    /// The subscription's public key is not an uncompressed P-256 point.
    BadKey,
    /// The authentication secret is not 16 bytes.
    BadAuth,
    /// The message is larger than one record.
    TooLong,
    Crypto,
}

/// What a subscription gives the sender: the browser's public key and its
/// authentication secret.
pub struct Receiver<'a> {
    /// 65 bytes, uncompressed.
    pub public: &'a [u8],
    /// 16 bytes.
    pub auth: &'a [u8],
}

/// Encrypt `plaintext` for `to` with a fresh key and salt.
pub fn encrypt(plaintext: &[u8], to: &Receiver<'_>) -> Result<Vec<u8>, Error> {
    let sender = PrivateKey::generate(&ECDH_P256).map_err(|_| Error::Crypto)?;
    let mut salt = [0u8; 16];
    SystemRandom::new()
        .fill(&mut salt)
        .map_err(|_| Error::Crypto)?;
    encrypt_with(plaintext, to, &sender, &salt)
}

/// [`encrypt`] with the sender's key and the salt given: for the RFC's test
/// vector.
pub fn encrypt_with(
    plaintext: &[u8],
    to: &Receiver<'_>,
    sender: &PrivateKey,
    salt: &[u8; 16],
) -> Result<Vec<u8>, Error> {
    if to.public.len() != 65 || to.public[0] != 4 {
        return Err(Error::BadKey);
    }
    if to.auth.len() != 16 {
        return Err(Error::BadAuth);
    }
    // the delimiter and the tag must fit in the one record
    if plaintext.len() + 1 + 16 > RECORD_SIZE as usize {
        return Err(Error::TooLong);
    }
    let as_public = sender.compute_public_key().map_err(|_| Error::Crypto)?;
    let as_public = as_public.as_ref().to_vec();
    let ecdh_secret = agreement::agree(
        sender,
        UnparsedPublicKey::new(&ECDH_P256, to.public),
        Error::BadKey,
        |s| Ok(s.to_vec()),
    )?;

    // RFC 8291 §3.4: combine the shared secret with the auth secret
    let mut key_info = b"WebPush: info\0".to_vec();
    key_info.extend_from_slice(to.public);
    key_info.extend_from_slice(&as_public);
    let mut ikm = [0u8; 32];
    Hkdf::<Sha256>::new(Some(to.auth), &ecdh_secret)
        .expand(&key_info, &mut ikm)
        .map_err(|_| Error::Crypto)?;

    // RFC 8188 §2.2–2.3: the content key and the nonce
    let prk = Hkdf::<Sha256>::new(Some(salt), &ikm);
    let mut cek = [0u8; 16];
    prk.expand(b"Content-Encoding: aes128gcm\0", &mut cek)
        .map_err(|_| Error::Crypto)?;
    let mut nonce = [0u8; 12];
    prk.expand(b"Content-Encoding: nonce\0", &mut nonce)
        .map_err(|_| Error::Crypto)?;

    let key = LessSafeKey::new(UnboundKey::new(&AES_128_GCM, &cek).map_err(|_| Error::Crypto)?);
    let mut record = plaintext.to_vec();
    // the last (and only) record ends with 0x02
    record.push(2);
    key.seal_in_place_append_tag(
        Nonce::assume_unique_for_key(nonce),
        Aad::empty(),
        &mut record,
    )
    .map_err(|_| Error::Crypto)?;

    let mut out = Vec::with_capacity(16 + 4 + 1 + 65 + record.len());
    out.extend_from_slice(salt);
    out.extend_from_slice(&RECORD_SIZE.to_be_bytes());
    out.push(as_public.len() as u8);
    out.extend_from_slice(&as_public);
    out.extend_from_slice(&record);
    Ok(out)
}

/// The receiving side of [`encrypt`]: what `receiver` (the subscriber's
/// private key) reads from `body`. A device does this; the server uses it to
/// check what it sent.
pub fn decrypt(body: &[u8], receiver: &PrivateKey, auth: &[u8]) -> Result<Vec<u8>, Error> {
    let header = body.get(..21).ok_or(Error::Crypto)?;
    let salt = &header[..16];
    let id_len = usize::from(header[20]);
    let as_public = body.get(21..21 + id_len).ok_or(Error::BadKey)?;
    let mut record = body.get(21 + id_len..).ok_or(Error::Crypto)?.to_vec();
    let ua_public = receiver.compute_public_key().map_err(|_| Error::Crypto)?;
    let ecdh_secret = agreement::agree(
        receiver,
        UnparsedPublicKey::new(&ECDH_P256, as_public),
        Error::BadKey,
        |s| Ok(s.to_vec()),
    )?;
    let mut key_info = b"WebPush: info\0".to_vec();
    key_info.extend_from_slice(ua_public.as_ref());
    key_info.extend_from_slice(as_public);
    let mut ikm = [0u8; 32];
    Hkdf::<Sha256>::new(Some(auth), &ecdh_secret)
        .expand(&key_info, &mut ikm)
        .map_err(|_| Error::Crypto)?;
    let prk = Hkdf::<Sha256>::new(Some(salt), &ikm);
    let mut cek = [0u8; 16];
    prk.expand(b"Content-Encoding: aes128gcm\0", &mut cek)
        .map_err(|_| Error::Crypto)?;
    let mut nonce = [0u8; 12];
    prk.expand(b"Content-Encoding: nonce\0", &mut nonce)
        .map_err(|_| Error::Crypto)?;
    let key = LessSafeKey::new(UnboundKey::new(&AES_128_GCM, &cek).map_err(|_| Error::Crypto)?);
    let plain = key
        .open_in_place(
            Nonce::assume_unique_for_key(nonce),
            Aad::empty(),
            &mut record,
        )
        .map_err(|_| Error::Crypto)?;
    // padding zeros, then the last record's 0x02 delimiter
    let end = plain.iter().rposition(|b| *b != 0).ok_or(Error::Crypto)?;
    match plain[end] {
        2 => Ok(plain[..end].to_vec()),
        _ => Err(Error::Crypto),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{b64url, from_b64url};

    #[test]
    fn the_rfc_8291_example_decrypts_with_the_receivers_key() {
        let d = |s: &str| from_b64url(&s.replace(' ', "")).unwrap();
        let ua_private = d("q1dXpw3UpT5VOmu_cf_v6ih07Aems3njxI-JWgLcM94");
        let receiver = PrivateKey::from_private_key(&ECDH_P256, &ua_private).unwrap();
        let body = d(
            "DGv6ra1nlYgDCS1FRnbzlwAAEABBBP4z9KsN6nGRTbVYI_c7VJSPQTBtkgcy27ml\
             mlMoZIIgDll6e3vCYLocInmYWAmS6TlzAC8wEqKK6PBru3jl7A_yl95bQpu6cVPT\
             pK4Mqgkf1CXztLVBSt2Ks3oZwbuwXPXLWyouBWLVWGNWQexSgSxsj_Qulcy4a-fN",
        );
        let auth = d("BTBZMqHH6r4Tts7J_aSIgg");
        assert_eq!(
            decrypt(&body, &receiver, &auth).unwrap(),
            b"When I grow up, I want to be a watermelon"
        );
        assert_eq!(
            decrypt(&body, &receiver, &[0u8; 16]),
            Err(Error::Crypto),
            "the wrong auth secret does not open it"
        );
    }

    /// RFC 8291 §5 and Appendix A, byte for byte.
    #[test]
    fn the_rfc_8291_example_encrypts_to_the_published_body() {
        let d = |s: &str| from_b64url(&s.replace(' ', "")).unwrap();
        let ua_public = d(
            "BCVxsr7N_eNgVRqvHtD0zTZsEc6-VV-JvLexhqUzORcxaOzi6-AYWXvTBHm4bjyPjs7Vd8pZGH6SRpkNtoIAiw4",
        );
        let auth = d("BTBZMqHH6r4Tts7J_aSIgg");
        let as_private = d("yfWPiYE-n46HLnH0KqZOF1fJJU3MYrct3AELtAQ-oRw");
        let salt: [u8; 16] = d("DGv6ra1nlYgDCS1FRnbzlw").try_into().unwrap();
        let sender = PrivateKey::from_private_key(&ECDH_P256, &as_private).unwrap();
        assert_eq!(
            b64url(sender.compute_public_key().unwrap().as_ref()),
            "BP4z9KsN6nGRTbVYI_c7VJSPQTBtkgcy27mlmlMoZIIgDll6e3vCYLocInmYWAmS6TlzAC8wEqKK6PBru3jl7A8"
        );
        let body = encrypt_with(
            b"When I grow up, I want to be a watermelon",
            &Receiver {
                public: &ua_public,
                auth: &auth,
            },
            &sender,
            &salt,
        )
        .unwrap();
        assert_eq!(
            b64url(&body),
            "DGv6ra1nlYgDCS1FRnbzlwAAEABBBP4z9KsN6nGRTbVYI_c7VJSPQTBtkgcy27ml\
             mlMoZIIgDll6e3vCYLocInmYWAmS6TlzAC8wEqKK6PBru3jl7A_yl95bQpu6cVPT\
             pK4Mqgkf1CXztLVBSt2Ks3oZwbuwXPXLWyouBWLVWGNWQexSgSxsj_Qulcy4a-fN"
        );
    }

    #[test]
    fn a_bad_subscription_key_or_secret_is_refused() {
        let key = [4u8; 65];
        let short = [0u8; 15];
        let r = Receiver {
            public: &key[..64],
            auth: &[0u8; 16],
        };
        assert_eq!(encrypt(b"x", &r), Err(Error::BadKey));
        let r = Receiver {
            public: &key,
            auth: &short,
        };
        assert_eq!(encrypt(b"x", &r), Err(Error::BadAuth));
    }
}

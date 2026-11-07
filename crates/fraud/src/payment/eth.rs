//! Ethereum addresses, counted only when their EIP-55 checksum holds.
//!
//! EIP-55 writes the checksum into the case of the address's hex letters:
//! a letter is upper case exactly where the matching nibble of the
//! Keccak-256 hash of the lower-case address is 8 or more. An address in
//! one case only carries no checksum, and so proves nothing: a string of
//! forty hex characters is as likely a hash or a tracking id.

use sha3::{Digest, Keccak256};

/// `word` is `0x` and forty hex characters in mixed case, and the case
/// is the address's EIP-55 checksum.
pub(super) fn is_checksummed(word: &str) -> bool {
    let Some(hex) = word.strip_prefix("0x") else {
        return false;
    };
    if hex.len() != 40 || !hex.bytes().all(|b| b.is_ascii_hexdigit()) {
        return false;
    }
    let has_upper = hex.bytes().any(|b| b.is_ascii_uppercase());
    let has_lower = hex.bytes().any(|b| b.is_ascii_lowercase());
    if !(has_upper && has_lower) {
        return false;
    }
    let lower = hex.to_ascii_lowercase();
    let hash = Keccak256::digest(lower.as_bytes());
    hex.bytes().enumerate().all(|(i, b)| {
        let nibble = match i % 2 {
            0 => hash[i / 2] >> 4,
            _ => hash[i / 2] & 0x0f,
        };
        match (b.is_ascii_alphabetic(), nibble >= 8) {
            (false, _) => true,
            (true, true) => b.is_ascii_uppercase(),
            (true, false) => b.is_ascii_lowercase(),
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    // the mixed-case test vectors published with EIP-55
    const VECTORS: [&str; 4] = [
        "0x5aAeb6053F3E94C9b9A09f33669435E7Ef1BeAed",
        "0xfB6916095ca1df60bB79Ce92cE3Ea74c37c5d359",
        "0xdbF03B407c01E7cD3CBea99509d93f8DDDC8C6FB",
        "0xD1220A0cf47c7B9Be7A2E6BA89F429762e7b9aDb",
    ];

    #[test]
    fn the_published_vectors_pass() {
        for v in VECTORS {
            assert!(is_checksummed(v), "{v}");
        }
    }

    #[test]
    fn one_letter_in_the_wrong_case_fails() {
        for v in VECTORS {
            let i = v[2..].find(|c: char| c.is_ascii_alphabetic()).unwrap() + 2;
            let c = v.as_bytes()[i] as char;
            let flipped = match c.is_ascii_uppercase() {
                true => c.to_ascii_lowercase(),
                false => c.to_ascii_uppercase(),
            };
            let near = format!("{}{flipped}{}", &v[..i], &v[i + 1..]);
            assert!(!is_checksummed(&near), "{near}");
        }
    }

    #[test]
    fn one_case_only_carries_no_checksum() {
        for v in VECTORS {
            let lower = format!("0x{}", v[2..].to_ascii_lowercase());
            let upper = format!("0x{}", v[2..].to_ascii_uppercase());
            assert!(!is_checksummed(&lower), "{lower}");
            assert!(!is_checksummed(&upper), "{upper}");
        }
        // the all-caps vectors EIP-55 lists are valid addresses, but not
        // a checksum the message proves
        assert!(!is_checksummed(
            "0x52908400098527886E0F7030069857D2E4169EE7"
        ));
    }

    #[test]
    fn the_wrong_length_or_prefix_is_not_an_address() {
        assert!(!is_checksummed("0x5aAeb6053F3E94C9b9A09f33669435E7Ef1BeAe"));
        assert!(!is_checksummed("5aAeb6053F3E94C9b9A09f33669435E7Ef1BeAed"));
        assert!(!is_checksummed(
            "0X5aAeb6053F3E94C9b9A09f33669435E7Ef1BeAed"
        ));
    }
}

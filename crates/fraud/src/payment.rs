//! Payment identifiers a message carries, recognised only when their own
//! check digits hold.
//!
//! A sum, an "urgent transfer", "our new account" are words, and words
//! are not evidence here. An IBAN whose mod-97 check passes, or a Bitcoin
//! address whose checksum does, is something the message itself proves
//! it contains. What it means depends on who sent it, which is the rules'
//! business, not this module's.

use sha2::{Digest, Sha256};

/// The identifiers found in a text.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct PaymentIds {
    /// An IBAN whose mod-97 check passes.
    pub iban: bool,
    /// The country code of the first such IBAN, for the reader.
    pub iban_country: Option<&'static str>,
    /// A Bitcoin address whose checksum passes.
    pub crypto: bool,
}

/// Look for check-digit-valid IBANs and Bitcoin addresses in `text`.
#[must_use]
pub fn find(text: &str) -> PaymentIds {
    let mut out = PaymentIds::default();
    for token in text.split(|c: char| !c.is_ascii_alphanumeric() && c != ' ') {
        if out.iban {
            break;
        }
        if let Some(country) = iban_in(token) {
            out.iban = true;
            out.iban_country = Some(country);
        }
    }
    for word in text.split(|c: char| !c.is_ascii_alphanumeric()) {
        if !out.crypto && (is_base58_address(word) || is_bech32_address(word)) {
            out.crypto = true;
        }
    }
    out
}

/// IBAN lengths by country (ISO 13616 registry, the countries mail from
/// and to this deployment names).
const IBAN_LENGTHS: &[(&str, usize)] = &[
    ("AD", 24),
    ("AE", 23),
    ("AT", 20),
    ("BE", 16),
    ("BG", 22),
    ("CH", 21),
    ("CY", 28),
    ("CZ", 24),
    ("DE", 22),
    ("DK", 18),
    ("EE", 20),
    ("ES", 24),
    ("FI", 18),
    ("FR", 27),
    ("GB", 22),
    ("GR", 27),
    ("HR", 21),
    ("HU", 28),
    ("IE", 22),
    ("IL", 23),
    ("IS", 26),
    ("IT", 27),
    ("LI", 21),
    ("LT", 20),
    ("LU", 20),
    ("LV", 21),
    ("MC", 27),
    ("MT", 31),
    ("NL", 18),
    ("NO", 15),
    ("PL", 28),
    ("PT", 25),
    ("RO", 24),
    ("SA", 24),
    ("SE", 24),
    ("SI", 19),
    ("SK", 24),
    ("SM", 27),
    ("TR", 26),
    ("UA", 29),
];

/// Whether a word of `chunk` begins an IBAN: from each word start, the
/// spaces print groups use are taken out and the country's length read.
/// The country of the first check-digit-valid IBAN starting a word of `chunk`.
fn iban_in(chunk: &str) -> Option<&'static str> {
    let bytes = chunk.as_bytes();
    (0..bytes.len()).find_map(|start| {
        let word_start = start == 0 || bytes[start - 1] == b' ';
        if !word_start || bytes.len() - start < 15 {
            return None;
        }
        let &(cc, len) = IBAN_LENGTHS
            .iter()
            .find(|(cc, _)| bytes[start..start + 2].eq_ignore_ascii_case(cc.as_bytes()))?;
        let compact: Vec<u8> = bytes[start..]
            .iter()
            .copied()
            .filter(|b| *b != b' ')
            .take(len)
            .collect();
        match compact.len() == len && iban_checks(&compact) {
            true => Some(cc),
            false => None,
        }
    })
}

fn iban_checks(iban: &[u8]) -> bool {
    if !iban[2].is_ascii_digit() || !iban[3].is_ascii_digit() {
        return false;
    }
    let mut remainder: u32 = 0;
    for &b in iban[4..].iter().chain(&iban[..4]) {
        let value = match b {
            b'0'..=b'9' => u32::from(b - b'0'),
            b'A'..=b'Z' => u32::from(b - b'A') + 10,
            b'a'..=b'z' => u32::from(b - b'a') + 10,
            _ => return false,
        };
        let shift = match value >= 10 {
            true => 100,
            false => 10,
        };
        remainder = (remainder * shift + value) % 97;
    }
    remainder == 1
}

const BASE58: &[u8] = b"123456789ABCDEFGHJKLMNPQRSTUVWXYZabcdefghijkmnopqrstuvwxyz";

/// A legacy (P2PKH) or script (P2SH) Bitcoin address whose base58check
/// checksum holds.
fn is_base58_address(word: &str) -> bool {
    if !(26..=35).contains(&word.len()) || !(word.starts_with('1') || word.starts_with('3')) {
        return false;
    }
    let mut bytes = [0u8; 25];
    for c in word.bytes() {
        let Some(mut carry) = BASE58.iter().position(|&a| a == c).map(|d| d as u32) else {
            return false;
        };
        for b in bytes.iter_mut().rev() {
            carry += u32::from(*b) * 58;
            *b = (carry & 0xff) as u8;
            carry >>= 8;
        }
        if carry != 0 {
            return false;
        }
    }
    let check = Sha256::digest(Sha256::digest(&bytes[..21]));
    check[..4] == bytes[21..]
}

const BECH32: &[u8] = b"qpzry9x8gf2tvdw0s3jn54khce6mua7l";

/// A segwit Bitcoin address (`bc1…`) whose bech32 or bech32m checksum
/// holds.
fn is_bech32_address(word: &str) -> bool {
    let lower = word.to_ascii_lowercase();
    if !(14..=74).contains(&lower.len()) || !lower.starts_with("bc1") {
        return false;
    }
    if word != lower && word != word.to_ascii_uppercase() {
        return false;
    }
    let Some(data) = lower[3..]
        .bytes()
        .map(|c| BECH32.iter().position(|&a| a == c).map(|d| d as u8))
        .collect::<Option<Vec<u8>>>()
    else {
        return false;
    };
    let mut values = vec![3u8, 3, 0, 2, 3];
    values.extend(&data);
    let check = polymod(&values);
    check == 1 || check == 0x2bc8_30a3
}

fn polymod(values: &[u8]) -> u32 {
    const GEN: [u32; 5] = [
        0x3b6a_57b2,
        0x2650_8e6d,
        0x1ea1_19fa,
        0x3d42_33dd,
        0x2a14_62b3,
    ];
    let mut chk: u32 = 1;
    for &v in values {
        let top = chk >> 25;
        chk = ((chk & 0x01ff_ffff) << 5) ^ u32::from(v);
        for (i, g) in GEN.iter().enumerate() {
            if (top >> i) & 1 == 1 {
                chk ^= g;
            }
        }
    }
    chk
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_valid_iban_is_found_printed_or_compact() {
        assert!(find("please pay DE89 3704 0044 0532 0130 00 today").iban);
        assert!(find("IBAN: GB82WEST12345698765432").iban);
        assert!(find("FR14 2004 1010 0505 0001 3M02 606").iban);
    }

    #[test]
    fn a_wrong_check_digit_is_not_an_iban() {
        assert!(!find("DE88 3704 0044 0532 0130 00").iban);
        assert!(!find("order DE123456789 shipped").iban);
        assert!(!find("tracking 1Z999AA10123456784").iban);
    }

    #[test]
    fn bitcoin_addresses_are_found_by_their_checksums() {
        // the genesis block's address, and BIP-173 / BIP-350 test vectors
        assert!(find("send to 1A1zP1eP5QGefi2DMPTfTL5SLmv7DivfNa now").crypto);
        assert!(find("bc1qw508d6qejxtdg4y5r3zarvary0c5xw7kv8f3t4").crypto);
        assert!(find("bc1p0xlxvlhemja6c4dqv22uapctqupfhlxm9h8z3k2e72q4k9hcz7vqzk5jj0").crypto);
    }

    #[test]
    fn near_misses_are_not_addresses() {
        assert!(!find("1A1zP1eP5QGefi2DMPTfTL5SLmv7DivfNb").crypto);
        assert!(!find("bc1qw508d6qejxtdg4y5r3zarvary0c5xw7kv8f3t5").crypto);
        assert!(!find("ref 3J98t1WpEZ73CNmQviecrnyiWrnqRhWNLx is it").crypto);
        // a script address is an address too
        assert!(find("ref 3J98t1WpEZ73CNmQviecrnyiWrnqRhWNLy is it").crypto);
    }
}

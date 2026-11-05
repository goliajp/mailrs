//! The entry names of a RAR archive, read from its block headers.
//!
//! RAR 1.5–4.x and RAR 5 both store each member's name in a plain header
//! in front of its data, unless the archive was made with header
//! encryption (`rar -hp`), in which case nothing but the signature is
//! readable. Walking the headers reads a table of names: nothing is
//! decompressed, every length the archive declares is checked against
//! the bytes that are actually there before it is used, and no buffer is
//! sized from a declared length, so the work is bounded by the
//! attachment's size whatever it claims.
//!
//! The listing is whole only when the end-of-archive block is reached;
//! running out of bytes before it is a listing that broke off.
//!
//! RAR 1.3 (`RE~^`) is not read: no current tool writes it.

use super::{ENTRIES_MAX, Entry, Listing};

const RAR4: &[u8] = b"Rar!\x1a\x07\x00";
const RAR5: &[u8] = b"Rar!\x1a\x07\x01\x00";

/// RAR 5 caps a name at 2048 bytes; a longer one is not a RAR 5 name.
const NAME_MAX: usize = 2048;

/// The listing of a RAR archive, or `None` when `b` is not one.
pub(super) fn listing(b: &[u8]) -> Option<Listing> {
    if b.starts_with(RAR5) {
        Some(rar5(b, RAR5.len()))
    } else if b.starts_with(RAR4) {
        Some(rar4(b, RAR4.len()))
    } else {
        None
    }
}

fn u16_at(b: &[u8], at: usize) -> Option<u16> {
    Some(u16::from_le_bytes(
        b.get(at..at.checked_add(2)?)?.try_into().ok()?,
    ))
}

fn u32_at(b: &[u8], at: usize) -> Option<u32> {
    Some(u32::from_le_bytes(
        b.get(at..at.checked_add(4)?)?.try_into().ok()?,
    ))
}

/// RAR 1.5–4.x: a 7-byte block head (crc, type, flags, size), then for a
/// file block the packed size, which is where the next block starts
/// after the header.
fn rar4(b: &[u8], mut at: usize) -> Listing {
    const MAIN: u8 = 0x73;
    const FILE: u8 = 0x74;
    const END: u8 = 0x7b;
    const MAIN_HEADERS_ENCRYPTED: u16 = 0x0080;
    const LONG_BLOCK: u16 = 0x8000;
    const FILE_ENCRYPTED: u16 = 0x0004;
    const FILE_DIRECTORY: u16 = 0x00e0;
    const FILE_LARGE: u16 = 0x0100;
    const FILE_UNICODE: u16 = 0x0200;

    let mut out = Listing::default();
    loop {
        let (Some(kind), Some(flags), Some(size)) =
            (b.get(at + 2).copied(), u16_at(b, at + 3), u16_at(b, at + 5))
        else {
            return out;
        };
        let size = usize::from(size);
        if size < 7 {
            return out;
        }
        let Some(header) = at.checked_add(size).and_then(|end| b.get(at..end)) else {
            return out;
        };
        let mut data = 0u64;
        match kind {
            MAIN if flags & MAIN_HEADERS_ENCRYPTED != 0 => return out,
            END => {
                out.whole = true;
                return out;
            }
            FILE => {
                if out.entries.len() >= ENTRIES_MAX {
                    return out;
                }
                let (Some(packed), Some(name_len)) = (u32_at(header, 7), u16_at(header, 26)) else {
                    return out;
                };
                let mut name_at = 32;
                data = u64::from(packed);
                if flags & FILE_LARGE != 0 {
                    let Some(high) = u32_at(header, 32) else {
                        return out;
                    };
                    data |= u64::from(high) << 32;
                    name_at = 40;
                }
                let Some(raw) = header.get(name_at..name_at + usize::from(name_len)) else {
                    return out;
                };
                let name = match flags & FILE_UNICODE != 0 {
                    true => rar4_unicode_name(raw),
                    false => String::from_utf8_lossy(raw).into_owned(),
                };
                out.entries.push(Entry {
                    name,
                    directory: flags & FILE_DIRECTORY == FILE_DIRECTORY,
                    encrypted: flags & FILE_ENCRYPTED != 0,
                });
            }
            _ if flags & LONG_BLOCK != 0 => {
                let Some(add) = u32_at(header, 7) else {
                    return out;
                };
                data = u64::from(add);
            }
            _ => {}
        }
        let Some(next) = usize::try_from(data)
            .ok()
            .and_then(|d| (at + size).checked_add(d))
            .filter(|&n| n <= b.len())
        else {
            return out;
        };
        at = next;
    }
}

/// A RAR 4 name stored as `oem name, NUL, encoded UTF-16`: the encoding
/// is unrar's `EncodeFileName`, which describes each character as a copy
/// of the OEM byte, a byte under a shared high byte, or a full 16-bit
/// unit. The UTF-16 form is the one archivers show, so it is the one
/// judged; without it the OEM name is all there is.
fn rar4_unicode_name(raw: &[u8]) -> String {
    let Some(nul) = raw.iter().position(|&c| c == 0) else {
        return String::from_utf8_lossy(raw).into_owned();
    };
    let (oem, enc) = (&raw[..nul], &raw[nul + 1..]);
    let mut out: Vec<u16> = Vec::new();
    let mut pos = 0usize;
    let Some(&high) = enc.get(pos) else {
        return String::from_utf8_lossy(oem).into_owned();
    };
    let high = u16::from(high) << 8;
    pos += 1;
    let (mut flags, mut bits) = (0u8, 0u32);
    while pos < enc.len() && out.len() < NAME_MAX {
        if bits == 0 {
            flags = enc[pos];
            pos += 1;
            bits = 8;
        }
        match flags >> 6 {
            0 => {
                let Some(&c) = enc.get(pos) else { break };
                out.push(u16::from(c));
                pos += 1;
            }
            1 => {
                let Some(&c) = enc.get(pos) else { break };
                out.push(u16::from(c) | high);
                pos += 1;
            }
            2 => {
                let Some(pair) = enc.get(pos..pos + 2) else {
                    break;
                };
                out.push(u16::from_le_bytes([pair[0], pair[1]]));
                pos += 2;
            }
            _ => {
                let Some(&len) = enc.get(pos) else { break };
                pos += 1;
                let correction = match len & 0x80 != 0 {
                    true => {
                        let Some(&c) = enc.get(pos) else { break };
                        pos += 1;
                        Some(c)
                    }
                    false => None,
                };
                for _ in 0..usize::from(len & 0x7f) + 2 {
                    let Some(&o) = oem.get(out.len()) else { break };
                    if out.len() >= NAME_MAX {
                        break;
                    }
                    out.push(match correction {
                        Some(c) => u16::from(o.wrapping_add(c)) | high,
                        None => u16::from(o),
                    });
                }
            }
        }
        flags <<= 2;
        bits -= 2;
    }
    String::from_utf16_lossy(&out)
}

/// A RAR 5 variable-length integer: seven bits a byte, low first, at
/// most ten bytes. `None` when it runs off the end or past 64 bits.
fn vint(b: &[u8], at: &mut usize) -> Option<u64> {
    let mut v = 0u64;
    for i in 0..10 {
        let byte = *b.get(*at)?;
        *at += 1;
        let part = u64::from(byte & 0x7f);
        if i == 9 && part > 1 {
            return None;
        }
        v |= part << (7 * i);
        if byte & 0x80 == 0 {
            return Some(v);
        }
    }
    None
}

/// A declared length, kept only if that many bytes remain after `at`.
fn within(b: &[u8], at: usize, len: u64) -> Option<usize> {
    let len = usize::try_from(len).ok()?;
    (len <= b.len().checked_sub(at)?).then_some(len)
}

/// RAR 5: each block is `crc32, size, type, flags, [extra size], [data
/// size], fields, extra area`, then the data.
fn rar5(b: &[u8], mut at: usize) -> Listing {
    const FILE: u64 = 2;
    const ENCRYPTION: u64 = 4;
    const END: u64 = 5;
    const HAS_EXTRA: u64 = 0x01;
    const HAS_DATA: u64 = 0x02;
    const FILE_DIRECTORY: u64 = 0x01;
    const FILE_HAS_TIME: u64 = 0x02;
    const FILE_HAS_CRC: u64 = 0x04;
    const EXTRA_ENCRYPTION: u64 = 0x01;

    let mut out = Listing::default();
    loop {
        let mut p = at + 4;
        let Some(size) = vint(b, &mut p).and_then(|s| within(b, p, s)) else {
            return out;
        };
        if size == 0 {
            return out;
        }
        let header = &b[p..p + size];
        let after_header = p + size;
        let mut h = 0usize;
        let (Some(kind), Some(flags)) = (vint(header, &mut h), vint(header, &mut h)) else {
            return out;
        };
        let extra = match flags & HAS_EXTRA != 0 {
            true => vint(header, &mut h),
            false => Some(0),
        };
        let data = match flags & HAS_DATA != 0 {
            true => vint(header, &mut h),
            false => Some(0),
        };
        let (Some(extra), Some(data)) = (extra, data) else {
            return out;
        };
        let Some(extra) = within(header, h, extra) else {
            return out;
        };
        match kind {
            ENCRYPTION => return out,
            END => {
                out.whole = true;
                return out;
            }
            FILE => {
                if out.entries.len() >= ENTRIES_MAX {
                    return out;
                }
                let Some(mut entry) = rar5_file(header, h, size - extra) else {
                    return out;
                };
                entry.encrypted = rar5_extra_has(&header[size - extra..], EXTRA_ENCRYPTION);
                out.entries.push(entry);
            }
            _ => {}
        }
        let Some(next) = within(b, after_header, data).map(|d| after_header + d) else {
            return out;
        };
        at = next;
    }

    /// A file header's fields, which end where its extra area begins.
    fn rar5_file(header: &[u8], mut h: usize, fields_end: usize) -> Option<Entry> {
        let fields = header.get(..fields_end)?;
        let file_flags = vint(fields, &mut h)?;
        let _unpacked = vint(fields, &mut h)?;
        let _attributes = vint(fields, &mut h)?;
        if file_flags & FILE_HAS_TIME != 0 {
            h = h.checked_add(4)?;
        }
        if file_flags & FILE_HAS_CRC != 0 {
            h = h.checked_add(4)?;
        }
        let _compression = vint(fields, &mut h)?;
        let _host = vint(fields, &mut h)?;
        let len = vint(fields, &mut h)?;
        let len = within(fields, h, len).filter(|&l| l <= NAME_MAX)?;
        Some(Entry {
            name: String::from_utf8_lossy(&fields[h..h + len]).into_owned(),
            directory: file_flags & FILE_DIRECTORY != 0,
            encrypted: false,
        })
    }
}

/// Whether a RAR 5 extra area holds a record of type `kind`. Records are
/// `size, type, data`, the size counting type and data.
fn rar5_extra_has(area: &[u8], kind: u64) -> bool {
    let mut at = 0usize;
    while at < area.len() {
        let mut p = at;
        let Some(size) = vint(area, &mut p).and_then(|s| within(area, p, s)) else {
            return false;
        };
        if size == 0 {
            return false;
        }
        let mut t = p;
        if vint(area, &mut t) == Some(kind) && t <= p + size {
            return true;
        }
        at = p + size;
    }
    false
}

#[cfg(test)]
#[path = "rar_tests.rs"]
mod tests;

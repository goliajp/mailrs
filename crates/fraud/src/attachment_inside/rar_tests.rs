//! The RAR header walk on hostile shapes: every declared length checked
//! against the bytes there, every loop advancing.

use super::*;
use crate::attachment_inside::archive_tests::{rar5, rar5_block, rar5_file, vint as enc};

#[test]
fn a_variable_integer_is_bounded() {
    let mut at = 0;
    assert_eq!(vint(&enc(300), &mut at), Some(300));
    assert_eq!(at, 2);
    let mut at = 0;
    assert_eq!(vint(&enc(u64::MAX), &mut at), Some(u64::MAX));
    // an eleventh byte, or a tenth that overflows, is no integer
    let mut at = 0;
    assert_eq!(vint(&[0xff; 11], &mut at), None);
    let mut tenth = vec![0xff; 9];
    tenth.push(0x02);
    let mut at = 0;
    assert_eq!(vint(&tenth, &mut at), None);
    let mut at = 0;
    assert_eq!(vint(&[0x80], &mut at), None);
}

#[test]
fn declared_sizes_past_the_end_stop_the_walk() {
    // a header size of 2^62
    let mut big = RAR5.to_vec();
    big.extend_from_slice(&[0; 4]);
    big.extend(enc(1 << 62));
    big.extend_from_slice(&[1, 0, 0]);
    let l = listing(&big).expect("rar");
    assert!(!l.whole && l.entries.is_empty());
    // a name longer than the header
    let mut fields = vec![2, 0];
    fields.extend([0, 0, 0x20, 0, 0]);
    fields.extend(enc(1 << 20));
    fields.extend_from_slice(b"a.exe");
    let mut long = RAR5.to_vec();
    long.extend(rar5_block(&fields, &[]));
    let l = listing(&long).expect("rar");
    assert!(!l.whole && l.entries.is_empty());
    // an extra area larger than the header
    let mut extra = RAR5.to_vec();
    extra.extend(rar5_block(&[2, 1, 0x7f, 0], &[]));
    assert!(!listing(&extra).expect("rar").whole);
}

#[test]
fn a_zero_sized_header_does_not_loop() {
    let mut zero = RAR5.to_vec();
    zero.extend_from_slice(&[0, 0, 0, 0, 0]);
    assert!(!listing(&zero).expect("rar").whole);
    let mut zero4 = RAR4.to_vec();
    zero4.extend_from_slice(&[0, 0, 0x7a, 0, 0, 0, 0]);
    assert!(!listing(&zero4).expect("rar").whole);
}

#[test]
fn a_rar_4_long_block_skips_its_data_and_a_short_one_stops() {
    // a sub-block (0x7a) with 3 data bytes, then the end block
    let mut b = RAR4.to_vec();
    b.extend_from_slice(&[0, 0, 0x7a, 0x00, 0x80, 11, 0, 3, 0, 0, 0]);
    b.extend_from_slice(&[9, 9, 9]);
    b.extend_from_slice(&[0, 0, 0x7b, 0, 0x40, 7, 0]);
    assert!(listing(&b).expect("rar").whole);
    // the same block declaring 4 GiB of data
    let mut huge = RAR4.to_vec();
    huge.extend_from_slice(&[0, 0, 0x7a, 0x00, 0x80, 11, 0, 0xff, 0xff, 0xff, 0xff]);
    assert!(!listing(&huge).expect("rar").whole);
}

#[test]
fn the_entry_count_is_capped() {
    let mut many = RAR5.to_vec();
    let one = rar5_file("a.txt", false, false);
    for _ in 0..ENTRIES_MAX + 5 {
        many.extend_from_slice(&one);
    }
    let l = listing(&many).expect("rar");
    assert_eq!(l.entries.len(), ENTRIES_MAX);
    assert!(!l.whole, "a capped listing is not a whole one");
}

#[test]
fn directories_and_service_blocks_are_not_members() {
    let mut b = RAR5.to_vec();
    b.extend(rar5_file("docs", true, false));
    // a service header (type 3) with two data bytes, skipped
    b.extend(rar5_block(&[3, 0x02, 2], &[0, 0]));
    b.extend(rar5_file("docs/a.pdf", false, false));
    b.extend(rar5_block(&[5, 0, 0], &[]));
    let l = listing(&b).expect("rar");
    assert!(l.whole);
    let names: Vec<(&str, bool)> = l
        .entries
        .iter()
        .map(|e| (e.name.as_str(), e.directory))
        .collect();
    assert_eq!(names, [("docs", true), ("docs/a.pdf", false)]);
    assert!(listing(&rar5(&[])).expect("rar").whole);
}

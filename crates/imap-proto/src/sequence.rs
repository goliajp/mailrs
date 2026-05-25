/// A parsed IMAP sequence set (RFC 3501 § 9 "sequence-set").
///
/// Sequence sets refer to messages by 1-based sequence number or UID.
/// `*` means the highest existing sequence number.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SequenceSet {
    /// `5` — a single number.
    Single(u32),
    /// `1:5` — closed range, both endpoints inclusive.
    Range(u32, u32),
    /// `5:*` — from `start` to the highest existing number.
    RangeFrom(u32),
    /// `*` or `*:*` — every existing number.
    All,
    /// `1,3,5:10` — comma-separated combination of the above. Order is
    /// preserved during parsing but normalized (sort + dedup) during UID
    /// expansion via [`sequence_set_to_uids`].
    List(Vec<SequenceSet>),
}

/// Parse an IMAP sequence-set string. Examples: `"1"`, `"1:5"`, `"5:*"`,
/// `"*"`, `"1,3,5:10"`. Returns an error message (not an error type) for
/// invalid input.
pub fn parse_sequence_set(input: &str) -> Result<SequenceSet, String> {
    let input = input.trim();
    if input.is_empty() {
        return Err("empty sequence set".into());
    }

    // check for comma-separated list
    if input.contains(',') {
        let parts: Result<Vec<SequenceSet>, String> = input
            .split(',')
            .map(|p| parse_sequence_set(p.trim()))
            .collect();
        return Ok(SequenceSet::List(parts?));
    }

    // check for range
    if let Some((start, end)) = input.split_once(':') {
        if end == "*" {
            if start == "*" {
                return Ok(SequenceSet::All);
            }
            let start: u32 = start
                .parse()
                .map_err(|_| format!("invalid number: {start}"))?;
            return Ok(SequenceSet::RangeFrom(start));
        }
        let start: u32 = start
            .parse()
            .map_err(|_| format!("invalid number: {start}"))?;
        let end: u32 = end.parse().map_err(|_| format!("invalid number: {end}"))?;
        return Ok(SequenceSet::Range(start, end));
    }

    // single value
    if input == "*" {
        return Ok(SequenceSet::All);
    }
    let n: u32 = input
        .parse()
        .map_err(|_| format!("invalid number: {input}"))?;
    Ok(SequenceSet::Single(n))
}

/// Expand a [`SequenceSet`] to a concrete sorted, deduplicated list of
/// numbers in `1..=max`. `max` represents the highest existing sequence
/// number / UID — used to resolve `*` and clamp out-of-range references.
pub fn sequence_set_to_uids(set: &SequenceSet, max: u32) -> Vec<u32> {
    match set {
        SequenceSet::Single(n) => {
            if *n <= max && *n >= 1 {
                vec![*n]
            } else {
                vec![]
            }
        }
        SequenceSet::Range(start, end) => {
            let s = (*start).max(1);
            let e = (*end).min(max);
            if s > e { vec![] } else { (s..=e).collect() }
        }
        SequenceSet::RangeFrom(start) => {
            let s = (*start).max(1);
            if s > max { vec![] } else { (s..=max).collect() }
        }
        SequenceSet::All => {
            if max >= 1 {
                (1..=max).collect()
            } else {
                vec![]
            }
        }
        SequenceSet::List(sets) => {
            let mut result: Vec<u32> = sets
                .iter()
                .flat_map(|s| sequence_set_to_uids(s, max))
                .collect();
            result.sort();
            result.dedup();
            result
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_single() {
        assert_eq!(parse_sequence_set("5").unwrap(), SequenceSet::Single(5));
    }

    #[test]
    fn parse_range() {
        assert_eq!(parse_sequence_set("1:5").unwrap(), SequenceSet::Range(1, 5));
    }

    #[test]
    fn parse_range_from() {
        assert_eq!(
            parse_sequence_set("5:*").unwrap(),
            SequenceSet::RangeFrom(5)
        );
    }

    #[test]
    fn parse_star() {
        assert_eq!(parse_sequence_set("*").unwrap(), SequenceSet::All);
    }

    #[test]
    fn parse_list() {
        let result = parse_sequence_set("1,3,5:10").unwrap();
        if let SequenceSet::List(parts) = result {
            assert_eq!(parts.len(), 3);
            assert_eq!(parts[0], SequenceSet::Single(1));
            assert_eq!(parts[1], SequenceSet::Single(3));
            assert_eq!(parts[2], SequenceSet::Range(5, 10));
        } else {
            panic!("expected List");
        }
    }

    #[test]
    fn sequence_set_to_uids_range() {
        let uids = sequence_set_to_uids(&SequenceSet::Range(2, 5), 10);
        assert_eq!(uids, vec![2, 3, 4, 5]);
    }

    #[test]
    fn sequence_set_to_uids_all() {
        let uids = sequence_set_to_uids(&SequenceSet::All, 3);
        assert_eq!(uids, vec![1, 2, 3]);
    }

    #[test]
    fn sequence_set_to_uids_range_from() {
        let uids = sequence_set_to_uids(&SequenceSet::RangeFrom(3), 5);
        assert_eq!(uids, vec![3, 4, 5]);
    }

    #[test]
    fn sequence_set_to_uids_single() {
        let uids = sequence_set_to_uids(&SequenceSet::Single(2), 5);
        assert_eq!(uids, vec![2]);
    }

    #[test]
    fn sequence_set_out_of_range() {
        let uids = sequence_set_to_uids(&SequenceSet::Single(10), 5);
        assert!(uids.is_empty());
    }

    // --- parse error cases ---

    #[test]
    fn parse_empty_returns_error() {
        assert!(parse_sequence_set("").is_err());
        assert!(parse_sequence_set("   ").is_err());
    }

    #[test]
    fn parse_invalid_number_returns_error() {
        assert!(parse_sequence_set("abc").is_err());
        assert!(parse_sequence_set("1:xyz").is_err());
        assert!(parse_sequence_set("foo:5").is_err());
    }

    // --- star:star → All ---

    #[test]
    fn parse_star_colon_star() {
        assert_eq!(parse_sequence_set("*:*").unwrap(), SequenceSet::All);
    }

    // --- list with RangeFrom and All ---

    #[test]
    fn parse_list_with_range_from() {
        let result = parse_sequence_set("1,5:*").unwrap();
        if let SequenceSet::List(parts) = result {
            assert_eq!(parts[0], SequenceSet::Single(1));
            assert_eq!(parts[1], SequenceSet::RangeFrom(5));
        } else {
            panic!("expected List");
        }
    }

    #[test]
    fn parse_list_with_star() {
        let result = parse_sequence_set("1,*").unwrap();
        if let SequenceSet::List(parts) = result {
            assert_eq!(parts[0], SequenceSet::Single(1));
            assert_eq!(parts[1], SequenceSet::All);
        } else {
            panic!("expected List");
        }
    }

    // --- sequence_set_to_uids edge cases ---

    #[test]
    fn sequence_set_to_uids_single_zero_is_empty() {
        // UIDs start at 1; 0 is never valid
        let uids = sequence_set_to_uids(&SequenceSet::Single(0), 10);
        assert!(uids.is_empty());
    }

    #[test]
    fn sequence_set_to_uids_all_with_zero_max() {
        let uids = sequence_set_to_uids(&SequenceSet::All, 0);
        assert!(uids.is_empty());
    }

    #[test]
    fn sequence_set_to_uids_range_from_exceeds_max() {
        let uids = sequence_set_to_uids(&SequenceSet::RangeFrom(10), 5);
        assert!(uids.is_empty());
    }

    #[test]
    fn sequence_set_to_uids_range_reversed_is_empty() {
        // start > end after clamping should yield empty
        let uids = sequence_set_to_uids(&SequenceSet::Range(8, 3), 10);
        assert!(uids.is_empty());
    }

    #[test]
    fn sequence_set_to_uids_range_clamped_to_max() {
        // range extends beyond max
        let uids = sequence_set_to_uids(&SequenceSet::Range(3, 20), 7);
        assert_eq!(uids, vec![3, 4, 5, 6, 7]);
    }

    #[test]
    fn sequence_set_to_uids_range_from_exact_max() {
        let uids = sequence_set_to_uids(&SequenceSet::RangeFrom(5), 5);
        assert_eq!(uids, vec![5]);
    }

    #[test]
    fn sequence_set_to_uids_list_deduplicates_and_sorts() {
        // "3,1,2,1" should yield [1, 2, 3]
        let set = SequenceSet::List(vec![
            SequenceSet::Single(3),
            SequenceSet::Single(1),
            SequenceSet::Single(2),
            SequenceSet::Single(1),
        ]);
        let uids = sequence_set_to_uids(&set, 10);
        assert_eq!(uids, vec![1, 2, 3]);
    }

    #[test]
    fn sequence_set_to_uids_list_overlapping_ranges() {
        // "1:3,2:5" should yield [1, 2, 3, 4, 5] without duplicates
        let set = SequenceSet::List(vec![SequenceSet::Range(1, 3), SequenceSet::Range(2, 5)]);
        let uids = sequence_set_to_uids(&set, 10);
        assert_eq!(uids, vec![1, 2, 3, 4, 5]);
    }

    #[test]
    fn sequence_set_to_uids_single_equal_max() {
        let uids = sequence_set_to_uids(&SequenceSet::Single(5), 5);
        assert_eq!(uids, vec![5]);
    }

    #[test]
    fn sequence_set_to_uids_all_single_message() {
        let uids = sequence_set_to_uids(&SequenceSet::All, 1);
        assert_eq!(uids, vec![1]);
    }

    // --- whitespace tolerance ---

    #[test]
    fn parse_sequence_set_trims_whitespace() {
        assert_eq!(parse_sequence_set("  5  ").unwrap(), SequenceSet::Single(5));
    }

    // --- additional edge-case tests ---

    #[test]
    fn parse_range_equal_start_end() {
        assert_eq!(parse_sequence_set("5:5").unwrap(), SequenceSet::Range(5, 5));
    }

    #[test]
    fn sequence_set_to_uids_range_equal_start_end() {
        let uids = sequence_set_to_uids(&SequenceSet::Range(3, 3), 10);
        assert_eq!(uids, vec![3]);
    }

    #[test]
    fn parse_single_one() {
        assert_eq!(parse_sequence_set("1").unwrap(), SequenceSet::Single(1));
    }

    #[test]
    fn parse_large_number() {
        assert_eq!(
            parse_sequence_set("4294967295").unwrap(),
            SequenceSet::Single(4294967295)
        );
    }

    #[test]
    fn parse_overflow_number_returns_error() {
        // u32::MAX + 1 should fail
        assert!(parse_sequence_set("4294967296").is_err());
    }

    #[test]
    fn parse_negative_number_returns_error() {
        assert!(parse_sequence_set("-1").is_err());
    }

    #[test]
    fn parse_list_single_element() {
        // "5" without comma should be Single, not List
        assert_eq!(parse_sequence_set("5").unwrap(), SequenceSet::Single(5));
    }

    #[test]
    fn parse_list_two_singles() {
        let result = parse_sequence_set("1,2").unwrap();
        if let SequenceSet::List(parts) = result {
            assert_eq!(parts.len(), 2);
            assert_eq!(parts[0], SequenceSet::Single(1));
            assert_eq!(parts[1], SequenceSet::Single(2));
        } else {
            panic!("expected List");
        }
    }

    #[test]
    fn parse_list_with_ranges_and_singles() {
        let result = parse_sequence_set("1,3:5,7,9:*").unwrap();
        if let SequenceSet::List(parts) = result {
            assert_eq!(parts.len(), 4);
            assert_eq!(parts[0], SequenceSet::Single(1));
            assert_eq!(parts[1], SequenceSet::Range(3, 5));
            assert_eq!(parts[2], SequenceSet::Single(7));
            assert_eq!(parts[3], SequenceSet::RangeFrom(9));
        } else {
            panic!("expected List");
        }
    }

    #[test]
    fn parse_list_invalid_element_returns_error() {
        assert!(parse_sequence_set("1,abc,3").is_err());
    }

    #[test]
    fn parse_range_invalid_start_returns_error() {
        assert!(parse_sequence_set("abc:5").is_err());
    }

    #[test]
    fn parse_range_invalid_end_returns_error() {
        assert!(parse_sequence_set("1:xyz").is_err());
    }

    #[test]
    fn sequence_set_to_uids_list_empty_subsets() {
        // all elements out of range → empty result
        let set = SequenceSet::List(vec![SequenceSet::Single(100), SequenceSet::Single(200)]);
        let uids = sequence_set_to_uids(&set, 10);
        assert!(uids.is_empty());
    }

    #[test]
    fn sequence_set_to_uids_range_start_zero_clamped() {
        // Range(0, 5) should clamp start to 1
        let uids = sequence_set_to_uids(&SequenceSet::Range(0, 5), 10);
        assert_eq!(uids, vec![1, 2, 3, 4, 5]);
    }

    #[test]
    fn sequence_set_to_uids_range_from_zero_clamped() {
        // RangeFrom(0) should clamp start to 1
        let uids = sequence_set_to_uids(&SequenceSet::RangeFrom(0), 3);
        assert_eq!(uids, vec![1, 2, 3]);
    }

    #[test]
    fn sequence_set_to_uids_list_with_all() {
        // list containing All + singles → full range deduplicated
        let set = SequenceSet::List(vec![SequenceSet::Single(2), SequenceSet::All]);
        let uids = sequence_set_to_uids(&set, 4);
        assert_eq!(uids, vec![1, 2, 3, 4]);
    }

    #[test]
    fn sequence_set_clone_and_eq() {
        let set = parse_sequence_set("1:5").unwrap();
        let cloned = set.clone();
        assert_eq!(set, cloned);
    }

    #[test]
    fn sequence_set_debug_format() {
        let set = SequenceSet::Single(42);
        let debug = format!("{:?}", set);
        assert!(debug.contains("Single"));
        assert!(debug.contains("42"));
    }

    #[test]
    fn parse_range_from_one() {
        assert_eq!(
            parse_sequence_set("1:*").unwrap(),
            SequenceSet::RangeFrom(1)
        );
    }

    #[test]
    fn sequence_set_to_uids_range_both_exceed_max() {
        // range entirely above max → empty
        let uids = sequence_set_to_uids(&SequenceSet::Range(10, 20), 5);
        assert!(uids.is_empty());
    }
}

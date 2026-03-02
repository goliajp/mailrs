/// IMAP sequence set representation
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SequenceSet {
    Single(u32),
    Range(u32, u32),
    RangeFrom(u32),
    All,
    List(Vec<SequenceSet>),
}

/// parse a sequence set string like "1", "1:5", "5:*", "*", "1,3,5:10"
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
            let start: u32 = start.parse().map_err(|_| format!("invalid number: {start}"))?;
            return Ok(SequenceSet::RangeFrom(start));
        }
        let start: u32 = start.parse().map_err(|_| format!("invalid number: {start}"))?;
        let end: u32 = end.parse().map_err(|_| format!("invalid number: {end}"))?;
        return Ok(SequenceSet::Range(start, end));
    }

    // single value
    if input == "*" {
        return Ok(SequenceSet::All);
    }
    let n: u32 = input.parse().map_err(|_| format!("invalid number: {input}"))?;
    Ok(SequenceSet::Single(n))
}

/// expand a sequence set to a list of numbers given a max value
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
            if s > e {
                vec![]
            } else {
                (s..=e).collect()
            }
        }
        SequenceSet::RangeFrom(start) => {
            let s = (*start).max(1);
            if s > max {
                vec![]
            } else {
                (s..=max).collect()
            }
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
        assert_eq!(
            parse_sequence_set("1:5").unwrap(),
            SequenceSet::Range(1, 5)
        );
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
}

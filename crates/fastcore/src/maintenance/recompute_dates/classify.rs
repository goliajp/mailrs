//! Why a stored date differs from what ingest computes from the file, and
//! whether that difference is one this route corrects.

use mailrs_inbound::dates::Zone;

/// What one message's file says, read the way ingest reads it.
#[derive(Debug, Clone, Copy)]
pub(crate) struct FromFile {
    /// `Date:` as `extract_headers` parses it, 0 when it does not.
    pub header: i64,
    pub zone: Option<Zone>,
    /// The delivery second in the file name.
    pub arrival: Option<i64>,
    /// What ingest stores today: `stored_date(header, blob_ref)`.
    pub want: i64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Verdict {
    Agree,
    /// Stored as 0: no date was known when it was written.
    Zero,
    /// Stored as the sender's local time read as UTC: the parser before
    /// 71986e7d ignored the zone.
    ZoneIgnored,
    /// `Date:` is `-0000` and the stored date is nine hours earlier.
    /// Nothing known writes that, so it is left alone.
    MinusZeroNineHoursEarly,
    /// Differs in a way none of the above explains; left alone.
    Unexplained,
}

impl Verdict {
    pub(crate) fn corrected(self) -> bool {
        matches!(self, Verdict::Zero | Verdict::ZoneIgnored)
    }
}

const NINE_HOURS: i64 = 9 * 3600;

pub(crate) fn classify(stored: i64, f: &FromFile) -> Verdict {
    if stored == f.want {
        return Verdict::Agree;
    }
    if f.zone == Some(Zone::Unknown) && f.header > 0 && stored == f.header - NINE_HOURS {
        return Verdict::MinusZeroNineHoursEarly;
    }
    if stored <= 0 && f.want > 0 {
        return Verdict::Zero;
    }
    if let Some(Zone::Offset(offset)) = f.zone
        && offset != 0
        && f.header > 0
    {
        // The local time the sender wrote, as the old parser kept it. Not
        // also that time capped at the arrival: a row stored at its arrival
        // matches that whenever the zone is east of UTC and delivery took
        // less than the offset, and production holds thousands of rows
        // stored at their arrival in every zone, which this does not explain.
        if stored == f.header + i64::from(offset) {
            return Verdict::ZoneIgnored;
        }
    }
    Verdict::Unexplained
}

/// How a difference nothing explains looks, for the report: stored at the
/// arrival, a whole number of hours off, or neither.
pub(crate) fn shape(stored: i64, f: &FromFile) -> String {
    let delta = stored - f.want;
    if Some(stored) == f.arrival {
        return "stored_at_arrival".into();
    }
    if f.header > 0 && f.want < f.header {
        return "header_after_arrival".into();
    }
    match delta % 3600 {
        0 => format!("{:+}h", delta / 3600),
        _ if delta.abs() < 3600 => "under_1h".into(),
        _ if delta > 0 => "later_other".into(),
        _ => "earlier_other".into(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // 2026-07-02 12:00:00 UTC
    const NOON: i64 = 1_782_993_600;
    const DAY_LATER: i64 = NOON + 86_400;

    fn file(header: i64, zone: Zone) -> FromFile {
        FromFile {
            header,
            zone: Some(zone),
            arrival: Some(DAY_LATER),
            want: header,
        }
    }

    #[test]
    fn local_time_read_as_utc_is_corrected_in_both_directions() {
        let tokyo = file(NOON, Zone::Offset(9 * 3600));
        assert_eq!(classify(NOON + 9 * 3600, &tokyo), Verdict::ZoneIgnored);
        let denver = file(NOON, Zone::Offset(-7 * 3600));
        assert_eq!(classify(NOON - 7 * 3600, &denver), Verdict::ZoneIgnored);
        let chicago = file(NOON, Zone::Offset(-5 * 3600));
        assert_eq!(classify(NOON - 5 * 3600, &chicago), Verdict::ZoneIgnored);
    }

    #[test]
    fn a_row_stored_at_its_arrival_is_not_taken_for_a_zone_error() {
        let mut tokyo = file(NOON, Zone::Offset(9 * 3600));
        tokyo.arrival = Some(NOON + 3600);
        assert_eq!(classify(NOON + 3600, &tokyo), Verdict::Unexplained);
        assert_eq!(shape(NOON + 3600, &tokyo), "stored_at_arrival");
    }

    #[test]
    fn minus_zero_nine_hours_early_is_left_alone() {
        let f = file(NOON, Zone::Unknown);
        assert_eq!(
            classify(NOON - NINE_HOURS, &f),
            Verdict::MinusZeroNineHoursEarly
        );
        assert!(!Verdict::MinusZeroNineHoursEarly.corrected());
    }

    #[test]
    fn zero_is_corrected_and_anything_else_is_not() {
        let f = file(NOON, Zone::Offset(0));
        assert_eq!(classify(0, &f), Verdict::Zero);
        assert_eq!(classify(NOON, &f), Verdict::Agree);
        assert_eq!(classify(NOON + 3 * 3600, &f), Verdict::Unexplained);
        let tokyo = file(NOON, Zone::Offset(9 * 3600));
        assert_eq!(classify(NOON + 3600, &tokyo), Verdict::Unexplained);
    }
}

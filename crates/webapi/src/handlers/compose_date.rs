//! The Date header of an outgoing message.

/// RFC 5322 date string in the shape smtpd wants.
pub(crate) fn rfc5322_date(epoch: i64) -> String {
    // Manual format so we don't pull in chrono here — the outbound
    // queue consumer re-parses this defensively anyway.
    // Sat, 02 Jul 2026 12:34:56 +0000
    static WEEKDAYS: [&str; 7] = ["Sun", "Mon", "Tue", "Wed", "Thu", "Fri", "Sat"];
    static MONTHS: [&str; 12] = [
        "Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec",
    ];
    // Simple date math — fine within the current epoch range.
    let secs = epoch.max(0) as u64;
    let mut days = secs / 86_400;
    let sec_of_day = secs % 86_400;
    let hour = (sec_of_day / 3600) as u32;
    let minute = ((sec_of_day % 3600) / 60) as u32;
    let second = (sec_of_day % 60) as u32;
    // 1970-01-01 was Thursday (index 4)
    let weekday = WEEKDAYS[((days + 4) % 7) as usize];
    let mut year: u32 = 1970;
    while {
        let leap =
            (year.is_multiple_of(4) && !year.is_multiple_of(100)) || year.is_multiple_of(400);
        let ydays = if leap { 366 } else { 365 };
        days >= ydays
    } {
        let leap =
            (year.is_multiple_of(4) && !year.is_multiple_of(100)) || year.is_multiple_of(400);
        days -= if leap { 366 } else { 365 };
        year += 1;
    }
    let leap = (year.is_multiple_of(4) && !year.is_multiple_of(100)) || year.is_multiple_of(400);
    let month_lengths = [
        31,
        if leap { 29 } else { 28 },
        31,
        30,
        31,
        30,
        31,
        31,
        30,
        31,
        30,
        31,
    ];
    let mut month: usize = 0;
    while month < 12 && days >= month_lengths[month] {
        days -= month_lengths[month];
        month += 1;
    }
    let day = days + 1;
    format!(
        "{weekday}, {day:02} {mon} {year} {hour:02}:{minute:02}:{second:02} +0000",
        mon = MONTHS[month],
    )
}

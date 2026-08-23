package jp.golia.mailrs.accounts

/**
 * Reading a `Date:` header — RFC 5322 §3.3.
 *
 * A list row shows when mail arrived, and sorts on it. Getting this
 * wrong is quiet: the row shows a plausible time that is hours out, or
 * the list orders itself by nothing at all.
 */
object MailDate {
    /**
     * Seconds since the epoch, or null.
     *
     * Null rather than "now": a message with an unreadable date shown
     * as having just arrived jumps to the top of the list and stays
     * there, which is worse than showing no date.
     */
    fun epochSeconds(header: String): Long? {
        val t = header.trim().substringBefore('(').trim() // drop a trailing comment
        if (t.isEmpty()) return null

        // The day name is optional, and when present it is followed by
        // a comma. Servers disagree about the space after it.
        val body = t.substringAfter(',', t).trim()
        val parts = body.split(Regex("\\s+"))
        if (parts.size < 5) return null

        val day = parts[0].toIntOrNull() ?: return null
        val month = MONTHS.indexOf(parts[1].lowercase().take(3)) + 1
        if (month == 0) return null
        val year = year(parts[2]) ?: return null
        val time = parts[3].split(":")
        if (time.size < 2) return null
        val hour = time[0].toIntOrNull() ?: return null
        val minute = time[1].toIntOrNull() ?: return null
        val second = if (time.size > 2) time[2].toIntOrNull() ?: 0 else 0
        val offset = offsetSeconds(parts[4]) ?: return null

        val days = daysFromCivil(year, month, day)
        return days * 86_400L + hour * 3600L + minute * 60L + second - offset
    }

    /**
     * A two-digit year, as RFC 5322 §4.3 says to read one.
     *
     * Obsolete and still in the wild. 50–99 is 19xx, 00–49 is 20xx;
     * reading `26` as year 26 puts the message two thousand years in
     * the past and sorts the whole list around it.
     */
    /**
     * The other direction: a `Date:` header.
     *
     * A numeric offset, never a zone name. `JST` and the rest are
     * obsolete by RFC 5322 §4.3 and ambiguous in practice — `CST` is
     * three different zones — so a receiving client is entitled to read
     * them as UTC, which moves a message by hours.
     */
    fun rfc5322(epochSeconds: Long, zone: java.util.TimeZone): String {
        val calendar = java.util.Calendar.getInstance(zone, java.util.Locale.US)
        calendar.timeInMillis = epochSeconds * 1000
        val days = listOf("Sun", "Mon", "Tue", "Wed", "Thu", "Fri", "Sat")
        val months = listOf(
            "Jan", "Feb", "Mar", "Apr", "May", "Jun",
            "Jul", "Aug", "Sep", "Oct", "Nov", "Dec",
        )
        val offset = zone.getOffset(calendar.timeInMillis) / 1000
        val sign = when {
            offset < 0 -> "-"
            else -> "+"
        }
        val magnitude = kotlin.math.abs(offset)
        return String.format(
            java.util.Locale.US,
            "%s, %d %s %04d %02d:%02d:%02d %s%02d%02d",
            days[calendar.get(java.util.Calendar.DAY_OF_WEEK) - 1],
            calendar.get(java.util.Calendar.DAY_OF_MONTH),
            months[calendar.get(java.util.Calendar.MONTH)],
            calendar.get(java.util.Calendar.YEAR),
            calendar.get(java.util.Calendar.HOUR_OF_DAY),
            calendar.get(java.util.Calendar.MINUTE),
            calendar.get(java.util.Calendar.SECOND),
            sign, magnitude / 3600, (magnitude % 3600) / 60,
        )
    }

    private fun year(s: String): Int? {
        val n = s.toIntOrNull() ?: return null
        return when {
            s.length == 4 -> n
            s.length == 2 -> if (n >= 50) 1900 + n else 2000 + n
            s.length == 3 -> 1900 + n // also obsolete, also seen
            else -> null
        }
    }

    /**
     * `+0900`, or one of the obsolete names.
     *
     * An unknown name is **not** zero: RFC 5322 §4.3 says to treat the
     * obsolete military zones as `-0000` because half the world got
     * their sign backwards, and guessing UTC for something unknown is
     * a silent thirteen-hour error.
     */
    private fun offsetSeconds(s: String): Long? {
        if (s.length == 5 && (s[0] == '+' || s[0] == '-')) {
            val h = s.substring(1, 3).toIntOrNull() ?: return null
            val m = s.substring(3, 5).toIntOrNull() ?: return null
            val magnitude = h * 3600L + m * 60L
            return if (s[0] == '-') -magnitude else magnitude
        }
        return when (s.uppercase()) {
            "UT", "GMT", "Z" -> 0L
            "EST" -> -5 * 3600L
            "EDT" -> -4 * 3600L
            "CST" -> -6 * 3600L
            "CDT" -> -5 * 3600L
            "MST" -> -7 * 3600L
            "MDT" -> -6 * 3600L
            "PST" -> -8 * 3600L
            "PDT" -> -7 * 3600L
            else -> null
        }
    }

    private val MONTHS = listOf(
        "jan", "feb", "mar", "apr", "may", "jun",
        "jul", "aug", "sep", "oct", "nov", "dec",
    )

    /**
     * Days from 1970-01-01 — Howard Hinnant's civil-from-days, which
     * is exact and has no library behind it to disagree with.
     */
    /** Seconds since the epoch for a UTC wall clock. */
    fun epochFromCivil(
        year: Int,
        month: Int,
        day: Int,
        hour: Int,
        minute: Int,
        second: Int,
    ): Long = daysFromCivil(year, month, day) * 86_400L + hour * 3600L + minute * 60L + second

    private fun daysFromCivil(y0: Int, m: Int, d: Int): Long {
        val y = if (m <= 2) y0 - 1 else y0
        val era = (if (y >= 0) y else y - 399) / 400
        val yoe = y - era * 400
        val doy = (153 * (if (m > 2) m - 3 else m + 9) + 2) / 5 + d - 1
        val doe = yoe * 365 + yoe / 4 - yoe / 100 + doy
        return era * 146_097L + doe - 719_468L
    }
}

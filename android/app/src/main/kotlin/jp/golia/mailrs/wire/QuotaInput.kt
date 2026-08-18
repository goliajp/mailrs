package jp.golia.mailrs.wire

/**
 * What an operator types into the storage-limit field, in bytes.
 *
 * Gigabytes, because that is the unit a mail quota is set in and a
 * field asking for bytes invites a typo three orders of magnitude
 * wide. Decimal gigabytes (10^9), matching `humanSize`, so what is
 * typed and what the row shows afterwards are the same number.
 *
 * **Empty and zero both mean no limit** — the server reads 0 that way
 * and the detail screen prints it as "No limit", so the field can be
 * cleared to lift a cap rather than needing a separate gesture.
 */
object QuotaInput {
    private const val GB = 1_000_000_000.0

    /**
     * `null` when the text is not a number — the caller keeps the
     * dialog open rather than sending a guess. A negative is not a
     * number here either: there is no such thing as less than no mail.
     */
    fun parse(text: String): Long? {
        val trimmed = text.trim()
        if (trimmed.isEmpty()) return 0
        val gb = trimmed.toDoubleOrNull() ?: return null
        if (gb < 0 || gb.isNaN() || gb.isInfinite()) return null
        return (gb * GB).toLong()
    }

    /**
     * What to put in the field when it opens, so an operator changing
     * 2 GB to 3 GB types one character.
     *
     * No limit prefills empty rather than "0": the field's own hint
     * says what empty means, and a literal 0 reads like a mailbox that
     * can hold nothing.
     */
    fun display(bytes: Long?): String {
        val b = bytes ?: 0
        if (b <= 0) return ""
        val gb = b / GB
        return if (gb == Math.floor(gb)) gb.toLong().toString() else "%.2f".format(gb).trimEnd('0').trimEnd('.')
    }
}

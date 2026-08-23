package jp.golia.mailrs.accounts

/**
 * A colour per mailbox, so a merged list can say which is which.
 *
 * Derived from the id rather than stored: the same account is the same
 * colour on every launch, and there is nothing to keep in step. A fold
 * rather than [String.hashCode] — the JVM's is stable, but the iOS side
 * had to fold to get the same guarantee, and the two lists should not
 * disagree about which mailbox is blue.
 *
 * Colour is never the only thing saying which account a row came from:
 * the row's own detail line says it in words, because a dot means
 * nothing to somebody who cannot tell these hues apart.
 */
object AccountColour {
    val palette = listOf(
        "#4285f4", "#12b7f5", "#ea4335", "#34a853", "#a142f4",
        "#f4b400", "#ff6d00", "#00897b",
    )

    fun forId(id: String): String {
        var h = -0x340d631b7bdddcdbL // 0xcbf29ce484222325
        for (b in id.toByteArray()) {
            h = h xor (b.toLong() and 0xff)
            h *= 0x100000001b3L
        }
        val index = ((h % palette.size) + palette.size) % palette.size
        return palette[index.toInt()]
    }
}

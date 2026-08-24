package jp.golia.mailrs.accounts

/**
 * How large a message this client can send.
 *
 * There are two limits and only one of them is the server's.
 *
 * **The server's** is what it will accept — 25 MB at most providers,
 * less at many — and it is measured on the *encoded* message, which
 * base64 makes a third larger than the files. A person who attaches
 * 20 MB of photos sends 27 MB and is refused, which reads as the
 * client losing the message.
 *
 * **This client used to have one too**, and it was memory: the message
 * was built as one string and dot-stuffed into another before it
 * reached the socket, so a 25 MB attachment was several times that in
 * memory at once — on a phone, a process the system kills, and a kill
 * mid-send looks exactly like mail that vanished. That limit is gone:
 * `OutgoingMessage.pieces` hands the socket 57 bytes of a file at a
 * time and `DotStuffer` stuffs each piece as it passes, so nothing
 * larger than a line is held.
 *
 * What remains is the server's, and it is worth stating before the
 * send rather than discovering during it — a message refused here is
 * one somebody still has.
 */
object OutgoingLimits {
    /** What most providers accept, on the encoded message. */
    const val ENCODED_MAX = 25L * 1000 * 1000

    /** base64 is four bytes out for every three in, plus line breaks. */
    fun encodedSize(bytes: Long): Long = bytes / 3 * 4 + bytes % 3 * 2 + bytes / 57 * 2

    sealed interface Verdict {
        data object Ok : Verdict

        /**
         * Named in the units the person chose the files in, not in the
         * encoded ones: they attached 26 MB of photos, and telling
         * them the message is 35 MB is telling them about arithmetic
         * they did not do.
         */
        data class TooLarge(val attachedBytes: Long, val limitBytes: Long) : Verdict
    }

    /**
     * @param limit the encoded ceiling. Overridable because a server
     *   that says its own limit in the EHLO `SIZE` capability has told
     *   the truth and this default has only guessed.
     */
    fun check(draft: OutgoingMessage.Draft, limit: Long = ENCODED_MAX): Verdict {
        val attached = draft.attachments.sumOf { it.bytes.size.toLong() }
        if (attached == 0L) return Verdict.Ok
        val encoded = encodedSize(attached) + draft.body.length
        if (encoded <= limit) return Verdict.Ok
        // The limit is reported back in raw bytes too, so a screen can
        // say "22 MB of 25" rather than mixing the two scales.
        return Verdict.TooLarge(attached, limit / 4 * 3)
    }
}

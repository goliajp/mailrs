package jp.golia.mailrs.wire

/**
 * The Send list's rows: sent mail joined with its delivery status.
 *
 * Ported from `ios/Mailrs/Wire/SendJoin.swift`, which ports
 * `web/src/components/send-list/send-model.ts` — where the semantics
 * were paid for. Two sources contribute rows, deduped on Message-ID:
 * the sent axis (`/api/mail/sent`) holds what the maildir sweep has
 * filed, and the Send projection (`/api/mail/sends`) holds delivery
 * status. Either can be missing the other's rows — a send that just
 * left has no maildir copy yet, and most old mail predates the
 * projection entirely.
 */
object SendJoin {

    data class Row(
        val threadId: String,
        val uid: Int?,
        val subject: String,
        val to: String,
        val date: Long,
        /**
         * Null for mail that predates the projection. Absence says
         * nothing rather than claiming delivery — the honest rendering
         * the web view settled on.
         */
        val status: String?,
        val key: String,
    )

    /**
     * Normalise a Message-ID for comparison: no brackets, no case.
     *
     * Both sides store it bare, but if either ever gains brackets the
     * join fails *silently* — every row simply loses its status — which
     * is why normalisation is unconditional rather than trusted.
     */
    fun joinKey(raw: String): String =
        raw.trim().removePrefix("<").removeSuffix(">").lowercase()

    fun join(messages: List<Wire.SentMessage>, sends: List<Wire.Send>): List<Row> {
        // Index the projection by the *original* message where a resend
        // chain exists — `resent_from` points at it — keeping the newest
        // attempt, whose status is the one that matters.
        val byMessage = HashMap<String, Wire.Send>()
        for (send in sends) {
            val key = joinKey(send.resentFrom ?: send.sendId)
            if (key.isEmpty()) continue
            val held = byMessage[key]
            if (held != null && held.createdAt > send.createdAt) continue
            byMessage[key] = send
        }

        val rows = LinkedHashMap<String, Row>()
        for (message in messages) {
            val key = joinKey(message.messageId)
            if (key.isEmpty()) continue
            rows[key] = Row(
                threadId = message.threadId,
                uid = message.uid,
                subject = message.subject,
                to = message.to,
                date = message.internalDate,
                status = byMessage[key]?.status,
                key = key,
            )
        }

        // Sends the sweep has not filed yet. Without this pass a send
        // that succeeded — accepted by the remote, row written — is
        // absent from the only screen that would show it.
        for ((key, send) in byMessage) {
            if (rows.containsKey(key)) continue
            rows[key] = Row(
                threadId = send.threadId,
                uid = null,
                subject = send.subject,
                to = send.to.joinToString(", "),
                date = send.createdAt,
                status = send.status,
                key = key,
            )
        }

        return rows.values.sortedByDescending { it.date }
    }
}

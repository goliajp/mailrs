package jp.golia.mailrs.wire

import jp.golia.mailrs.R

/**
 * One look at the mailbox: how many are unread, and if that rose, who the
 * newest is from and what it is about.
 *
 * The periodic worker runs it, and so does a UnifiedPush ping. The ping
 * says only that the phone should look, so everything the notification
 * shows comes from here, over the app's own signed-in connection.
 */
class NewMailCheck(
    private val unseen: suspend () -> Int?,
    private val inbox: suspend () -> List<Wire.Conversation>?,
) {
    /** What to show when something arrived. */
    data class Notice(
        val title: String,
        val text: String,
        val summary: String?,
        val threadId: String?,
        val channelId: String,
    )

    /**
     * @param count what to remember as seen
     * @param inbox the inbox, when it was fetched: only when something arrived
     */
    data class Found(val count: Int, val inbox: List<Wire.Conversation>?, val notice: Notice?)

    /** Null when the count could not be fetched: try again, remember nothing. */
    suspend fun run(previous: Int?): Found? {
        val count = unseen() ?: return null
        val arrived = NewMailRule.arrived(previous, count) ?: return Found(count, null, null)
        val list = inbox()
        val newest = list?.filter { it.unreadCount > 0 }?.maxByOrNull { it.lastDate }
        val notice = Notice(
            title = newest?.let { SenderIdentity.readableName(it.participants.firstOrNull().orEmpty()) } ?: "Mailrs",
            text = newest?.subject?.ifBlank { Texts.get(R.string.mail_no_subject) } ?: NewMailRule.text(arrived),
            summary = summaryOf(arrived),
            threadId = newest?.threadId,
            channelId = NewMailRule.channelFor(newest?.importanceLevel.orEmpty()),
        )
        return Found(count, list, notice)
    }

    // with one message the sender and subject are the whole story
    private fun summaryOf(arrived: Int): String? {
        if (arrived > 1) return NewMailRule.text(arrived)
        return null
    }
}

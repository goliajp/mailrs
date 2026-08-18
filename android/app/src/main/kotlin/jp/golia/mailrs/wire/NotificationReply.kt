package jp.golia.mailrs.wire

/**
 * Answering from the notification shade.
 *
 * Android's direct reply is the one place a person answers mail without
 * the app coming to the front, and what it sends has to be decided from
 * the thread rather than from the notification — a notification carries
 * a name and a subject line, and a reply needs an address, a threading
 * id, and the subject the thread already has.
 *
 * Pure over the thread, so the rules are testable without a shade.
 */
object NotificationReply {

    data class Send(
        val to: List<String>,
        val subject: String,
        val inReplyTo: String?,
        val body: String,
    )

    /**
     * What a typed answer becomes, or null when there is nothing to
     * answer.
     *
     * **The newest message that is not mine**, not simply the newest:
     * the last word in a thread is often one's own, and replying to it
     * would address the answer back at the person writing it. Null when
     * every message is mine — there is no correspondent to reply to,
     * and a reply addressed to oneself is worse than no action.
     */
    fun of(messages: List<Wire.Message>, myAddress: String, typed: String): Send? {
        val mine = myAddress.lowercase()
        val answering = messages
            .sortedBy { it.internalDate }
            .lastOrNull { SenderIdentity.emailOf(it.sender).lowercase() != mine }
            ?: return null
        return Send(
            to = ReplyRecipients.reply(answering.sender),
            subject = ReplyRecipients.subject(answering.subject),
            inReplyTo = answering.messageId,
            // Quoted, like a reply written in the app: an answer read
            // days later out of a long thread needs what it answers,
            // and the shade is the one place the writer cannot see it.
            body = typed + ReplyRecipients.quote(
                answering.sender,
                answering.internalDate,
                answering.textBody.orEmpty(),
            ),
        )
    }
}

package jp.golia.mailrs.wire

/**
 * Whether a check found new mail worth saying something about.
 *
 * Push is not available to this app — the server speaks APNs and an FCM
 * sender needs a Firebase project that does not exist yet — so a mail
 * client that never mentions arriving mail is what is left. A periodic
 * check is the other thing Android offers, and this is the one decision
 * inside it, kept out of the worker so it can be read and tested.
 *
 * **Only a rise counts.** The count falls whenever mail is read on any
 * device, and a client that notified on every change would announce
 * somebody else's phone catching up. It also has to survive the first
 * run, when nothing has been seen yet: there is no "before", and
 * announcing the entire unread mailbox as new is the wrong first
 * impression.
 */
object NewMailRule {

    /**
     * @param previous what the last check saw, or null on the first one
     * @param current what this check saw
     * @return how many arrived, or null when nothing should be said
     */
    fun arrived(previous: Int?, current: Int): Int? {
        // First run: record and stay quiet. The unread mailbox is not
        // news, it is the backlog.
        if (previous == null) return null
        if (current <= previous) return null
        return current - previous
    }

    /**
     * What the notification says.
     *
     * A count and nothing else. Sender and subject would be the useful
     * version and this check does not have them — it asks for a number
     * — and inventing a subject line from nothing is worse than a
     * number that is true.
     */
    fun text(count: Int): String = if (count == 1) "1 new message" else "$count new messages"

    /**
     * Which notification channel a thread's arrival belongs in.
     *
     * Two channels, because a channel is the only thing a person can
     * actually tune: with one, silencing the ordinary silences the
     * important as well, and that is the choice this app makes on
     * their behalf if it ships a single channel. The list already
     * marks these two levels with an icon, so the notification is
     * saying the same thing the inbox says rather than inventing a
     * distinction.
     */
    fun channelFor(importanceLevel: String): String = when (importanceLevel) {
        "critical", "important" -> IMPORTANT_CHANNEL
        else -> ORDINARY_CHANNEL
    }

    const val ORDINARY_CHANNEL = "new-mail"
    const val IMPORTANT_CHANNEL = "important-mail"
}

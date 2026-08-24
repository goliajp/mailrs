package jp.golia.mailrs.accounts

/**
 * Which commands a move needs on this particular server.
 *
 * `MOVE` (RFC 6851) where the server has it, and the older three-step
 * dance where it does not — and the difference matters more than it
 * looks:
 *
 * **A bare `EXPUNGE` removes every message in the folder flagged
 * `\Deleted`**, including ones somebody else's client flagged and has
 * not expunged yet. `UID EXPUNGE` (RFC 4315) removes only the one
 * named. Where neither `MOVE` nor `UIDPLUS` is offered, the message is
 * flagged and **left** rather than expunged: it disappears from the
 * list either way, and no other message is taken with it.
 *
 * Decided here rather than inside the session because it is the part
 * that varies between servers and the part that can lose somebody
 * else's mail, and neither of those should need a socket to check.
 */
object MovePlan {
    sealed interface Step {
        /** Send this and wait for its tagged completion. */
        data class Command(val text: String) : Step

        /** `UID STORE … +FLAGS (\Deleted)`, which the session owns. */
        data object MarkDeleted : Step
    }

    fun steps(uid: Long, folder: String, capabilities: Set<String>): List<Step> {
        if ("MOVE" in capabilities) {
            return listOf(Step.Command("UID MOVE $uid ${Imap.quoted(folder)}"))
        }
        val out = mutableListOf<Step>(
            Step.Command("UID COPY $uid ${Imap.quoted(folder)}"),
            Step.MarkDeleted,
        )
        // Only where the server can be told *which* one.
        if ("UIDPLUS" in capabilities) out.add(Step.Command("UID EXPUNGE $uid"))
        return out
    }
}

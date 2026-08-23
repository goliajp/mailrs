package jp.golia.mailrs

import jp.golia.mailrs.wire.Wire

/**
 * The message being written, and what survives leaving the screen.
 *
 * Split from `MailState.kt` at the file-size gate along the seam that
 * was already there: everything here is about composing, and nothing
 * in it is read by the list.
 */
/**
 * A message being written.
 *
 * `id` exists so the composer's fields reset when a *different*
 * draft opens and not on every recomposition — `remember(draft.id)`
 * rather than `remember(Unit)`, which would keep the previous
 * reply's text when you opened a second one.
 */
/**
 * The message being written, held here and nowhere else.
 *
 * The composer used to mirror these into its own `remember`d state
 * and hand them back on send. That is the pattern this codebase has
 * a rule against (`frontend/no-rq-mirror.md`), and it had a second
 * cost here: the back gesture cancels through the shell, which
 * cannot see a screen's local variables, so leaving by the gesture
 * everybody uses would have thrown the text away.
 *
 * `serverId` is null until the draft has been saved once; after
 * that it is reused, or one message leaves a trail of drafts.
 */
@kotlinx.serialization.Serializable
data class Draft(
    val id: Int,
    val to: String = "",
    val cc: String = "",
    val bcc: String = "",
    val subject: String = "",
    val body: String = "",
    val inReplyTo: String? = null,
    val replyToThreadId: String? = null,
    val serverId: Long? = null,
    /**
     * The message being forwarded, by uid.
     *
     * Set only for a forward. The server re-extracts that message's
     * attachments and sends them along, so what is passed on is what
     * arrived rather than a copy this phone had to fetch first.
     */
    val forwardFrom: Int? = null,
    /**
     * The send this is a re-edit of, and which of its files to keep.
     *
     * The bytes stay on the server — a re-edit describes its
     * attachments rather than downloading and re-uploading them. Null
     * `redraftKeep` keeps every carried file and an empty list keeps
     * none, which is the distinction the handler makes and the reason
     * this is nullable: collapsing them would silently re-attach files
     * somebody had just removed.
     */
    val redraftOf: String? = null,
    /**
     * Which address it leaves by, or empty for the signed-in one.
     *
     * A reply to mail that arrived at a connected Gmail has to go out
     * through that Gmail — sent from anywhere else it lands in the
     * conversation as a stranger.
     */
    val from: String = "",
    val carried: List<Wire.RedraftAttachment> = emptyList(),
    val carriedDropped: Set<Int> = emptySet(),
    /**
     * Files picked to go with it.
     *
     * In memory only, and deliberately: a server draft has nowhere
     * to keep an attachment, and a `content://` URI granted to this
     * activity does not survive the process — a draft reopened
     * tomorrow with a file it can no longer read would be worse than
     * one that says it has none.
     */
    @kotlinx.serialization.Transient
    val attachments: List<Attached> = emptyList(),
) {
    /** Nothing typed and nothing quoted: not worth saving. */
    val isEmpty: Boolean
        get() = to.isBlank() && cc.isBlank() && bcc.isBlank() &&
            subject.isBlank() && body.isBlank()
}

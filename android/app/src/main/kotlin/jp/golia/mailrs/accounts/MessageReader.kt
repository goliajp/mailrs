package jp.golia.mailrs.accounts

/**
 * Fetching one message to read it.
 *
 * Bodies are not stored — they are fetched when a message is opened and
 * kept only while it is on screen. A phone that keeps every body it has
 * ever shown fills up, and the ones worth keeping are the ones somebody
 * chose to keep.
 */
object MessageReader {
    /** How a session is made; injectable like the others. */
    /** Where IMAP connections are kept between messages. */
    internal var pool: ImapPool = ImapPool.shared
    data class Loaded(
        /**
         * Always text by the time it gets here: markup is turned into
         * text rather than rendered, so no message can ask another
         * server for an image and report that it was read.
         */
        val text: String,
        /**
         * Whether the text came out of markup. Shown, because a message
         * that reads oddly is easier to forgive when it says where it
         * came from.
         */
        val fromHtml: Boolean,
        /**
         * The message's own headers, for replying.
         *
         * Read here rather than from the list row: the row has what a
         * row shows, and a reply needs `Reply-To` and `References`,
         * which no list has ever displayed.
         */
        val headers: MessageHeaders.Parsed = MessageHeaders.Parsed(),
        /**
         * What came with it. Out of the same bytes the body came from,
         * so listing them costs nothing beyond the fetch already made
         * — a second request to find out whether there is an
         * attachment is a second request on somebody's data.
         */
        val attachments: List<MessageAttachments.Attachment> = emptyList(),
        /**
         * Set when only the beginning of the message was fetched.
         *
         * **The screen must say so.** The text will usually be
         * complete — it comes before the attachments in nearly every
         * message — but the attachment list will not be, and a list
         * that is silently short is worse than one that is absent.
         */
        val partial: Boolean = false,
        /** What the whole message weighs, for the offer to fetch it. */
        val size: Long? = null,
    )

    /** What a reader gets: the message, or a sentence about why not. */
    sealed class Outcome {
        data class Ok(val loaded: Loaded) : Outcome()
        data class Failed(val why: String) : Outcome()
    }

    suspend fun load(
        account: MailAccount,
        row: MailboxRow,
        store: AccountStore,
        /** Set when the reader has asked for the whole of a large one. */
        wholeMessage: Boolean = false,
    ): Outcome {
        val secret = store.secret(account.id)
            ?: return Outcome.Failed("Sign in again to read this account")
        // Through the pool. Opening a message is the wait a person
        // notices most, and most of it used to be a TLS handshake and
        // a LOGIN for one FETCH. The session is **not** closed here.
        return try {
            pool.use(account, secret) { session ->
                session.select(row.folder)
                val plan = FetchWhole.decide(row.size, wholeMessage)
                val raw = session.fetchRaw(row.uid, plan)
                // Marked read only after the body is in hand: a fetch
                // that fails should leave the message unread, or a
                // server that was briefly unwell quietly empties
                // somebody's unread count.
                if (!row.seen) {
                    runCatching { session.markSeen(row.uid) }
                    store.setRowSeen(row, true)
                }
                Outcome.Ok(
                    display(raw).copy(
                        partial = plan is FetchWhole.Plan.Beginning,
                        size = row.size,
                    ),
                )
            }
        } catch (e: Exception) {
            Outcome.Failed("Could not open this message")
        }
    }

    /** The body, as text. */
    fun display(raw: ByteArray): Loaded {
        val body = MessageBody.extract(raw)
        val headers = MessageHeaders.parse(String(raw, Charsets.UTF_8))
        val attached = MessageAttachments.of(raw)
        if (!body.isHtml) return Loaded(body.text, false, headers, attached)
        return Loaded(HtmlText.plain(body.text), true, headers, attached)
    }
}
